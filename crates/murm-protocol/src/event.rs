use crate::ProtocolError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use std::{fmt, str::FromStr};

pub const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

macro_rules! hex_type {
    ($name:ident, $len:expr) => {
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub [u8; $len]);
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&hex::encode(self.0))
            }
        }
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { fmt::Display::fmt(self, f) }
        }
        impl FromStr for $name {
            type Err = ProtocolError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                if s.len() != $len * 2 || !s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
                    return Err(ProtocolError::InvalidEvent);
                }
                let mut bytes = [0; $len];
                hex::decode_to_slice(s, &mut bytes).map_err(|_| ProtocolError::InvalidEvent)?;
                Ok(Self(bytes))
            }
        }
        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> { s.serialize_str(&self.to_string()) }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                String::deserialize(d)?.parse().map_err(serde::de::Error::custom)
            }
        }
    }
}
hex_type!(EventId, 32);
hex_type!(SignerKey, 32);
hex_type!(SignatureBytes, 64);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub version: u64,
    pub id: EventId,
    pub signer: SignerKey,
    pub created_at: u64,
    pub kind: u64,
    pub header: Map<String, Value>,
    pub content: Value,
    pub signature: SignatureBytes,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct UnsignedEvent {
    pub version: u64,
    pub signer: SignerKey,
    pub created_at: u64,
    pub kind: u64,
    pub header: Map<String, Value>,
    pub content: Value,
}
impl Event {
    pub fn unsigned(&self) -> UnsignedEvent {
        UnsignedEvent { version: self.version, signer: self.signer, created_at: self.created_at,
            kind: self.kind, header: self.header.clone(), content: self.content.clone() }
    }
}
/// Construction is restricted to successful envelope/hash/signature verification.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct VerifiedEvent(pub(crate) Event);
impl VerifiedEvent {
    pub fn event(&self) -> &Event { &self.0 }
    pub fn into_event(self) -> Event { self.0 }
}
