mod common;
use common::*;
use murm_protocol::*;
use murm_relay::*;
use std::sync::Arc;
struct Fixed;
impl Clock for Fixed {
    fn now_unix(&self) -> u64 {
        100
    }
}
struct Server {
    url: String,
    task: tokio::task::JoinHandle<()>,
    _dir: tempfile::TempDir,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn start(mut c: RelayConfig) -> Server {
    let dir = tempfile::tempdir().unwrap();
    c.database_path = dir.path().join("r.db");
    let service = Arc::new(RelayService::open(c, Arc::new(Fixed)).unwrap());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        serve(listener, service, std::future::pending::<()>())
            .await
            .unwrap();
    });
    Server {
        url,
        task,
        _dir: dir,
    }
}
fn body() -> serde_json::Value {
    serde_json::json!({"filters":[{}],"limit":10})
}
#[tokio::test]
async fn single_event_post() {
    let s = start(config()).await;
    let client = reqwest::Client::new();
    let e = event("a", 1, false, 100);
    let a = client
        .post(format!("{}/events", s.url))
        .json(&e)
        .send()
        .await
        .unwrap();
    assert_eq!(a.status(), 201);
    assert!(
        a.headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap()
            .ends_with(&e.event().id.to_string())
    );
    assert_eq!(
        a.json::<StoreReceipt>().await.unwrap().retain_until,
        Some(110)
    );
    assert_eq!(
        client
            .post(format!("{}/events", s.url))
            .json(&e)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let fetched = client
        .get(format!("{}/events/{}", s.url, e.event().id))
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(
        parse_and_verify(&fetched, ParseLimits::default())
            .unwrap()
            .event()
            .id,
        e.event().id
    );
}
#[tokio::test]
async fn local_get_404() {
    let s = start(config()).await;
    let r = reqwest::get(format!("{}/events/{}", s.url, "00".repeat(32)))
        .await
        .unwrap();
    assert_eq!(r.status(), 404);
    assert_eq!(
        r.json::<Problem>().await.unwrap().code,
        ProblemCode::NotFound
    );
}
#[tokio::test]
async fn query_and_post_equivalent() {
    let s = start(config()).await;
    let c = reqwest::Client::new();
    c.post(format!("{}/events", s.url))
        .json(&event("a", 1, false, 100))
        .send()
        .await
        .unwrap();
    let a = c
        .request(
            reqwest::Method::from_bytes(b"QUERY").unwrap(),
            format!("{}/events", s.url),
        )
        .json(&body())
        .send()
        .await
        .unwrap();
    let b = c
        .post(format!("{}/events/query", s.url))
        .json(&body())
        .send()
        .await
        .unwrap();
    assert_eq!(a.status(), 200);
    assert_eq!(
        a.json::<serde_json::Value>().await.unwrap(),
        b.json::<serde_json::Value>().await.unwrap()
    );
}
#[tokio::test]
async fn query_sync_and_post_equivalent() {
    let s = start(config()).await;
    let c = reqwest::Client::new();
    let b = serde_json::json!({"filters":[{}],"limit":10,"cursor":null});
    let a = c
        .request(
            reqwest::Method::from_bytes(b"QUERY").unwrap(),
            format!("{}/events/sync", s.url),
        )
        .json(&b)
        .send()
        .await
        .unwrap();
    let d = c
        .post(format!("{}/events/sync", s.url))
        .json(&b)
        .send()
        .await
        .unwrap();
    assert_eq!(a.status(), 200);
    assert_eq!(
        a.json::<serde_json::Value>().await.unwrap(),
        d.json::<serde_json::Value>().await.unwrap()
    );
}
#[tokio::test]
async fn fixed_window_shared_between_aliases() {
    let mut cfg = config();
    cfg.info.rate_limits = vec![RateLimit {
        operation: Operation::Query,
        scope: RateScope::Ip,
        requests: 1,
        window_seconds: 60,
    }];
    let s = start(cfg).await;
    let c = reqwest::Client::new();
    assert_eq!(
        c.request(
            reqwest::Method::from_bytes(b"QUERY").unwrap(),
            format!("{}/events", s.url)
        )
        .json(&body())
        .send()
        .await
        .unwrap()
        .status(),
        200
    );
    let r = c
        .post(format!("{}/events/query", s.url))
        .json(&body())
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 429);
    assert_eq!(r.headers()["retry-after"], "20");
}
#[tokio::test]
async fn invalid_event_problem_details() {
    let s = start(config()).await;
    let c = reqwest::Client::new();
    let mut e = event("a", 1, false, 100).into_event();
    e.content = serde_json::json!("tampered");
    let r = c
        .post(format!("{}/events", s.url))
        .json(&e)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 422);
    assert_eq!(r.headers()["content-type"], "application/problem+json");
    assert_eq!(
        r.json::<Problem>().await.unwrap().code,
        ProblemCode::InvalidId
    );
    let r = c
        .post(format!("{}/events", s.url))
        .header("content-type", "application/json")
        .body(r#"{"a":1,"a":2}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(
        r.json::<Problem>().await.unwrap().code,
        ProblemCode::InvalidJson
    );
}
#[tokio::test]
async fn unsupported_feature_explicit() {
    let s = start(config()).await;
    for path in [
        "/relays".into(),
        format!("/events/{}?lookup=true", "00".repeat(32)),
    ] {
        let r = reqwest::get(format!("{}{path}", s.url)).await.unwrap();
        assert_eq!(r.status(), 422);
        assert_eq!(
            r.json::<Problem>().await.unwrap().code,
            ProblemCode::UnsupportedFeature
        );
    }
}
#[tokio::test]
async fn source_ip_not_untrusted_forwarded_header() {
    let mut cfg = config();
    cfg.info.rate_limits = vec![RateLimit {
        operation: Operation::Query,
        scope: RateScope::Ip,
        requests: 1,
        window_seconds: 60,
    }];
    let s = start(cfg).await;
    let c = reqwest::Client::new();
    for (ip, status) in [("192.0.2.1", 200), ("192.0.2.2", 429)] {
        assert_eq!(
            c.post(format!("{}/events/query", s.url))
                .header("x-forwarded-for", ip)
                .json(&body())
                .send()
                .await
                .unwrap()
                .status(),
            status
        );
    }
}
#[tokio::test]
async fn bounded_body_before_parse() {
    let mut cfg = config();
    cfg.info.limits.request_bytes = 2000;
    cfg.info.limits.event_bytes = 2000;
    cfg.info.limits.query_bytes = 2000;
    let s = start(cfg).await;
    let c = reqwest::Client::new();
    let r = c
        .post(format!("{}/events", s.url))
        .header("content-type", "application/json")
        .body("x".repeat(2001))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 413);
    assert_eq!(
        r.json::<Problem>().await.unwrap().code,
        ProblemCode::ContentTooLarge
    );
}
#[tokio::test]
async fn kind_byte_limit_counts_original_received_bytes() {
    let mut cfg = config();
    cfg.info.kind_policies.overrides[0].event_bytes = Some(1000);
    let s = start(cfg).await;
    let event = event("sized", 1, false, 100);
    let wire = format!(
        "{}{}",
        " ".repeat(1500),
        serde_json::to_string(&event).unwrap()
    );
    let response = reqwest::Client::new()
        .post(format!("{}/events", s.url))
        .header("content-type", "application/json")
        .body(wire)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 413);
}
