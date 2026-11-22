use crate::*;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::HashSet;

fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HeaderOp {
    Eq,
    Contains,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeaderCondition {
    pub path: String,
    pub op: HeaderOp,
    pub value: Value,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub kinds: Option<Vec<u64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub signers: Option<Vec<SignerKey>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub since: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub until: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub header: Option<Vec<HeaderCondition>>,
}
pub type FilterSet = Vec<Filter>;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryView {
    Current,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventQuery {
    pub filters: FilterSet,
    pub limit: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub view: Option<QueryView>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncRequest {
    pub filters: FilterSet,
    pub cursor: Option<String>,
    pub limit: u64,
}

pub fn validate_filters(filters: &[Filter], info: &RelayInfo) -> Result<(), ProtocolError> {
    if filters.is_empty() {
        return Err(ProtocolError::InvalidRequest);
    }
    if filters.len() as u64 > info.limits.filters {
        return Err(ProtocolError::LimitExceeded);
    }
    for f in filters {
        for size in [
            f.kinds.as_ref().map(|v| v.len()),
            f.signers.as_ref().map(|v| v.len()),
        ]
        .into_iter()
        .flatten()
        {
            if size == 0 {
                return Err(ProtocolError::InvalidRequest);
            }
            if size as u64 > info.limits.filter_values {
                return Err(ProtocolError::LimitExceeded);
            }
        }
        if f.kinds
            .as_ref()
            .is_some_and(|v| v.iter().any(|&n| n > MAX_SAFE_INTEGER))
            || f.since.is_some_and(|v| v > MAX_SAFE_INTEGER)
            || f.until.is_some_and(|v| v > MAX_SAFE_INTEGER)
            || matches!((f.since,f.until),(Some(a),Some(b)) if a>=b)
        {
            return Err(ProtocolError::InvalidRequest);
        }
        if let Some(conditions) = &f.header {
            if conditions.is_empty() {
                return Err(ProtocolError::InvalidRequest);
            }
            if conditions.len() as u64 > info.limits.header_conditions {
                return Err(ProtocolError::LimitExceeded);
            }
            let kinds = f.kinds.as_ref().ok_or(ProtocolError::UnsupportedFilter)?;
            for c in conditions {
                for kind in kinds {
                    if !info.header_filters.iter().any(|cap| {
                        cap.kind == *kind && cap.path == c.path && cap.ops.contains(&c.op)
                    }) {
                        return Err(ProtocolError::UnsupportedFilter);
                    }
                }
            }
        }
    }
    Ok(())
}
fn validate_limit_cursor(
    limit: u64,
    cursor: Option<&str>,
    info: &RelayInfo,
) -> Result<(), ProtocolError> {
    if limit == 0 {
        return Err(ProtocolError::InvalidRequest);
    }
    if limit > info.limits.query_results
        || cursor.is_some_and(|s| s.len() as u64 > info.limits.cursor_bytes)
    {
        return Err(ProtocolError::LimitExceeded);
    }
    Ok(())
}
pub fn validate_query(q: &EventQuery, info: &RelayInfo) -> Result<(), ProtocolError> {
    validate_filters(&q.filters, info)?;
    validate_limit_cursor(q.limit, q.cursor.as_deref(), info)?;
    if q.view.is_some() {
        for f in &q.filters {
            if f.kinds
                .as_ref()
                .is_none_or(|v| v.iter().any(|k| !info.features.current_kinds.contains(k)))
            {
                return Err(ProtocolError::UnsupportedFeature);
            }
        }
    }
    Ok(())
}
pub fn validate_sync(q: &SyncRequest, info: &RelayInfo) -> Result<(), ProtocolError> {
    validate_filters(&q.filters, info)?;
    validate_limit_cursor(q.limit, q.cursor.as_deref(), info)
}
fn equals(a: &Value, b: &Value) -> bool {
    match (serde_jcs::to_vec(a), serde_jcs::to_vec(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}
pub fn matches_filter(event: &VerifiedEvent, f: &Filter) -> bool {
    let e = event.event();
    if f.kinds.as_ref().is_some_and(|v| !v.contains(&e.kind))
        || f.signers.as_ref().is_some_and(|v| !v.contains(&e.signer))
        || f.since.is_some_and(|v| e.created_at < v)
        || f.until.is_some_and(|v| e.created_at >= v)
    {
        return false;
    }
    let header = Value::Object(e.header.clone());
    f.header.as_ref().is_none_or(|cs| {
        cs.iter().all(|c| {
            header.pointer(&c.path).is_some_and(|v| match c.op {
                HeaderOp::Eq => equals(v, &c.value),
                HeaderOp::Contains => v
                    .as_array()
                    .is_some_and(|vs| vs.iter().any(|v| equals(v, &c.value))),
            })
        })
    })
}
pub fn matches_filters(event: &VerifiedEvent, filters: &[Filter]) -> bool {
    filters.iter().any(|f| matches_filter(event, f))
}
pub fn filter_events(
    events: impl IntoIterator<Item = VerifiedEvent>,
    filters: &[Filter],
) -> Vec<VerifiedEvent> {
    let mut seen = HashSet::new();
    events
        .into_iter()
        .filter(|e| matches_filters(e, filters) && seen.insert(e.event().id))
        .collect()
}
