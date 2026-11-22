#![allow(dead_code)]
use murm_protocol::*;
use serde_json::json;
pub fn event(publication:&str,revision:u64,withdrawn:bool,time:u64)->VerifiedEvent {
    sign_event(&UnsignedEvent{version:1,signer:derive_public_key(&[1;32]),created_at:time,kind:1,
        header:json!({"publication":publication,"revision":revision,"status":if withdrawn {"withdrawn"}else{"published"}}).as_object().unwrap().clone(),
        content:if withdrawn {json!(null)}else{json!(format!("article {publication} revision {revision}"))}},&[1;32]).unwrap()
}
pub fn config()->murm_relay::RelayConfig {
    let mut c=murm_relay::RelayConfig::default();
    c.info.kind_policies.overrides[0].retention_seconds=Some(Some(10));
    c.info.kind_policies.overrides[0].max_retention_seconds=Some(Some(20));c
}
pub fn sql(path:&std::path::Path,query:&str)->i64 {
    rusqlite::Connection::open(path).unwrap().query_row(query,[],|r|r.get(0)).unwrap()
}
