use murm_relay::{DbWorker, RelayConfig, RelayStore};
use std::time::{Duration, Instant};

#[test]
fn rejects_inconsistent_info_policy() {
    let mut c = RelayConfig::default();
    c.info.validated_kinds = vec![2];
    assert!(c.validate().is_err());
    let mut c = RelayConfig::default();
    c.info.kind_policies.default.retention_seconds = Some(90000000);
    assert!(c.validate().is_err());
}
#[test]
fn migrations_are_repeatable() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("relay.sqlite");
    let a = RelayStore::open(&p).unwrap();
    let epoch = a.epoch().unwrap();
    drop(a);
    let b = RelayStore::open(&p).unwrap();
    assert_eq!(epoch, b.epoch().unwrap());
    let c = rusqlite::Connection::open(&p).unwrap();
    assert_eq!(
        c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        c.query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "wal"
    );
}
#[test]
fn corrupt_database_not_recreated() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("broken.sqlite");
    std::fs::write(&p, b"not a database").unwrap();
    assert!(RelayStore::open(&p).is_err());
    assert_eq!(std::fs::read(p).unwrap(), b"not a database");
}
#[tokio::test(flavor = "current_thread")]
async fn sqlite_worker_does_not_block_runtime() {
    let dir = tempfile::tempdir().unwrap();
    let db = DbWorker::open(dir.path().join("relay.sqlite")).unwrap();
    let slow = tokio::spawn(async move {
        db.call(|_| {
            std::thread::sleep(Duration::from_millis(100));
            Ok(())
        })
        .await
        .unwrap()
    });
    let start = Instant::now();
    tokio::time::sleep(Duration::from_millis(5)).await;
    assert!(start.elapsed() < Duration::from_millis(80));
    slow.await.unwrap();
}
