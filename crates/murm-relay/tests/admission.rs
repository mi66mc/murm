mod common;
use common::*;
use murm_protocol::*;
use murm_relay::*;
#[test]
fn commit_before_receipt() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("r.db");
    let e = event("a", 1, false, 100);
    let mut s = RelayStore::open(&p).unwrap();
    let r = s.admit(&e, &config(), 100).unwrap();
    assert_eq!(r.status, StoreStatus::Stored);
    assert_eq!(sql(&p, "SELECT COUNT(*) FROM events"), 1);
    drop(s);
    assert!(
        RelayStore::open(&p)
            .unwrap()
            .fetch(e.event().id, 101)
            .unwrap()
            .is_some()
    );
}
#[test]
fn duplicate_does_not_renew() {
    let d = tempfile::tempdir().unwrap();
    let mut s = RelayStore::open(&d.path().join("r.db")).unwrap();
    let e = event("a", 1, false, 100);
    let a = s.admit(&e, &config(), 100).unwrap();
    let b = s.admit(&e, &config(), 105).unwrap();
    assert_eq!(b.status, StoreStatus::AlreadyStored);
    assert_eq!(a.retain_until, Some(110));
    assert_eq!(a.retain_until, b.retain_until);
}
#[test]
fn duplicate_has_no_new_sequence() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("r.db");
    let mut s = RelayStore::open(&p).unwrap();
    let e = event("a", 1, false, 100);
    s.admit(&e, &config(), 100).unwrap();
    s.admit(&e, &config(), 101).unwrap();
    assert_eq!(sql(&p, "SELECT MAX(admission_seq) FROM events"), 1);
}
#[test]
fn reaccept_evicted_gets_new_sequence() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("r.db");
    let mut s = RelayStore::open(&p).unwrap();
    let e = event("a", 1, false, 100);
    s.admit(&e, &config(), 100).unwrap();
    assert!(s.fetch(e.event().id, 120).unwrap().is_none());
    assert_eq!(
        s.admit(&e, &config(), 121).unwrap().status,
        StoreStatus::Stored
    );
    assert_eq!(sql(&p, "SELECT MAX(admission_seq) FROM events"), 2);
}
#[test]
fn future_timestamp_policy() {
    let d = tempfile::tempdir().unwrap();
    let mut s = RelayStore::open(&d.path().join("r.db")).unwrap();
    assert_eq!(
        s.admit(&event("a", 1, false, 1901), &config(), 100)
            .unwrap_err()
            .code(),
        ProblemCode::PolicyRejected
    );
    s.admit(&event("a", 1, false, 1900), &config(), 100)
        .unwrap();
}
#[test]
fn minimum_and_maximum_retention() {
    let d = tempfile::tempdir().unwrap();
    let mut s = RelayStore::open(&d.path().join("r.db")).unwrap();
    let e = event("a", 1, false, 100);
    s.admit(&e, &config(), 100).unwrap();
    assert!(s.evict(e.event().id, None, 109).is_err());
    assert!(s.fetch(e.event().id, 119).unwrap().is_some());
    assert!(s.fetch(e.event().id, 120).unwrap().is_none());
}
#[test]
fn removal_exception_is_explicit() {
    let d = tempfile::tempdir().unwrap();
    let mut s = RelayStore::open(&d.path().join("r.db")).unwrap();
    let e = event("a", 1, false, 100);
    let mut c = config();
    c.info.policies.early_removal = vec![RemovalReason::Legal];
    s.admit(&e, &c, 100).unwrap();
    assert!(
        s.evict(e.event().id, Some(RemovalReason::Policy), 101)
            .is_err()
    );
    s.evict(e.event().id, Some(RemovalReason::Legal), 101)
        .unwrap();
    assert!(s.fetch(e.event().id, 101).unwrap().is_none());
}
#[test]
fn head_survives_body_expiry() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("r.db");
    let mut s = RelayStore::open(&p).unwrap();
    let e = event("a", 2, true, 100);
    s.admit(&e, &config(), 100).unwrap();
    s.fetch(e.event().id, 120).unwrap();
    drop(s);
    let _s = RelayStore::open(&p).unwrap();
    assert_eq!(sql(&p, "SELECT revision FROM publication_heads"), 2);
}
#[test]
fn higher_revision_restores() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("r.db");
    let mut s = RelayStore::open(&p).unwrap();
    s.admit(&event("a", 2, true, 100), &config(), 100).unwrap();
    s.admit(&event("a", 1, false, 101), &config(), 101).unwrap();
    assert_eq!(sql(&p, "SELECT revision FROM publication_heads"), 2);
    s.admit(&event("a", 3, false, 90), &config(), 102).unwrap();
    assert_eq!(sql(&p, "SELECT revision FROM publication_heads"), 3);
}
#[test]
fn full_head_budget_refuses_new_work() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("r.db");
    let mut s = RelayStore::open(&p).unwrap();
    let mut c = config();
    c.info.limits.publication_states = 1;
    s.admit(&event("a", 1, false, 100), &c, 100).unwrap();
    assert_eq!(
        s.admit(&event("b", 1, false, 100), &c, 100)
            .unwrap_err()
            .code(),
        ProblemCode::LimitExceeded
    );
    s.admit(&event("a", 2, false, 100), &c, 100).unwrap();
    assert_eq!(sql(&p, "SELECT COUNT(*) FROM events"), 2);
}
#[test]
fn metadata_cannot_grow_past_total_database_budget() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("relay.db");
    let mut store = RelayStore::open(&path).unwrap();
    let mut c = config();
    c.max_storage_bytes = 131072;
    c.info.kind_policies.overrides[0].retention_seconds = Some(Some(0));
    c.info.kind_policies.overrides[0].max_retention_seconds = Some(Some(0));
    let mut refused = false;
    for revision in 1..=1000 {
        if let Err(error) = store.admit(&event("same-work", revision, false, 100), &c, 100) {
            assert_eq!(error.code(), ProblemCode::TemporarilyUnavailable);
            refused = true;
            break;
        }
    }
    assert!(
        refused,
        "expired bodies must not allow unbounded metadata growth"
    );
}
