use crate::*;
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{ConnectInfo, Request, State},
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use murm_protocol::*;
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

#[derive(Default)]
pub(crate) struct Rates {
    entries: HashMap<(usize, Option<IpAddr>), (u64, u64)>,
}
struct ApiError {
    code: ProblemCode,
    retry: Option<u64>,
}
impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self {
        Self {
            code: e.code(),
            retry: None,
        }
    }
}
impl From<ProtocolError> for ApiError {
    fn from(e: ProtocolError) -> Self {
        Self {
            code: e.into(),
            retry: None,
        }
    }
}
impl From<ProblemCode> for ApiError {
    fn from(code: ProblemCode) -> Self {
        Self { code, retry: None }
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (
            StatusCode::from_u16(self.code.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            axum::Json(Problem::new(self.code)),
        )
            .into_response();
        response.headers_mut().insert(
            "content-type",
            HeaderValue::from_static("application/problem+json"),
        );
        if let Some(n) = self.retry {
            if let Ok(value) = HeaderValue::from_str(&n.to_string()) {
                response.headers_mut().insert("retry-after", value);
            }
        }
        response
    }
}
impl Rates {
    fn check(
        &mut self,
        op: Operation,
        ip: IpAddr,
        now: u64,
        c: &RelayConfig,
    ) -> Result<(), ApiError> {
        self.entries.retain(|(i, _), (window, _)| {
            c.info
                .rate_limits
                .get(*i)
                .is_some_and(|r| *window == now / r.window_seconds)
        });
        let mut wait = 0;
        for (i, r) in c
            .info
            .rate_limits
            .iter()
            .enumerate()
            .filter(|(_, r)| r.operation == op)
        {
            let key = (
                i,
                if r.scope == RateScope::Ip {
                    Some(ip)
                } else {
                    None
                },
            );
            if !self.entries.contains_key(&key) && self.entries.len() >= 10000 {
                return Err(ProblemCode::TemporarilyUnavailable.into());
            }
            let window = now / r.window_seconds;
            let entry = self.entries.entry(key).or_insert((window, 0));
            entry.1 = entry.1.saturating_add(1);
            if entry.1 > r.requests {
                wait = wait.max(r.window_seconds - now % r.window_seconds);
            }
        }
        if wait > 0 {
            return Err(ApiError {
                code: ProblemCode::RateLimited,
                retry: Some(wait),
            });
        }
        Ok(())
    }
}
fn json_response(
    value: impl serde::Serialize,
    max: u64,
    status: StatusCode,
) -> Result<Response, ApiError> {
    let bytes = serde_json::to_vec(&value)
        .map_err(|_| ApiError::from(ProblemCode::TemporarilyUnavailable))?;
    if bytes.len() as u64 > max {
        return Err(ProblemCode::LimitExceeded.into());
    }
    let mut r = Response::new(Body::from(bytes));
    *r.status_mut() = status;
    r.headers_mut()
        .insert("content-type", HeaderValue::from_static("application/json"));
    Ok(r)
}
async fn decode<T: serde::de::DeserializeOwned + Send + 'static>(
    bytes: Vec<u8>,
    limits: ParseLimits,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(move || {
        let value = parse_json(&bytes, limits)?;
        serde_json::from_value(value).map_err(|_| ProtocolError::InvalidRequest)
    })
    .await
    .map_err(|_| ApiError::from(ProblemCode::TemporarilyUnavailable))?
    .map_err(ApiError::from)
}
async fn handle(
    State(s): State<Arc<RelayService>>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    request: Request,
) -> Result<Response, ApiError> {
    let path = request.uri().path().to_owned();
    let query = request.uri().query().unwrap_or("").to_owned();
    let method = request.method().as_str().to_owned();
    let op = match (method.as_str(), path.as_str()) {
        ("GET", "/info") => Operation::Info,
        ("GET", "/relays") => Operation::Relays,
        ("POST", "/events") => Operation::Publish,
        ("QUERY", "/events") | ("POST", "/events/query") => Operation::Query,
        ("QUERY" | "POST", "/events/sync") => Operation::Sync,
        ("GET", p)
            if p.starts_with("/events/") && !["/events/query", "/events/sync"].contains(&p) =>
        {
            Operation::Fetch
        }
        (_, p) if p == "/info" || p == "/relays" || p == "/events" || p.starts_with("/events/") => {
            return Ok(StatusCode::METHOD_NOT_ALLOWED.into_response());
        }
        _ => return Ok(StatusCode::NOT_FOUND.into_response()),
    };
    s.rates
        .lock()
        .map_err(|_| ApiError::from(ProblemCode::TemporarilyUnavailable))?
        .check(op, remote.ip(), s.clock.now_unix(), &s.config)?;
    let _permit = s
        .slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::from(ProblemCode::TemporarilyUnavailable))?;
    let max = s.config.info.limits.response_bytes;
    match op {
        Operation::Info => json_response(&s.config.info, max, StatusCode::OK),
        Operation::Relays => Err(ProblemCode::UnsupportedFeature.into()),
        Operation::Fetch => {
            let id = path
                .trim_start_matches("/events/")
                .parse::<EventId>()
                .map_err(|_| ApiError::from(ProblemCode::InvalidRequest))?;
            if query == "lookup=true" {
                return Err(ProblemCode::UnsupportedFeature.into());
            }
            if !query.is_empty() && query != "lookup=false" {
                return Err(ProblemCode::InvalidRequest.into());
            }
            json_response(s.fetch(id).await?, max, StatusCode::OK)
        }
        Operation::Publish | Operation::Query | Operation::Sync => {
            if !query.is_empty() {
                return Err(ProblemCode::InvalidRequest.into());
            }
            let content_type = request
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            let mut parts = content_type.split(';');
            if !parts
                .next()
                .is_some_and(|p| p.trim().eq_ignore_ascii_case("application/json"))
                || parts.any(|p| !p.trim().eq_ignore_ascii_case("charset=utf-8"))
                || request
                    .headers()
                    .get("content-encoding")
                    .is_some_and(|v| v != "identity")
            {
                return Err(ProblemCode::UnsupportedMediaType.into());
            }
            let mut limits = s.config.info.parse_limits();
            if op != Operation::Publish {
                limits.max_bytes = limits
                    .max_bytes
                    .min(s.config.info.limits.query_bytes as usize);
            }
            let bytes = to_bytes(request.into_body(), limits.max_bytes)
                .await
                .map_err(|_| ApiError::from(ProblemCode::ContentTooLarge))?
                .to_vec();
            match op {
                Operation::Publish => {
                    let receipt = s.submit(bytes).await?;
                    let status = if receipt.status == StoreStatus::Stored {
                        StatusCode::CREATED
                    } else {
                        StatusCode::OK
                    };
                    let location = format!("/events/{}", receipt.id);
                    let mut r = json_response(receipt, max, status)?;
                    if let Ok(value) = HeaderValue::from_str(&location) {
                        r.headers_mut().insert("location", value);
                    }
                    Ok(r)
                }
                Operation::Query => json_response(
                    s.query(decode(bytes, limits).await?).await?,
                    max,
                    StatusCode::OK,
                ),
                _ => json_response(
                    s.sync(decode(bytes, limits).await?).await?,
                    max,
                    StatusCode::OK,
                ),
            }
        }
    }
}
pub async fn serve(
    listener: tokio::net::TcpListener,
    service: Arc<RelayService>,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let router = Router::new().fallback(handle).with_state(service);
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await
}
