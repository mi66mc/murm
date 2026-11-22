use murm_protocol::*;
use serde_json::{Value, json};
fn event() -> VerifiedEvent {
    sign_event(&UnsignedEvent{version:1,signer:derive_public_key(&[1;32]),kind:1,created_at:10,
    header:json!({"publication":"test","revision":1,"status":"published","topics":["rust"],"null":null}).as_object().unwrap().clone(),content:json!("a")},&[1;32]).unwrap()
}
fn filter(v: Value) -> Filter {
    serde_json::from_value(v).unwrap()
}
fn query(v: Value) -> EventQuery {
    serde_json::from_value(v).unwrap()
}
#[test]
fn and_inside_or_between() {
    let e = event();
    let fs = vec![
        filter(json!({"kinds":[2]})),
        filter(json!({"kinds":[1],"since":10,"until":11})),
    ];
    assert!(matches_filters(&e, &fs));
    assert!(!matches_filter(
        &e,
        &filter(json!({"kinds":[1],"since":11}))
    ));
}
#[test]
fn deduplicate_by_id() {
    let e = event();
    let fs = vec![filter(json!({})), filter(json!({"kinds":[1]}))];
    assert_eq!(filter_events([e.clone(), e], &fs).len(), 1);
}
#[test]
fn typed_equality() {
    let e = event();
    for (value, want) in [
        (json!(1), true),
        (json!(1.0), true),
        (json!("1"), false),
        (json!(true), false),
    ] {
        assert_eq!(
            matches_filter(
                &e,
                &filter(json!({"header":[{"path":"/revision","op":"eq","value":value}]}))
            ),
            want
        );
    }
}
#[test]
fn missing_vs_null() {
    let e = event();
    assert!(matches_filter(
        &e,
        &filter(json!({"header":[{"path":"/null","op":"eq","value":null}]}))
    ));
    assert!(!matches_filter(
        &e,
        &filter(json!({"header":[{"path":"/absent","op":"eq","value":null}]}))
    ));
}
#[test]
fn contains_is_array_membership() {
    let e = event();
    assert!(matches_filter(
        &e,
        &filter(json!({"header":[{"path":"/topics","op":"contains","value":"rust"}]}))
    ));
    assert!(!matches_filter(
        &e,
        &filter(json!({"header":[{"path":"/topics","op":"contains","value":"ru"}]}))
    ));
}
#[test]
fn since_inclusive_until_exclusive() {
    let e = event();
    assert!(matches_filter(&e, &filter(json!({"since":10,"until":11}))));
    assert!(!matches_filter(&e, &filter(json!({"until":10}))));
}
#[test]
fn unknown_request_member_rejected() {
    for v in [
        json!({"filters":[{}],"limit":1,"foo":1}),
        json!({"filters":[{"author":"x"}],"limit":1}),
        json!({"filters":[{"header":[{"path":"/status","op":"eq","value":"x","foo":1}]}],"limit":1}),
    ] {
        assert!(serde_json::from_value::<EventQuery>(v).is_err());
    }
}
#[test]
fn header_requires_explicit_kinds() {
    let q = query(
        json!({"filters":[{"header":[{"path":"/status","op":"eq","value":"published"}]}],"limit":1}),
    );
    assert_eq!(
        validate_query(&q, &RelayInfo::default()),
        Err(ProtocolError::UnsupportedFilter)
    );
}
#[test]
fn unsupported_path_rejected() {
    let q = query(
        json!({"filters":[{"kinds":[1],"header":[{"path":"/title","op":"eq","value":"x"}]}],"limit":1}),
    );
    assert_eq!(
        validate_query(&q, &RelayInfo::default()),
        Err(ProtocolError::UnsupportedFilter)
    );
}
#[test]
fn current_requires_supported_kinds() {
    for f in [json!({}), json!({"kinds":[2]})] {
        assert_eq!(
            validate_query(
                &query(json!({"filters":[f],"limit":1,"view":"current"})),
                &RelayInfo::default()
            ),
            Err(ProtocolError::UnsupportedFeature)
        );
    }
}
#[test]
fn complete_info_policies_and_relations() {
    let info = RelayInfo::default();
    let decoded: RelayInfo = serde_json::from_value(serde_json::to_value(&info).unwrap()).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded.effective_policy(1).retention_seconds, Some(2592000));
    assert_eq!(decoded.effective_policy(2).retention_seconds, Some(604800));
    let mut bad = info;
    bad.validated_kinds = vec![2];
    assert!(bad.validate().is_err());
}
#[test]
fn closed_shape_null_and_limits() {
    for f in [
        json!({"kinds":null}),
        json!({"header":null}),
        json!({"since":null}),
    ] {
        assert!(serde_json::from_value::<Filter>(f).is_err());
    }
    for q in [
        json!({"filters":[],"limit":1}),
        json!({"filters":[{"kinds":[]}],"limit":1}),
        json!({"filters":[{"since":10,"until":10}],"limit":1}),
        json!({"filters":[{}],"limit":0}),
    ] {
        assert!(validate_query(&query(q), &RelayInfo::default()).is_err());
    }
}
#[test]
fn problem_status_and_urn() {
    for (code, status) in [
        (ProblemCode::InvalidJson, 400),
        (ProblemCode::InvalidSignature, 422),
        (ProblemCode::RateLimited, 429),
        (ProblemCode::CursorExpired, 410),
        (ProblemCode::PolicyRejected, 403),
    ] {
        let p = Problem::new(code);
        assert_eq!(p.status, status);
        assert!(p.r#type.starts_with("urn:murm:problem:"));
    }
}
