use curve25519_dalek::{
    constants::{ED25519_BASEPOINT_POINT, EIGHT_TORSION},
    scalar::Scalar,
};
use ed25519_dalek::{Signer, SigningKey};
use murm_protocol::*;
use serde_json::json;
use sha2::{Digest, Sha512};

fn seed() -> [u8; 32] {
    hex::decode("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60")
        .unwrap()
        .try_into()
        .unwrap()
}
fn draft() -> UnsignedEvent {
    UnsignedEvent {
        version: 1,
        signer: "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"
            .parse()
            .unwrap(),
        created_at: 1710000000,
        kind: 1,
        header: json!({"publication":"hello-murm","revision":1,"status":"published"})
            .as_object()
            .unwrap()
            .clone(),
        content: json!("Hello Murm."),
    }
}
fn verify(e: &Event) -> Result<VerifiedEvent, ProtocolError> {
    parse_and_verify(&serde_json::to_vec(e).unwrap(), ParseLimits::default())
}

#[test]
fn mip02_known_event() {
    let d = draft();
    assert_eq!(derive_public_key(&seed()), d.signer);
    assert_eq!(
        String::from_utf8(canonical_unsigned(&d).unwrap()).unwrap(),
        r#"{"content":"Hello Murm.","created_at":1710000000,"header":{"publication":"hello-murm","revision":1,"status":"published"},"kind":1,"signer":"d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a","version":1}"#
    );
    let signed = sign_event(&d, &seed()).unwrap();
    assert_eq!(
        signed.event().id.to_string(),
        "945708370d265a3825922c890b2ac7f09fb0d5a68fa5e12e9b768ec0e3790791"
    );
    assert_eq!(
        signed.event().signature.to_string(),
        "8ab72b5776fbd22b5530c93e7262012b94a73b37c4e9f097d9bf4f8d0d738cb3f517c8b3ebb4e8c3ffb894769b5448a532b1674e2357caa0c051f7c725b4fc01"
    );
    verify(signed.event()).unwrap();
}
#[test]
fn raw_digest_not_hex() {
    let mut e = sign_event(&draft(), &seed()).unwrap().into_event();
    e.signature = SignatureBytes(
        SigningKey::from_bytes(&seed())
            .sign(e.id.to_string().as_bytes())
            .to_bytes(),
    );
    assert_eq!(verify(&e).unwrap_err(), ProtocolError::InvalidSignature);
}
#[test]
fn each_signed_field_changes_id() {
    let original = draft();
    let id = event_id(&original).unwrap();
    let mut variants = Vec::new();
    let mut d = original.clone();
    d.created_at += 1;
    variants.push(d);
    let mut d = original.clone();
    d.kind = 2;
    variants.push(d);
    let mut d = original.clone();
    d.signer = derive_public_key(&[7; 32]);
    variants.push(d);
    let mut d = original.clone();
    d.content = json!("Other");
    variants.push(d);
    let mut d = original.clone();
    d.header.insert("unknown".into(), json!(1));
    variants.push(d);
    for d in variants {
        assert_ne!(event_id(&d).unwrap(), id);
    }
    let mut d = original;
    d.version = 2;
    assert!(event_id(&d).is_err());
}
#[test]
fn formatting_and_object_order_preserve_id() {
    let e = sign_event(&draft(), &seed()).unwrap();
    let pretty = serde_json::to_vec_pretty(&e).unwrap();
    assert_eq!(
        parse_and_verify(&pretty, ParseLimits::default())
            .unwrap()
            .event()
            .id,
        e.event().id
    );
}
#[test]
fn array_order_changes_id() {
    let mut d = draft();
    d.content = json!([1, 2]);
    let a = event_id(&d).unwrap();
    d.content = json!([2, 1]);
    assert_ne!(a, event_id(&d).unwrap());
}
#[test]
fn unicode_not_normalized() {
    let mut d = draft();
    d.content = json!("é");
    let a = event_id(&d).unwrap();
    d.content = json!("e\u{301}");
    assert_ne!(a, event_id(&d).unwrap());
}
#[test]
fn negative_zero_uses_jcs_zero() {
    let mut d = draft();
    d.content = json!(-0.0);
    let a = canonical_unsigned(&d).unwrap();
    d.content = json!(0);
    assert_eq!(a, canonical_unsigned(&d).unwrap());
}
#[test]
fn jcs_sorts_utf16_not_utf8() {
    let mut d = draft();
    d.content = json!({"\u{e000}":1,"\u{1f600}":2});
    assert!(
        String::from_utf8(canonical_unsigned(&d).unwrap())
            .unwrap()
            .starts_with(r#"{"content":{"😀":2,"":1}"#)
    );
}
#[test]
fn identity_public_key_rejected() {
    let mut d = draft();
    d.signer = SignerKey({
        let mut a = [0; 32];
        a[0] = 1;
        a
    });
    let e = Event {
        version: 1,
        id: event_id(&d).unwrap(),
        signer: d.signer,
        created_at: d.created_at,
        kind: 1,
        header: d.header,
        content: d.content,
        signature: SignatureBytes([0; 64]),
    };
    assert_eq!(verify(&e).unwrap_err(), ProtocolError::InvalidSignature);
}
#[test]
fn noncanonical_point_rejected() {
    let mut e = sign_event(&draft(), &seed()).unwrap().into_event();
    let mut y = [255; 32];
    y[0] = 0xed;
    y[31] = 0x7f;
    e.signature.0[..32].copy_from_slice(&y);
    assert_eq!(verify(&e).unwrap_err(), ProtocolError::InvalidSignature);
}
#[test]
fn torsion_point_rejected() {
    let mut e = sign_event(&draft(), &seed()).unwrap().into_event();
    e.signature.0[..32].copy_from_slice(&EIGHT_TORSION[1].compress().to_bytes());
    assert_eq!(verify(&e).unwrap_err(), ProtocolError::InvalidSignature);
}
#[test]
fn scalar_out_of_range_rejected() {
    let mut e = sign_event(&draft(), &seed()).unwrap().into_event();
    e.signature.0[32..].fill(255);
    assert_eq!(verify(&e).unwrap_err(), ProtocolError::InvalidSignature);
}
#[test]
fn valid_identity_r_is_accepted() {
    // Algebraic fixture only: R=0*B, S=H(R,A,m)*a. Never use this nonce in a signer.
    let a = Scalar::from(123456u64);
    let mut d = draft();
    d.signer = SignerKey((ED25519_BASEPOINT_POINT * a).compress().to_bytes());
    let id = event_id(&d).unwrap();
    let mut r = [0; 32];
    r[0] = 1;
    let mut h = Sha512::new();
    h.update(r);
    h.update(d.signer.0);
    h.update(id.0);
    let k = Scalar::from_bytes_mod_order_wide(&h.finalize().into());
    let mut sig = [0; 64];
    sig[..32].copy_from_slice(&r);
    sig[32..].copy_from_slice(&(k * a).to_bytes());
    let e = Event {
        version: 1,
        id,
        signer: d.signer,
        created_at: d.created_at,
        kind: 1,
        header: d.header,
        content: d.content,
        signature: SignatureBytes(sig),
    };
    verify(&e).unwrap();
}
