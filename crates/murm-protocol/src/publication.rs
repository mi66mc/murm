use crate::*;
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, collections::HashSet};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PublicationKey {
    pub signer: SignerKey,
    pub kind: u64,
    pub publication: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicationStatus {
    Published,
    Withdrawn,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicationMeta {
    pub key: PublicationKey,
    pub revision: u64,
    pub id: EventId,
    pub status: PublicationStatus,
}
impl PublicationMeta {
    pub fn next_revision(&self) -> Result<u64, ProtocolError> {
        if self.revision >= MAX_SAFE_INTEGER {
            Err(ProtocolError::InvalidPublication)
        } else {
            Ok(self.revision + 1)
        }
    }
}
pub fn validate_publication(event: &VerifiedEvent) -> Result<PublicationMeta, ProtocolError> {
    let e = event.event();
    let bad = ProtocolError::InvalidPublication;
    if e.kind != 1 {
        return Err(bad);
    }
    let h = &e.header;
    let id = h
        .get("publication")
        .and_then(|v| v.as_str())
        .ok_or(bad.clone())?;
    if id.is_empty()
        || id.len() > 128
        || !id.as_bytes()[0].is_ascii_alphanumeric()
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._~-".contains(&b))
    {
        return Err(bad);
    }
    let revision = h
        .get("revision")
        .and_then(safe_u64)
        .filter(|&v| v > 0 && v <= MAX_SAFE_INTEGER)
        .ok_or(bad.clone())?;
    let status = match h.get("status").and_then(|v| v.as_str()) {
        Some("published") if e.content.is_string() => PublicationStatus::Published,
        Some("withdrawn") if e.content.is_null() => PublicationStatus::Withdrawn,
        _ => return Err(bad),
    };
    for (k, min, max) in [("title", 0, 200), ("summary", 0, 1000), ("language", 1, 64)] {
        if let Some(v) = h.get(k) {
            let n = v.as_str().ok_or(bad.clone())?.chars().count();
            if n < min || n > max {
                return Err(bad);
            }
        }
    }
    if let Some(topics) = h.get("topics") {
        let topics = topics
            .as_array()
            .filter(|v| v.len() <= 20)
            .ok_or(bad.clone())?;
        let mut seen = HashSet::new();
        for v in topics {
            let s = v.as_str().ok_or(bad.clone())?;
            if !(1..=64).contains(&s.chars().count()) || !seen.insert(s) {
                return Err(bad);
            }
        }
    }
    Ok(PublicationMeta {
        key: PublicationKey {
            signer: e.signer,
            kind: e.kind,
            publication: id.into(),
        },
        revision,
        id: e.id,
        status,
    })
}
pub fn compare_revisions(
    a: &PublicationMeta,
    b: &PublicationMeta,
) -> Result<Ordering, ProtocolError> {
    if a.key != b.key {
        return Err(ProtocolError::InvalidPublication);
    }
    Ok((a.revision, a.id).cmp(&(b.revision, b.id)))
}
