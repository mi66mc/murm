use murm_protocol::*;
use serde_json::{json,Value};
use std::cmp::Ordering;

fn make(h:Value,c:Value, seed:u8)->VerifiedEvent {
    sign_event(&UnsignedEvent{version:1,signer:derive_public_key(&[seed;32]),created_at:10,kind:1,
        header:h.as_object().unwrap().clone(),content:c},&[seed;32]).unwrap()
}
fn header()->Value { json!({"publication":"test","revision":1,"status":"published"}) }
#[test]
fn identifier_bounds() {
    for id in ["","_abc","á","a/b","a b",&"a".repeat(129)] {
        let mut h=header();h["publication"]=json!(id);
        assert!(validate_publication(&make(h,json!("body"),1)).is_err(),"{id}");
    }
    for id in ["A","a._~-9",&"a".repeat(128)] {
        let mut h=header();h["publication"]=json!(id);validate_publication(&make(h,json!(""),1)).unwrap();
    }
}
#[test]
fn metadata_bounds() {
    for (key,valid,invalid) in [
        ("title",json!("😀".repeat(200)),json!("😀".repeat(201))),
        ("summary",json!("x".repeat(1000)),json!("x".repeat(1001))),
        ("language",json!("x".repeat(64)),json!("")),
        ("topics",json!(["a","A"]),json!(["a","a"]))] {
        let mut h=header();h[key]=valid;validate_publication(&make(h,json!(""),1)).unwrap();
        let mut h=header();h[key]=invalid;assert!(validate_publication(&make(h,json!(""),1)).is_err(),"{key}");
    }
    for topics in [json!([""]),json!(["x".repeat(65)]),json!(0),json!((0..21).map(|n|n.to_string()).collect::<Vec<_>>())] {
        let mut h=header();h["topics"]=topics;assert!(validate_publication(&make(h,json!(""),1)).is_err());
    }
}
#[test]
fn withdrawn_requires_null() {
    let mut h=header();h["status"]=json!("withdrawn");
    assert!(validate_publication(&make(h.clone(),json!(""),1)).is_err());
    validate_publication(&make(h,Value::Null,1)).unwrap();
}
#[test]
fn published_requires_string() {
    assert!(validate_publication(&make(header(),Value::Null,1)).is_err());
    assert!(validate_publication(&make(header(),json!({"text":"no"}),1)).is_err());
}
#[test]
fn missing_predecessor_allowed() {
    let mut h=header();h["revision"]=json!(50);
    assert_eq!(validate_publication(&make(h,json!(""),1)).unwrap().revision,50);
}
#[test]
fn greater_revision_then_greater_id() {
    let a=validate_publication(&make(header(),json!("a"),1)).unwrap();
    let b=validate_publication(&make(header(),json!("b"),1)).unwrap();
    assert_eq!(compare_revisions(&a,&b).unwrap(),a.id.cmp(&b.id));
    let mut h=header();h["revision"]=json!(2);
    assert_eq!(compare_revisions(&validate_publication(&make(h,json!(""),1)).unwrap(),&a).unwrap(),Ordering::Greater);
}
#[test]
fn another_signer_is_another_publication() {
    let a=validate_publication(&make(header(),json!(""),1)).unwrap();
    let b=validate_publication(&make(header(),json!(""),2)).unwrap();
    assert_ne!(a.key,b.key); assert!(compare_revisions(&a,&b).is_err());
}
#[test]
fn unknown_metadata_preserved() {
    let mut h=header();h["unknown"]=json!({"other":[1,2]});
    let e=make(h,json!(""),1);validate_publication(&e).unwrap();
    assert_eq!(e.event().header["unknown"]["other"],json!([1,2]));
}
#[test]
fn maximum_revision_does_not_wrap() {
    let mut h=header();h["revision"]=json!(MAX_SAFE_INTEGER);
    let m=validate_publication(&make(h,json!(""),1)).unwrap();
    assert!(m.next_revision().is_err());
    for rev in [json!(0),json!(-1),json!(1.2),json!("1")] {
        let mut h=header();h["revision"]=rev;
        assert!(validate_publication(&make(h,json!(""),1)).is_err());
    }
}
