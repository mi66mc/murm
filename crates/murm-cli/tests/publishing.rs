mod common;
use common::*;
use murm_cli::*;
use murm_protocol::*;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
struct Clock(AtomicU64);
impl ClientClock for Clock {
    fn now_unix(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}
async fn signer() -> (tempfile::TempDir, ClientService, ProfileId, Arc<Clock>) {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(Clock(AtomicU64::new(100)));
    let app = ClientService::open(dir.path())
        .unwrap()
        .with_clock(clock.clone());
    app.initialize_vault(
        SecretString::new("senha longa de testes".into()),
        app.begin_vault_action(),
    )
    .await
    .unwrap();
    let p = app
        .create_profile(
            "Autor",
            ProfileInput::ImportSecret {
                seed: SecretSeed::from_hex(&"01".repeat(32)).unwrap(),
                public_key: None,
            },
        )
        .await
        .unwrap();
    app.add_relay(p.id, "http://127.0.0.1:9").await.unwrap();
    (dir, app, p.id, clock)
}
#[tokio::test]
async fn read_only_cannot_publish() {
    let dir = tempfile::tempdir().unwrap();
    let app = ClientService::open(dir.path()).unwrap();
    let p = app
        .create_profile(
            "Leitor",
            ProfileInput::ImportPublic(derive_public_key(&[1; 32])),
        )
        .await
        .unwrap();
    let d = app.create_draft(p.id, "Title", "body").await.unwrap();
    assert!(matches!(
        app.publish(p.id, d.id).await,
        Err(ClientError::ReadOnly)
    ));
}
#[tokio::test]
async fn locked_cannot_sign() {
    let (_dir, app, p, _) = signer().await;
    let d = app.create_draft(p, "Title", "body").await.unwrap();
    app.lock();
    assert!(matches!(
        app.publish(p, d.id).await,
        Err(ClientError::Locked)
    ));
}
#[tokio::test]
async fn draft_survives_restart() {
    let (dir, app, p, _) = signer().await;
    let d = app
        .create_draft(p, "Title", "long **source**")
        .await
        .unwrap();
    drop(app);
    let app = ClientService::open(dir.path()).unwrap();
    let loaded = app.load_draft(p, d.id).await.unwrap();
    assert_eq!(loaded.content, serde_json::json!("long **source**"));
    assert_eq!(loaded.publication, d.publication);
}
#[tokio::test]
async fn publish_uses_selected_profile() {
    let (_dir, app, p, _) = signer().await;
    let other = app
        .create_profile("Outro", ProfileInput::Generate)
        .await
        .unwrap();
    let d = app.create_draft(p, "Title", "body").await.unwrap();
    app.select_profile(other.id).await.unwrap();
    let id = app.publish(p, d.id).await.unwrap();
    let e = app.cached_event(id).await.unwrap().unwrap();
    assert_eq!(e.event().signer, derive_public_key(&[1; 32]));
    assert!(app.publish(other.id, d.id).await.is_err());
}
#[tokio::test]
async fn duplicate_publish_reuses_queued_event() {
    let (_dir, app, p, clock) = signer().await;
    let d = app.create_draft(p, "Title", "body").await.unwrap();
    let first = app.publish(p, d.id).await.unwrap();
    clock.0.store(200, Ordering::SeqCst);
    assert_eq!(first, app.publish(p, d.id).await.unwrap());
    assert_eq!(app.outbox(p).await.unwrap().len(), 1);
}
#[tokio::test]
async fn timeout_retries_same_id() {
    let relay = server(vec![(500, String::new())]).await;
    let (_dir, app, p, clock) = signer().await;
    app.remove_relay(p, "http://127.0.0.1:9").await.unwrap();
    app.add_relay(p, &relay.url).await.unwrap();
    let d = app.create_draft(p, "Title", "body").await.unwrap();
    let id = app.publish(p, d.id).await.unwrap();
    app.retry_event(p, id).await.unwrap();
    relay.script.lock().unwrap().responses.push_back((
        201,
        serde_json::to_string(&StoreReceipt {
            id,
            status: StoreStatus::Stored,
            retain_until: Some(500),
        })
        .unwrap(),
    ));
    clock.0.store(200, Ordering::SeqCst);
    app.retry_event(p, id).await.unwrap();
    {
        let script = relay.script.lock().unwrap();
        assert_eq!(script.requests.len(), 2);
        assert_eq!(script.requests[0].1, script.requests[1].1);
    }
    assert_eq!(
        app.outbox(p).await.unwrap()[0].destinations[0].state,
        DestinationState::Stored
    );
}
#[tokio::test]
async fn editing_uses_max_known_revision() {
    let (_dir, app, p, _) = signer().await;
    let d = app.create_draft(p, "Title", "body").await.unwrap();
    app.publish(p, d.id).await.unwrap();
    let edit = app.edit_publication(p, &d.publication).await.unwrap();
    assert_eq!(edit.header["revision"], 2);
}
#[tokio::test]
async fn withdraw_and_restore() {
    let (_dir, app, p, _) = signer().await;
    let d = app.create_draft(p, "Title", "body").await.unwrap();
    app.publish(p, d.id).await.unwrap();
    let withdrawn = app.withdraw(p, &d.publication).await.unwrap();
    let e = app.cached_event(withdrawn).await.unwrap().unwrap();
    assert!(e.event().content.is_null());
    assert_eq!(e.event().header["revision"], 2);
    assert!(app.list_articles(false).await.unwrap().is_empty());
    let restored = app.restore(p, &d.publication, "new body").await.unwrap();
    let e = app.cached_event(restored).await.unwrap().unwrap();
    assert_eq!(e.event().header["revision"], 3);
    assert_eq!(e.event().content, "new body");
}
#[tokio::test]
async fn stale_revision_conflict_is_visible() {
    let (_dir, app, p, _) = signer().await;
    let d = app.create_draft(p, "Title", "body").await.unwrap();
    app.publish(p, d.id).await.unwrap();
    let stale = app.edit_publication(p, &d.publication).await.unwrap();
    app.withdraw(p, &d.publication).await.unwrap();
    assert!(matches!(
        app.publish(p, stale.id).await,
        Err(ClientError::Conflict)
    ));
}
#[tokio::test]
async fn switching_profile_keeps_outbox_owner() {
    let (_dir, app, p, _) = signer().await;
    let d = app.create_draft(p, "Title", "body").await.unwrap();
    let id = app.publish(p, d.id).await.unwrap();
    let other = app
        .create_profile("Outro", ProfileInput::Generate)
        .await
        .unwrap();
    app.select_profile(other.id).await.unwrap();
    assert!(app.outbox(other.id).await.unwrap().is_empty());
    assert_eq!(app.outbox(p).await.unwrap()[0].event_id, id);
}
#[tokio::test]
async fn removing_relay_cancels_pending_destinations() {
    let (_dir, app, p, _) = signer().await;
    let draft = app.create_draft(p, "Title", "body").await.unwrap();
    app.publish(p, draft.id).await.unwrap();
    app.remove_relay(p, "http://127.0.0.1:9").await.unwrap();
    assert_eq!(
        app.outbox(p).await.unwrap()[0].destinations[0].state,
        DestinationState::Refused
    );
}
#[tokio::test]
async fn retry_all_does_not_starve_older_pending_entries() {
    let relay = server(vec![]).await;
    let (_dir, app, p, clock) = signer().await;
    app.remove_relay(p, "http://127.0.0.1:9").await.unwrap();
    app.add_relay(p, &relay.url).await.unwrap();
    let draft = app.create_draft(p, "Old", "pending").await.unwrap();
    let old = app.publish(p, draft.id).await.unwrap();
    let mut db = rusqlite::Connection::open(app.database_path()).unwrap();
    let transaction = db.transaction().unwrap();
    for n in 0..1001 {
        let event = common::event(&format!("stored-{n}"), 200 + n);
        let id = event.event().id.to_string();
        transaction
            .execute(
                "INSERT INTO outbox(event_id,profile_id,json,created_at) VALUES(?1,?2,?3,?4)",
                rusqlite::params![
                    id,
                    p.to_string(),
                    serde_json::to_vec(&event).unwrap(),
                    200 + n
                ],
            )
            .unwrap();
        transaction
            .execute(
                "INSERT INTO outbox_destinations(event_id,relay,state) VALUES(?1,?2,'stored')",
                rusqlite::params![id, relay.url],
            )
            .unwrap();
    }
    transaction.commit().unwrap();
    relay.script.lock().unwrap().responses.push_back((
        201,
        serde_json::to_string(&StoreReceipt {
            id: old,
            status: StoreStatus::Stored,
            retain_until: Some(20000),
        })
        .unwrap(),
    ));
    clock.0.store(10000, Ordering::SeqCst);
    app.retry_outbox(p).await.unwrap();
    assert_eq!(relay.script.lock().unwrap().requests.len(), 1);
    let state: String = db
        .query_row(
            "SELECT state FROM outbox_destinations WHERE event_id=?1",
            [old.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(state, "stored");
}
