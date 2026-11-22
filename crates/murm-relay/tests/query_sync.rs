mod common;
use common::*;
use murm_relay::*;
use murm_protocol::*;
use serde_json::json;
fn query()->EventQuery { EventQuery{filters:vec![Filter::default()],limit:1,cursor:None,view:None} }
fn sync()->SyncRequest {SyncRequest{filters:vec![Filter::default()],limit:10,cursor:None}}
#[test]
fn filters_match_reference_implementation() {
    let d=tempfile::tempdir().unwrap();let mut s=RelayStore::open(&d.path().join("r.db")).unwrap();let c=config();
    let es=vec![event("A",1,false,100),event("B",2,true,100),event("C",3,false,99)];
    for e in &es {s.admit(e,&c,100).unwrap();}
    for fs in [
        json!([{"kinds":[1],"since":100}]),
        json!([{"kinds":[1],"header":[{"path":"/revision","op":"eq","value":2}]}]),
        json!([{"kinds":[1],"header":[{"path":"/status","op":"eq","value":"published"}]}]),
        json!([{"kinds":[1],"header":[{"path":"/revision","op":"eq","value":"1"}]}]),
        json!([{"kinds":[1],"header":[{"path":"/language","op":"eq","value":null}]}]),
        json!([{"kinds":[1],"header":[{"path":"/topics","op":"contains","value":"rust"}]}]),
        json!([{"until":100},{"since":100}])] {
        let filters:Vec<Filter>=serde_json::from_value(fs).unwrap();
        let mut q=query();q.filters=filters.clone();q.limit=100;
        let got=s.query(&q,&c,100).unwrap().events.into_iter().map(|e|e.id).collect::<std::collections::HashSet<_>>();
        let want=es.iter().filter(|e|matches_filters(e,&filters)).map(|e|e.event().id).collect();
        assert_eq!(got,want);
    }
}
#[test]
fn raw_keyset_ties() {
    let d=tempfile::tempdir().unwrap();let mut s=RelayStore::open(&d.path().join("r.db")).unwrap();let c=config();
    let a=event("a",1,false,100);let b=event("b",1,false,100);
    for e in [&a,&b] {s.admit(e,&c,100).unwrap();}
    let mut q=query();let first=s.query(&q,&c,100).unwrap();
    assert_eq!(first.events[0].id,a.event().id.max(b.event().id));q.cursor=first.next_cursor;
    let last=s.query(&q,&c,100).unwrap();assert_eq!(last.events[0].id,a.event().id.min(b.event().id));assert!(last.next_cursor.is_none());
}
#[test]
fn current_selected_before_filters() {
    let d=tempfile::tempdir().unwrap();let mut s=RelayStore::open(&d.path().join("r.db")).unwrap();let c=config();
    s.admit(&event("a",1,false,100),&c,100).unwrap();let w=event("a",2,true,99);s.admit(&w,&c,100).unwrap();
    let mut q=query();q.view=Some(QueryView::Current);q.filters=serde_json::from_value(json!([{"kinds":[1],"header":[{"path":"/status","op":"eq","value":"published"}]}])).unwrap();
    assert!(s.query(&q,&c,100).unwrap().events.is_empty());
    s.evict(w.event().id,Some(RemovalReason::Policy),101).unwrap();
    assert!(s.query(&q,&c,101).unwrap().events.is_empty());
}
#[test]
fn current_snapshot_under_updates() {
    let d=tempfile::tempdir().unwrap();let mut s=RelayStore::open(&d.path().join("r.db")).unwrap();let c=config();
    let a=event("a",1,false,100);let b=event("b",1,false,99);
    s.admit(&a,&c,100).unwrap();s.admit(&b,&c,100).unwrap();
    let mut q=query();q.view=Some(QueryView::Current);q.filters=vec![serde_json::from_value(json!({"kinds":[1]})).unwrap()];
    let first=s.query(&q,&c,100).unwrap();assert_eq!(first.events[0].id,a.event().id);
    s.admit(&event("b",2,false,101),&c,101).unwrap();q.cursor=first.next_cursor;
    assert_eq!(s.query(&q,&c,101).unwrap().events[0].id,b.event().id);
}
#[test]
fn late_old_event_appears_in_sync() {
    let d=tempfile::tempdir().unwrap();let mut s=RelayStore::open(&d.path().join("r.db")).unwrap();let c=config();
    s.admit(&event("a",1,false,100),&c,100).unwrap();let mut q=sync();let first=s.sync(&q,&c,100).unwrap();
    let late=event("b",1,false,1);s.admit(&late,&c,101).unwrap();q.cursor=Some(first.cursor);
    assert_eq!(s.sync(&q,&c,101).unwrap().events[0].id,late.event().id);
}
#[test]
fn wrong_filter_cursor_rejected() {
    let d=tempfile::tempdir().unwrap();let mut s=RelayStore::open(&d.path().join("r.db")).unwrap();let c=config();
    let a=s.sync(&sync(),&c,100).unwrap();let mut q=sync();q.cursor=Some(a.cursor);q.filters=vec![serde_json::from_value(json!({"kinds":[1]})).unwrap()];
    assert_eq!(s.sync(&q,&c,100).unwrap_err().code(),ProblemCode::InvalidCursor);
    q.cursor=Some("forged".into());assert_eq!(s.sync(&q,&c,100).unwrap_err().code(),ProblemCode::InvalidCursor);
}
#[test]
fn cursor_expires_explicitly() {
    let d=tempfile::tempdir().unwrap();let mut s=RelayStore::open(&d.path().join("r.db")).unwrap();let mut c=config();
    c.info.limits.sync_cursor_seconds=1;let a=s.sync(&sync(),&c,100).unwrap();let mut q=sync();q.cursor=Some(a.cursor);
    assert_eq!(s.sync(&q,&c,101).unwrap_err().code(),ProblemCode::CursorExpired);
}
#[test]
fn sync_survives_restart() {
    let d=tempfile::tempdir().unwrap();let p=d.path().join("r.db");let c=config();
    let mut s=RelayStore::open(&p).unwrap();let a=s.sync(&sync(),&c,100).unwrap();drop(s);
    let mut s=RelayStore::open(&p).unwrap();let e=event("a",1,false,1);s.admit(&e,&c,101).unwrap();
    let mut q=sync();q.cursor=Some(a.cursor);q.limit=1;
    assert_eq!(s.sync(&q,&c,101).unwrap().events[0].id,e.event().id);
}
#[test]
fn response_byte_limit_does_not_skip() {
    let d=tempfile::tempdir().unwrap();let mut s=RelayStore::open(&d.path().join("r.db")).unwrap();let mut c=config();
    c.info.limits.response_bytes=2500;
    for n in 1..=3 {let e=event(&format!("a{n}"),1,false,100);let mut draft=e.event().unsigned();draft.content=json!("x".repeat(1000));
        s.admit(&sign_event(&draft,&[1;32]).unwrap(),&c,100).unwrap();}
    let mut q=sync();let mut ids=std::collections::HashSet::new();
    for _ in 0..5 {let r=s.sync(&q,&c,100).unwrap();assert!(serde_json::to_vec(&r).unwrap().len()<=2500);
        for e in &r.events {assert!(ids.insert(e.id));}q.cursor=Some(r.cursor);if !r.has_more {break;}}
    assert_eq!(ids.len(),3);
}
#[test]
fn expired_rows_do_not_loop() {
    let d=tempfile::tempdir().unwrap();let mut s=RelayStore::open(&d.path().join("r.db")).unwrap();let c=config();
    s.admit(&event("a",1,false,100),&c,100).unwrap();
    let first=s.sync(&sync(),&c,120).unwrap();assert!(first.events.is_empty());assert!(!first.has_more);
    let mut q=sync();q.cursor=Some(first.cursor);assert!(!s.sync(&q,&c,121).unwrap().has_more);
}
