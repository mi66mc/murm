use crate::{Event, ProtocolError, MAX_SAFE_INTEGER};
use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use std::fmt;

#[derive(Debug, Clone, Copy)]
pub struct ParseLimits { pub max_bytes: usize, pub max_depth: usize }
impl Default for ParseLimits {
    fn default() -> Self { Self { max_bytes: 1_048_576, max_depth: 32 } }
}

struct Strict(Value);
impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct JsonVisitor;
        impl<'de> Visitor<'de> for JsonVisitor {
            type Value = Strict;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("Murm JSON") }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Strict,E> { Ok(Strict(v.into())) }
            fn visit_unit<E: de::Error>(self) -> Result<Strict,E> { Ok(Strict(Value::Null)) }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Strict,E> { Ok(Strict(v.into())) }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Strict,E> { Ok(Strict(v.into())) }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Strict,E> {
                if v.unsigned_abs() > MAX_SAFE_INTEGER { return Err(E::custom("unsafe integer")); }
                Ok(Strict(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Strict,E> {
                if v > MAX_SAFE_INTEGER { return Err(E::custom("unsafe integer")); }
                Ok(Strict(v.into()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Strict,E> {
                if !v.is_finite() || (v.fract() == 0.0 && v.abs() > MAX_SAFE_INTEGER as f64) {
                    return Err(E::custom("unsafe number"));
                }
                Ok(Strict(Value::Number(Number::from_f64(v).ok_or_else(|| E::custom("nonfinite"))?)))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a:A) -> Result<Strict,A::Error> {
                let mut values=Vec::new();
                while let Some(Strict(v))=a.next_element()? { values.push(v); }
                Ok(Strict(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a:A) -> Result<Strict,A::Error> {
                let mut values=Map::new();
                while let Some(k)=a.next_key::<String>()? {
                    if values.contains_key(&k) { return Err(de::Error::custom("duplicate member")); }
                    let Strict(v)=a.next_value()?;
                    values.insert(k,v);
                }
                Ok(Strict(Value::Object(values)))
            }
        }
        d.deserialize_any(JsonVisitor)
    }
}

/// Bounds nesting before recursive deserialization; quoted brackets do not count.
pub fn parse_json(bytes: &[u8], limits: ParseLimits) -> Result<Value, ProtocolError> {
    if bytes.len() > limits.max_bytes { return Err(ProtocolError::ContentTooLarge); }
    let (mut depth, mut quoted, mut escaped)=(0usize,false,false);
    for &b in bytes {
        if quoted {
            if escaped { escaped=false; } else if b==b'\\' { escaped=true; } else if b==b'"' { quoted=false; }
        } else {
            match b {
                b'"' => quoted=true,
                b'{' | b'[' => {
                    depth+=1;
                    if depth > limits.max_depth { return Err(ProtocolError::LimitExceeded); }
                }
                b'}' | b']' => depth=depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    let mut deserializer=serde_json::Deserializer::from_slice(bytes);
    let Strict(value)=Strict::deserialize(&mut deserializer).map_err(|_| ProtocolError::InvalidJson)?;
    deserializer.end().map_err(|_| ProtocolError::InvalidJson)?;
    Ok(value)
}

pub fn parse_event(bytes: &[u8], limits: ParseLimits) -> Result<Event, ProtocolError> {
    let value=parse_json(bytes, limits)?;
    // Value deserialization alone treats a missing nullable field as null.
    let object=value.as_object().ok_or(ProtocolError::InvalidEvent)?;
    if object.len()!=8 || !["version","id","signer","created_at","kind","header","content","signature"]
        .iter().all(|k| object.contains_key(*k)) { return Err(ProtocolError::InvalidEvent); }
    let event: Event=serde_json::from_value(value).map_err(|_| ProtocolError::InvalidEvent)?;
    if event.version!=1 { return Err(ProtocolError::UnsupportedVersion); }
    if event.created_at > MAX_SAFE_INTEGER || event.kind > MAX_SAFE_INTEGER { return Err(ProtocolError::InvalidEvent); }
    Ok(event)
}
