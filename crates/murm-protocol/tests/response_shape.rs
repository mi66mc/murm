use murm_protocol::*;
use serde_json::json;
#[test]
fn nullable_response_members_are_still_required() {
    assert!(serde_json::from_value::<QueryResponse>(json!({"events":[]})).is_err());
    assert!(
        serde_json::from_value::<StoreReceipt>(json!({"id":"00".repeat(32),"status":"stored"}))
            .is_err()
    );
    assert!(serde_json::from_value::<KindPolicy>(json!({"event_bytes":1000})).is_err());
    assert!(
        serde_json::from_value::<KindPolicy>(
            json!({"event_bytes":1000,"retention_seconds":null,"max_retention_seconds":null})
        )
        .is_ok()
    );
}
