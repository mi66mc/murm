mod common;
use common::*;
use murm_protocol::*;
use murm_relay::*;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
struct TestClock(AtomicU64);
impl Clock for TestClock {
    fn now_unix(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}
struct Server {
    service: Arc<RelayService>,
    url: String,
    task: tokio::task::JoinHandle<()>,
    _dir: tempfile::TempDir,
    clock: Arc<TestClock>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn start(c: RelayConfig) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    start_on(c, listener).await
}
async fn start_on(mut c: RelayConfig, listener: tokio::net::TcpListener) -> Server {
    let dir = tempfile::tempdir().unwrap();
    c.database_path = dir.path().join("r.db");
    let clock = Arc::new(TestClock(AtomicU64::new(100)));
    let service = Arc::new(RelayService::open(c, clock.clone()).unwrap());
    let url = format!("http://{}", listener.local_addr().unwrap());
    let s = service.clone();
    let task = tokio::spawn(async move {
        serve(listener, s, std::future::pending::<()>())
            .await
            .unwrap();
    });
    Server {
        service,
        url,
        task,
        _dir: dir,
        clock,
    }
}
fn payload(e: &VerifiedEvent) -> Vec<u8> {
    serde_json::to_vec(e).unwrap()
}
#[tokio::test]
async fn two_relays_converge_without_loop() {
    let la = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let lb = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut ca = config();
    ca.peers = vec![format!("http://{}", lb.local_addr().unwrap())];
    let mut cb = config();
    cb.peers = vec![format!("http://{}", la.local_addr().unwrap())];
    let a = start_on(ca, la).await;
    let b = start_on(cb, lb).await;
    let e = event("a", 1, false, 100);
    a.service.submit(payload(&e)).await.unwrap();
    let ra = Replicator::new(a.service.clone()).unwrap();
    assert!(ra.propagate_once().await.unwrap());
    assert_eq!(
        b.service.fetch(e.event().id).await.unwrap().event().id,
        e.event().id
    );
    let rb = Replicator::new(b.service.clone()).unwrap();
    assert!(rb.propagate_once().await.unwrap());
    assert!(!rb.propagate_once().await.unwrap());
    assert!(!ra.propagate_once().await.unwrap());
    a.service.submit(payload(&e)).await.unwrap();
    assert!(!ra.propagate_once().await.unwrap());
}
#[tokio::test]
async fn retries_are_globally_bounded() {
    let mut cfg = config();
    cfg.peers = vec!["http://127.0.0.1:9".into(), "http://127.0.0.1:10".into()];
    cfg.info.propagation.max_attempts = 3;
    cfg.info.kind_policies.overrides[0].max_retention_seconds = Some(None);
    let a = start(cfg).await;
    a.service
        .submit(payload(&event("a", 1, false, 100)))
        .await
        .unwrap();
    let r = Replicator::new(a.service.clone()).unwrap();
    for i in 0..10 {
        a.clock.0.store(100 + i * 100, Ordering::SeqCst);
        r.propagate_once().await.unwrap();
    }
    assert_eq!(
        sql(
            &a.service.config.database_path,
            "SELECT SUM(attempts) FROM propagation_budgets"
        ),
        3
    );
}
#[tokio::test]
async fn duplicate_does_not_reopen_job() {
    let b = start(config()).await;
    let mut cfg = config();
    cfg.peers = vec![b.url.clone()];
    let a = start(cfg).await;
    let e = event("a", 1, false, 100);
    let r = Replicator::new(a.service.clone()).unwrap();
    a.service.submit(payload(&e)).await.unwrap();
    r.propagate_once().await.unwrap();
    a.service.submit(payload(&e)).await.unwrap();
    assert_eq!(
        sql(
            &a.service.config.database_path,
            "SELECT COUNT(*) FROM propagation_jobs"
        ),
        0
    );
}
#[tokio::test]
async fn queue_full_keeps_receipt() {
    let mut cfg = config();
    cfg.peers = vec!["http://127.0.0.1:9".into()];
    cfg.info.propagation.queue_events = 1;
    let a = start(cfg).await;
    for id in ["a", "b"] {
        assert_eq!(
            a.service
                .submit(payload(&event(id, 1, false, 100)))
                .await
                .unwrap()
                .status,
            StoreStatus::Stored
        );
    }
    assert_eq!(
        sql(
            &a.service.config.database_path,
            "SELECT COUNT(*) FROM propagation_jobs"
        ),
        1
    );
    assert_eq!(
        sql(
            &a.service.config.database_path,
            "SELECT COUNT(*) FROM events"
        ),
        2
    );
}
#[test]
fn restart_resumes_job() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("r.db");
    let mut c = config();
    c.peers = vec!["http://127.0.0.1:9".into()];
    let mut s = RelayStore::open(&p).unwrap();
    s.admit(&event("a", 1, false, 100), &c, 100).unwrap();
    drop(s);
    let _s = RelayStore::open(&p).unwrap();
    assert_eq!(sql(&p, "SELECT COUNT(*) FROM propagation_jobs"), 1);
}
#[tokio::test]
async fn retry_after_respected() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = axum::Router::new().route(
        "/events",
        axum::routing::post(|| async {
            (
                axum::http::StatusCode::TOO_MANY_REQUESTS,
                [("retry-after", "60")],
            )
        }),
    );
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let mut cfg = config();
    cfg.peers = vec![url];
    let a = start(cfg).await;
    a.service
        .submit(payload(&event("a", 1, false, 100)))
        .await
        .unwrap();
    let r = Replicator::new(a.service.clone()).unwrap();
    r.propagate_once().await.unwrap();
    assert_eq!(
        sql(
            &a.service.config.database_path,
            "SELECT next_try FROM propagation_jobs"
        ),
        160
    );
    a.clock.0.store(159, Ordering::SeqCst);
    assert!(!r.propagate_once().await.unwrap());
    task.abort();
}
#[tokio::test]
async fn peer_sync_restores_outage() {
    let b = start(config()).await;
    let mut cfg = config();
    cfg.peers = vec![b.url.clone()];
    let a = start(cfg).await;
    let e = event("old", 1, false, 1);
    b.service.submit(payload(&e)).await.unwrap();
    let r = Replicator::new(a.service.clone()).unwrap();
    r.sync_once(&b.url).await.unwrap();
    assert_eq!(
        a.service.fetch(e.event().id).await.unwrap().event().id,
        e.event().id
    );
    assert_eq!(
        sql(
            &a.service.config.database_path,
            "SELECT COUNT(*) FROM peer_sync"
        ),
        1
    );
}
#[test]
fn invalid_last_event_leaves_batch_uncommitted() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("r.db");
    let _s = RelayStore::open(&p).unwrap();
    let mut invalid = event("b", 1, false, 100).into_event();
    invalid.content = serde_json::json!("changed");
    let value = serde_json::json!({"events":[event("a",1,false,100).event(),&invalid],"cursor":"next","has_more":false});
    assert!(
        ValidatedSyncBatch::parse(
            &serde_json::to_vec(&value).unwrap(),
            &RelayInfo::default(),
            ParseLimits::default()
        )
        .is_err()
    );
    assert_eq!(sql(&p, "SELECT COUNT(*) FROM events"), 0);
}
#[test]
fn failed_batch_commit_keeps_cursor() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("r.db");
    let mut s = RelayStore::open(&p).unwrap();
    rusqlite::Connection::open(&p).unwrap().execute_batch("CREATE TRIGGER refuse_cursor BEFORE INSERT ON peer_sync BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    let value = serde_json::json!({"events":[event("a",1,false,100).event()],"cursor":"next","has_more":false});
    let batch = ValidatedSyncBatch::parse(
        &serde_json::to_vec(&value).unwrap(),
        &RelayInfo::default(),
        ParseLimits::default(),
    )
    .unwrap();
    let filters = vec![Filter::default()];
    assert!(
        s.import_sync_batch("peer", &filters, batch, &config(), 100)
            .is_err()
    );
    assert_eq!(sql(&p, "SELECT COUNT(*) FROM events"), 0);
    assert_eq!(sql(&p, "SELECT COUNT(*) FROM publication_heads"), 0);
    assert_eq!(sql(&p, "SELECT COUNT(*) FROM peer_sync"), 0);
}
#[test]
fn sync_wrapper_does_not_reduce_event_depth_budget() {
    let mut draft = event("deep", 1, false, 100).event().unsigned();
    draft
        .header
        .insert("extra".into(), serde_json::json!({"nested":{"value":true}}));
    let event = sign_event(&draft, &[1; 32]).unwrap();
    let wire =
        serde_json::to_vec(&serde_json::json!({"events":[event],"cursor":"next","has_more":false}))
            .unwrap();
    assert!(
        ValidatedSyncBatch::parse(
            &wire,
            &RelayInfo::default(),
            ParseLimits {
                max_bytes: 10000,
                max_depth: 4
            }
        )
        .is_ok()
    );
}
#[test]
fn seen_lifetime_matches_declared_policy() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("r.db");
    let mut store = RelayStore::open(&p).unwrap();
    let mut c = config();
    c.info.propagation.seen_seconds = 5;
    store.admit(&event("a", 1, false, 100), &c, 100).unwrap();
    assert_eq!(sql(&p, "SELECT expires_at FROM seen_ids"), 105);
}
#[test]
fn active_budget_suppresses_post_eviction_wave() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("r.db");
    let mut store = RelayStore::open(&p).unwrap();
    let mut c = config();
    c.peers = vec!["http://127.0.0.1:9".into()];
    c.info.propagation.seen_seconds = 0;
    let event = event("a", 1, false, 100);
    store.admit(&event, &c, 100).unwrap();
    store
        .evict(event.event().id, Some(RemovalReason::Policy), 101)
        .unwrap();
    store.admit(&event, &c, 102).unwrap();
    assert_eq!(sql(&p, "SELECT COUNT(*) FROM propagation_jobs"), 0);
}
