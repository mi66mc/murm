use crate::ClientError;
use murm_protocol::*;
use serde_json::Value;
use std::time::Duration;

#[derive(Clone)]
pub struct NetworkOptions {
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub max_response_bytes: usize,
    pub max_depth: usize,
}
impl Default for NetworkOptions {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(20),
            max_response_bytes: 4194304,
            max_depth: 32,
        }
    }
}
pub struct VerifiedQuery {
    pub events: Vec<VerifiedEvent>,
    pub rejections: Vec<(EventId, ProblemCode)>,
    pub next_cursor: Option<String>,
}
pub struct VerifiedSync {
    pub events: Vec<VerifiedEvent>,
    pub rejections: Vec<(EventId, ProblemCode)>,
    pub cursor: String,
    pub has_more: bool,
}
#[derive(Clone)]
pub struct RelayTransport {
    client: reqwest::Client,
    url: String,
    options: NetworkOptions,
}
pub fn validate_url(value: &str) -> Result<String, ClientError> {
    let url = reqwest::Url::parse(value).map_err(|_| ClientError::InvalidInput)?;
    let local = url.host_str().is_some_and(|s| {
        s == "localhost"
            || s.trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
        || (url.scheme() != "https" && !(url.scheme() == "http" && local))
    {
        return Err(ClientError::InvalidInput);
    }
    Ok(url.as_str().trim_end_matches('/').into())
}
async fn bounded(mut response: reqwest::Response, max: usize) -> Result<Vec<u8>, ClientError> {
    if response.content_length().is_some_and(|n| n > max as u64) {
        return Err(ClientError::Protocol(ProtocolError::ContentTooLarge));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| ClientError::Network)? {
        if bytes.len().saturating_add(chunk.len()) > max {
            return Err(ClientError::Protocol(ProtocolError::ContentTooLarge));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
type VerifiedEventSet=(Vec<VerifiedEvent>,Vec<(EventId,ProblemCode)>);
fn verify_values(
    value: &Value,
    info: &RelayInfo,
    limits: ParseLimits,
) -> Result<VerifiedEventSet, ClientError> {
    let values = value
        .get("events")
        .and_then(Value::as_array)
        .ok_or(ProtocolError::InvalidRequest)?;
    let mut events = Vec::new();
    let mut rejected = Vec::new();
    for value in values {
        let bytes = serde_json::to_vec(value).map_err(|_| ClientError::Network)?;
        let event = parse_and_verify(&bytes, limits)?;
        if event.event().kind != 1 {
            rejected.push((event.event().id, ProblemCode::UnsupportedKind));
            continue;
        }
        if validate_publication(&event).is_err() {
            if info.validated_kinds.contains(&1) {
                return Err(ProtocolError::InvalidPublication.into());
            }
            rejected.push((event.event().id, ProblemCode::InvalidEvent));
            continue;
        }
        events.push(event);
    }
    Ok((events, rejected))
}
impl RelayTransport {
    pub fn new(url: &str) -> Result<Self, ClientError> {
        Self::with_options(url, NetworkOptions::default())
    }
    pub fn with_options(url: &str, options: NetworkOptions) -> Result<Self, ClientError> {
        let url = validate_url(url)?;
        if options.max_response_bytes == 0
            || options.max_depth == 0
            || options.request_timeout.is_zero()
            || options.connect_timeout.is_zero()
        {
            return Err(ClientError::InvalidInput);
        }
        let client = reqwest::Client::builder()
            .connect_timeout(options.connect_timeout)
            .timeout(options.request_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| ClientError::Network)?;
        Ok(Self {
            client,
            url,
            options,
        })
    }
    fn limits(&self) -> ParseLimits {
        ParseLimits {
            max_bytes: self.options.max_response_bytes,
            max_depth: self.options.max_depth,
        }
    }
    async fn response(&self, response: reqwest::Response) -> Result<Vec<u8>, ClientError> {
        let status = response.status().as_u16();
        let wait = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(60);
        if status == 429 {
            return Err(ClientError::RetryAfter(wait));
        }
        let bytes = bounded(response, self.options.max_response_bytes).await?;
        if !(200..300).contains(&status) {
            let problem: Problem = serde_json::from_value(parse_json(&bytes, self.limits())?)
                .map_err(|_| ClientError::Network)?;
            if problem.status != status || problem.code.status() != status {
                return Err(ClientError::Network);
            }
            return Err(ClientError::Relay(problem.code));
        }
        Ok(bytes)
    }
    pub async fn info(&self) -> Result<RelayInfo, ClientError> {
        let response = self
            .client
            .get(format!("{}/info", self.url))
            .send()
            .await
            .map_err(|_| ClientError::Network)?;
        let bytes = self.response(response).await?;
        let info: RelayInfo = serde_json::from_value(parse_json(&bytes, self.limits())?)
            .map_err(|_| ClientError::Network)?;
        info.validate()?;
        Ok(info)
    }
    pub async fn submit(&self, event: &VerifiedEvent) -> Result<StoreReceipt, ClientError> {
        let response = self
            .client
            .post(format!("{}/events", self.url))
            .json(event)
            .send()
            .await
            .map_err(|_| ClientError::Network)?;
        let status = response.status().as_u16();
        let bytes = self.response(response).await?;
        let receipt: StoreReceipt = serde_json::from_value(parse_json(&bytes, self.limits())?)
            .map_err(|_| ClientError::Network)?;
        if receipt.id != event.event().id
            || !matches!(
                (status, receipt.status),
                (201, StoreStatus::Stored) | (200, StoreStatus::AlreadyStored)
            )
            || receipt.retain_until.is_some_and(|n| n > MAX_SAFE_INTEGER)
        {
            return Err(ClientError::Network);
        }
        Ok(receipt)
    }
    pub async fn fetch(&self, id: EventId) -> Result<VerifiedEvent, ClientError> {
        let response = self
            .client
            .get(format!("{}/events/{id}", self.url))
            .send()
            .await
            .map_err(|_| ClientError::Network)?;
        let bytes = self.response(response).await?;
        let limits = self.limits();
        tokio::task::spawn_blocking(move || {
            let e = parse_and_verify(&bytes, limits)?;
            if e.event().id != id {
                return Err(ProtocolError::InvalidId.into());
            }
            validate_publication(&e)?;
            Ok(e)
        })
        .await
        .map_err(|_| ClientError::Network)?
    }
    async fn request(
        &self,
        path: &str,
        alias: &str,
        body: &impl serde::Serialize,
        http_query: bool,
    ) -> Result<Vec<u8>, ClientError> {
        let method = if http_query {
            reqwest::Method::from_bytes(b"QUERY").map_err(|_| ClientError::Network)?
        } else {
            reqwest::Method::POST
        };
        let response = self
            .client
            .request(method, format!("{}{path}", self.url))
            .json(body)
            .send()
            .await
            .map_err(|_| ClientError::Network)?;
        let response = if http_query && matches!(response.status().as_u16(), 405 | 501) {
            self.client
                .post(format!("{}{alias}", self.url))
                .json(body)
                .send()
                .await
                .map_err(|_| ClientError::Network)?
        } else {
            response
        };
        self.response(response).await
    }
    pub async fn query(
        &self,
        q: &EventQuery,
        info: &RelayInfo,
    ) -> Result<VerifiedQuery, ClientError> {
        validate_query(q, info)?;
        if !info.features.http_query && !info.features.post_query {
            return Err(ProtocolError::UnsupportedFeature.into());
        }
        let bytes = self
            .request("/events", "/events/query", q, info.features.http_query)
            .await?;
        let info = info.clone();
        let limit = q.limit;
        let limits = self.limits();
        tokio::task::spawn_blocking(move || {
            let mut outer = limits;
            outer.max_depth = outer.max_depth.saturating_add(2);
            let value = parse_json(&bytes, outer)?;
            let (events, rejections) = verify_values(&value, &info, limits)?;
            if events.len() + rejections.len() > limit as usize {
                return Err(ProtocolError::InvalidRequest.into());
            }
            let cursor = value
                .get("next_cursor")
                .ok_or(ProtocolError::InvalidRequest)?;
            let next_cursor = if cursor.is_null() {
                None
            } else {
                Some(
                    cursor
                        .as_str()
                        .ok_or(ProtocolError::InvalidRequest)?
                        .to_owned(),
                )
            };
            if next_cursor
                .as_ref()
                .is_some_and(|s| s.len() as u64 > info.limits.cursor_bytes)
            {
                return Err(ProtocolError::LimitExceeded.into());
            }
            Ok(VerifiedQuery {
                events,
                rejections,
                next_cursor,
            })
        })
        .await
        .map_err(|_| ClientError::Network)?
    }
    pub async fn sync(
        &self,
        q: &SyncRequest,
        info: &RelayInfo,
    ) -> Result<VerifiedSync, ClientError> {
        validate_sync(q, info)?;
        if !info.features.sync && !info.features.post_sync {
            return Err(ProtocolError::UnsupportedFeature.into());
        }
        let bytes = self
            .request("/events/sync", "/events/sync", q, info.features.sync)
            .await?;
        let info = info.clone();
        let q = q.clone();
        let limits = self.limits();
        tokio::task::spawn_blocking(move || {
            let mut outer = limits;
            outer.max_depth = outer.max_depth.saturating_add(2);
            let value = parse_json(&bytes, outer)?;
            let (events, rejections) = verify_values(&value, &info, limits)?;
            let cursor = value
                .get("cursor")
                .and_then(Value::as_str)
                .ok_or(ProtocolError::InvalidRequest)?
                .to_owned();
            let has_more = value
                .get("has_more")
                .and_then(Value::as_bool)
                .ok_or(ProtocolError::InvalidRequest)?;
            if events.len() + rejections.len() > q.limit as usize
                || cursor.len() as u64 > info.limits.cursor_bytes
                || (has_more && q.cursor.as_deref() == Some(cursor.as_str()))
            {
                return Err(ProtocolError::InvalidRequest.into());
            }
            Ok(VerifiedSync {
                events,
                rejections,
                cursor,
                has_more,
            })
        })
        .await
        .map_err(|_| ClientError::Network)?
    }
}
