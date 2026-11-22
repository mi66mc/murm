use murm_protocol::{ParseLimits, parse_event};

fn input(header: &str, content: &str) -> String {
    format!(
        r#"{{"version":1,"id":"{}","signer":"{}","created_at":1,"kind":1,"header":{},"content":{},"signature":"{}"}}"#,
        "00".repeat(32),
        "11".repeat(32),
        header,
        content,
        "00".repeat(64)
    )
}
fn limits() -> ParseLimits {
    ParseLimits {
        max_bytes: 4096,
        max_depth: 8,
    }
}

#[test]
fn duplicate_nested_member_rejected() {
    for (h, c) in [
        (r#"{"a":{"x":1,"x":2}}"#, "null"),
        ("{}", r#"[{"x":1,"x":2}]"#),
        (r#"{"a":1,"\u0061":2}"#, "null"),
    ] {
        assert!(parse_event(input(h, c).as_bytes(), limits()).is_err());
    }
}
#[test]
fn unknown_outer_field_rejected() {
    let s = input("{}", "null").replacen("{", r#"{"extra":1,"#, 1);
    assert!(parse_event(s.as_bytes(), limits()).is_err());
}
#[test]
fn missing_field_rejected() {
    let s = input("{}", "null").replace(r#""content":null,"#, "");
    assert!(parse_event(s.as_bytes(), limits()).is_err());
}
#[test]
fn unknown_nested_field_preserved() {
    let e = parse_event(
        input(r#"{"extra":{"a":[true,null,4.5]}}"#, r#"{"unknown":"á"}"#).as_bytes(),
        limits(),
    )
    .unwrap();
    assert_eq!(e.header["extra"]["a"][2], 4.5);
    assert_eq!(e.content["unknown"], "á");
}
#[test]
fn invalid_unicode_rejected() {
    assert!(parse_event(input("{}", r#""\ud800""#).as_bytes(), limits()).is_err());
    assert!(parse_event(&[0xff, 0xfe], limits()).is_err());
}
#[test]
fn unsafe_integer_rejected() {
    for n in ["9007199254740992", "-9007199254740992", "1e100", "1e309"] {
        assert!(
            parse_event(input("{}", n).as_bytes(), limits()).is_err(),
            "{n}"
        );
    }
}
#[test]
fn byte_and_depth_limit_rejected() {
    let s = input("{}", "null");
    assert!(
        parse_event(
            s.as_bytes(),
            ParseLimits {
                max_bytes: 2,
                ..limits()
            }
        )
        .is_err()
    );
    assert!(
        parse_event(
            input("{}", "[[[0]]]").as_bytes(),
            ParseLimits {
                max_depth: 3,
                ..limits()
            }
        )
        .is_err()
    );
    assert!(
        parse_event(
            input("{}", r#""[[[[]]]]""#).as_bytes(),
            ParseLimits {
                max_depth: 2,
                ..limits()
            }
        )
        .is_ok()
    );
}
#[test]
fn safe_integer_field_types_and_lowercase_hex() {
    for (from, to) in [
        (r#""version":1"#, r#""version":2"#),
        (r#""kind":1"#, r#""kind":-1"#),
        (r#""created_at":1"#, r#""created_at":1.5"#),
    ] {
        assert!(parse_event(input("{}", "null").replace(from, to).as_bytes(), limits()).is_err());
    }
    assert!(
        parse_event(
            input("{}", "null")
                .replace(&"11".repeat(32), &"AA".repeat(32))
                .as_bytes(),
            limits()
        )
        .is_err()
    );
}
