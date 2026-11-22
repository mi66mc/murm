mod common;
use common::*;
use murm_cli::*;
use murm_protocol::*;
fn count(app: &ClientService, table: &str) -> i64 {
    rusqlite::Connection::open(app.database_path())
        .unwrap()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}
async fn client() -> (tempfile::TempDir, ClientService, ProfileId) {
    let d = tempfile::tempdir().unwrap();
    let app = ClientService::open(d.path()).unwrap();
    let p = app
        .create_profile(
            "Leitor",
            ProfileInput::ImportPublic(derive_public_key(&[7; 32])),
        )
        .await
        .unwrap();
    (d, app, p.id)
}
#[tokio::test]
async fn verify_before_cache() {
    let (_d, app, p) = client().await;
    let mut bad = event("bad", 1).into_event();
    bad.content = serde_json::json!("tampered");
    let relay = server(vec![(200, batch(vec![bad], "next"))]).await;
    app.add_relay(p, &relay.url).await.unwrap();
    assert!(app.sync_profile(p, &relay.url).await.is_err());
    assert_eq!(count(&app, "verified_events"), 0);
    assert_eq!(count(&app, "sync_cursors"), 0);
}
#[tokio::test]
async fn sync_cursor_commits_with_events() {
    let (_d, app, p) = client().await;
    let e = event("a", 1).into_event();
    let relay = server(vec![
        (200, batch(vec![e.clone()], "next")),
        (200, batch(vec![e], "next")),
    ])
    .await;
    app.add_relay(p, &relay.url).await.unwrap();
    let db = rusqlite::Connection::open(app.database_path()).unwrap();
    db.execute_batch("CREATE TRIGGER no_cursor BEFORE INSERT ON sync_cursors BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert!(app.sync_profile(p, &relay.url).await.is_err());
    assert_eq!(count(&app, "verified_events"), 0);
    assert_eq!(count(&app, "publication_heads"), 0);
    db.execute_batch("DROP TRIGGER no_cursor;").unwrap();
    app.sync_profile(p, &relay.url).await.unwrap();
    assert_eq!(count(&app, "verified_events"), 1);
    assert_eq!(count(&app, "sync_cursors"), 1);
}
#[tokio::test]
async fn invalid_batch_leaves_cursor_unchanged() {
    let (_d, app, p) = client().await;
    let mut bad = event("bad", 1).into_event();
    bad.content = serde_json::json!("tampered");
    let relay = server(vec![
        (200, batch(vec![event("a", 100).into_event()], "first")),
        (200, batch(vec![event("b", 2).into_event(), bad], "second")),
    ])
    .await;
    app.add_relay(p, &relay.url).await.unwrap();
    app.sync_profile(p, &relay.url).await.unwrap();
    assert!(app.sync_profile(p, &relay.url).await.is_err());
    let cursor: String = rusqlite::Connection::open(app.database_path())
        .unwrap()
        .query_row("SELECT cursor FROM sync_cursors", [], |r| r.get(0))
        .unwrap();
    assert_eq!(cursor, "first");
    assert_eq!(count(&app, "verified_events"), 1);
}
#[tokio::test]
async fn replay_deduplicates() {
    let (_d, app, p) = client().await;
    let e = event("a", 1).into_event();
    let b = batch(vec![e], "same");
    let relay = server(vec![(200, b.clone()), (200, b)]).await;
    app.add_relay(p, &relay.url).await.unwrap();
    app.sync_profile(p, &relay.url).await.unwrap();
    app.sync_profile(p, &relay.url).await.unwrap();
    assert_eq!(count(&app, "verified_events"), 1);
}
#[tokio::test]
async fn late_event_is_visible() {
    let (_d, app, p) = client().await;
    let late = event("late", 1);
    let relay = server(vec![
        (200, batch(vec![event("a", 100).into_event()], "first")),
        (200, batch(vec![late.clone().into_event()], "second")),
    ])
    .await;
    app.add_relay(p, &relay.url).await.unwrap();
    app.sync_profile(p, &relay.url).await.unwrap();
    app.sync_profile(p, &relay.url).await.unwrap();
    assert!(app.cached_event(late.event().id).await.unwrap().is_some());
}
#[tokio::test]
async fn expired_cursor_rescans() {
    let (_d, app, p) = client().await;
    let e = event("a", 1).into_event();
    let relay = server(vec![
        (200, batch(vec![e.clone()], "old")),
        (
            410,
            serde_json::to_string(&Problem::new(ProblemCode::CursorExpired)).unwrap(),
        ),
        (200, batch(vec![e, event("b", 2).into_event()], "new")),
    ])
    .await;
    app.add_relay(p, &relay.url).await.unwrap();
    app.sync_profile(p, &relay.url).await.unwrap();
    app.sync_profile(p, &relay.url).await.unwrap();
    assert_eq!(count(&app, "verified_events"), 2);
    assert!(relay.script.lock().unwrap().requests[2].1["cursor"].is_null());
}
#[tokio::test]
async fn retry_after_respected() {
    let (_d, app, p) = client().await;
    let relay = server(vec![(
        429,
        serde_json::to_string(&Problem::new(ProblemCode::RateLimited)).unwrap(),
    )])
    .await;
    app.add_relay(p, &relay.url).await.unwrap();
    assert!(matches!(
        app.sync_profile(p, &relay.url).await,
        Err(ClientError::RetryAfter(_))
    ));
    assert!(matches!(
        app.sync_profile(p, &relay.url).await,
        Err(ClientError::RetryAfter(_))
    ));
    assert_eq!(relay.script.lock().unwrap().requests.len(), 1);
}
#[tokio::test]
async fn http_alias_fallback_only_for_method_failure() {
    let response = serde_json::json!({"events":[],"next_cursor":null}).to_string();
    let relay = server(vec![(405, String::new()), (200, response)]).await;
    let transport = RelayTransport::new(&relay.url).unwrap();
    let q = EventQuery {
        filters: vec![Filter::default()],
        limit: 1,
        cursor: None,
        view: None,
    };
    transport.query(&q, &RelayInfo::default()).await.unwrap();
    assert_eq!(
        relay
            .script
            .lock()
            .unwrap()
            .requests
            .iter()
            .map(|v| v.0.as_str())
            .collect::<Vec<_>>(),
        vec!["QUERY", "POST"]
    );
    let relay = server(vec![(
        422,
        serde_json::to_string(&Problem::new(ProblemCode::UnsupportedFilter)).unwrap(),
    )])
    .await;
    assert!(
        RelayTransport::new(&relay.url)
            .unwrap()
            .query(&q, &RelayInfo::default())
            .await
            .is_err()
    );
    assert_eq!(relay.script.lock().unwrap().requests.len(), 1);
}
#[tokio::test]
async fn network_timeout_is_bounded() {
    let relay = server(vec![(200, batch(vec![], "next"))]).await;
    relay.script.lock().unwrap().delay = std::time::Duration::from_secs(2);
    let options = NetworkOptions {
        request_timeout: std::time::Duration::from_millis(50),
        ..Default::default()
    };
    let transport = RelayTransport::with_options(&relay.url, options).unwrap();
    let start = std::time::Instant::now();
    assert!(
        transport
            .sync(
                &SyncRequest {
                    filters: vec![Filter::default()],
                    cursor: None,
                    limit: 1
                },
                &RelayInfo::default()
            )
            .await
            .is_err()
    );
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
}
