use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResponse {
    pub events: Vec<Event>,
    #[serde(deserialize_with = "required_nullable")]
    pub next_cursor: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResponse {
    pub events: Vec<Event>,
    pub cursor: String,
    pub has_more: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreStatus {
    Stored,
    AlreadyStored,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreReceipt {
    pub id: EventId,
    pub status: StoreStatus,
    #[serde(deserialize_with = "required_nullable")]
    pub retain_until: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayInfo {
    pub protocol: String,
    pub versions: Vec<u64>,
    pub profiles: Vec<String>,
    pub accepted_kinds: Vec<u64>,
    pub validated_kinds: Vec<u64>,
    pub accept_unknown_kinds: bool,
    pub features: Features,
    pub limits: Limits,
    pub kind_policies: KindPolicies,
    pub rate_limits: Vec<RateLimit>,
    pub policies: Policies,
    pub propagation: Propagation,
    pub header_filters: Vec<HeaderCapability>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Features {
    pub http_query: bool,
    pub post_query: bool,
    pub sync: bool,
    pub post_sync: bool,
    pub current_kinds: Vec<u64>,
    pub relay_announcements: bool,
    pub lookup: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Limits {
    pub event_bytes: u64,
    pub request_bytes: u64,
    pub json_depth: u64,
    pub query_bytes: u64,
    pub query_results: u64,
    pub response_bytes: u64,
    pub filters: u64,
    pub header_conditions: u64,
    pub filter_values: u64,
    pub cursor_bytes: u64,
    pub query_cursor_seconds: u64,
    pub sync_cursor_seconds: u64,
    pub snapshot_events: u64,
    pub publication_states: u64,
    pub future_skew_seconds: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KindPolicy {
    pub event_bytes: u64,
    #[serde(deserialize_with = "required_nullable")]
    pub retention_seconds: Option<u64>,
    #[serde(deserialize_with = "required_nullable")]
    pub max_retention_seconds: Option<u64>,
}
fn required_nullable<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(deserializer)
}
fn nullable<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<u64>>, D::Error> {
    Option::<u64>::deserialize(d).map(Some)
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KindPolicyOverride {
    pub kind: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_bytes: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "nullable"
    )]
    pub retention_seconds: Option<Option<u64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "nullable"
    )]
    pub max_retention_seconds: Option<Option<u64>>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KindPolicies {
    pub default: KindPolicy,
    pub overrides: Vec<KindPolicyOverride>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Publish,
    Fetch,
    Query,
    Sync,
    Info,
    Relays,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateScope {
    Ip,
    Global,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimit {
    pub operation: Operation,
    pub scope: RateScope,
    pub requests: u64,
    pub window_seconds: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemovalReason {
    Policy,
    Legal,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policies {
    pub early_removal: Vec<RemovalReason>,
    pub description: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Propagation {
    pub enabled: bool,
    pub max_peers: u64,
    pub max_attempts: u64,
    pub max_age_seconds: u64,
    pub queue_events: u64,
    pub seen_seconds: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderCapability {
    pub kind: u64,
    pub path: String,
    pub ops: Vec<HeaderOp>,
}

impl RelayInfo {
    pub fn effective_policy(&self, kind: u64) -> KindPolicy {
        let mut p = self.kind_policies.default.clone();
        if let Some(o) = self.kind_policies.overrides.iter().find(|p| p.kind == kind) {
            if let Some(v) = o.event_bytes {
                p.event_bytes = v;
            }
            if let Some(v) = o.retention_seconds {
                p.retention_seconds = v;
            }
            if let Some(v) = o.max_retention_seconds {
                p.max_retention_seconds = v;
            }
        }
        p.event_bytes = p.event_bytes.min(self.limits.event_bytes);
        p
    }
    pub fn parse_limits(&self) -> ParseLimits {
        ParseLimits {
            max_bytes: self.limits.request_bytes as usize,
            max_depth: self.limits.json_depth as usize,
        }
    }
    pub fn validate(&self) -> Result<(), ProtocolError> {
        let bad = ProtocolError::InvalidRequest;
        fn kinds(v: &[u64]) -> bool {
            v.iter().all(|&x| x <= MAX_SAFE_INTEGER)
                && v.iter().collect::<HashSet<_>>().len() == v.len()
        }
        if self.protocol != "murm"
            || self.versions.is_empty()
            || !kinds(&self.versions)
            || !kinds(&self.accepted_kinds)
            || !kinds(&self.validated_kinds)
            || !kinds(&self.features.current_kinds)
            || self
                .validated_kinds
                .iter()
                .any(|k| !self.accepted_kinds.contains(k))
            || self
                .features
                .current_kinds
                .iter()
                .any(|k| !self.validated_kinds.contains(k))
        {
            return Err(bad);
        }
        let numeric = serde_json::to_value(&self.limits).map_err(|_| bad.clone())?;
        for (name, v) in numeric.as_object().ok_or(bad.clone())? {
            let n = v.as_u64().ok_or(bad.clone())?;
            if n > MAX_SAFE_INTEGER || (n == 0 && name != "future_skew_seconds") {
                return Err(bad);
            }
        }
        if self.limits.request_bytes < self.limits.event_bytes
            || self.limits.request_bytes < self.limits.query_bytes
            || self.limits.response_bytes < self.limits.event_bytes.saturating_add(1024)
        {
            return Err(bad);
        }
        let mut overrides = HashSet::new();
        for o in &self.kind_policies.overrides {
            if o.kind > MAX_SAFE_INTEGER
                || !overrides.insert(o.kind)
                || o.event_bytes
                    .is_some_and(|n| n == 0 || n > MAX_SAFE_INTEGER)
            {
                return Err(bad);
            }
        }
        for p in std::iter::once(self.kind_policies.default.clone()).chain(
            self.kind_policies
                .overrides
                .iter()
                .map(|o| self.effective_policy(o.kind)),
        ) {
            if p.event_bytes == 0
                || p.event_bytes > MAX_SAFE_INTEGER
                || [p.retention_seconds, p.max_retention_seconds]
                    .into_iter()
                    .flatten()
                    .any(|n| n > MAX_SAFE_INTEGER)
                || matches!((p.retention_seconds,p.max_retention_seconds),(Some(a),Some(b)) if a>b)
            {
                return Err(bad);
            }
        }
        if self.rate_limits.iter().any(|r| {
            r.requests == 0
                || r.window_seconds == 0
                || r.requests > MAX_SAFE_INTEGER
                || r.window_seconds > MAX_SAFE_INTEGER
        }) || self
            .policies
            .early_removal
            .iter()
            .collect::<HashSet<_>>()
            .len()
            != self.policies.early_removal.len()
        {
            return Err(bad);
        }
        for n in [
            self.propagation.max_peers,
            self.propagation.max_attempts,
            self.propagation.max_age_seconds,
            self.propagation.queue_events,
            self.propagation.seen_seconds,
        ] {
            if n > MAX_SAFE_INTEGER {
                return Err(bad);
            }
        }
        Ok(())
    }
}
impl Default for RelayInfo {
    fn default() -> Self {
        Self {
            protocol: "murm".into(),
            versions: vec![1],
            profiles: vec!["publications-test-v1".into()],
            accepted_kinds: vec![1],
            validated_kinds: vec![1],
            accept_unknown_kinds: false,
            features: Features {
                http_query: true,
                post_query: true,
                sync: true,
                post_sync: true,
                current_kinds: vec![1],
                relay_announcements: false,
                lookup: false,
            },
            limits: Limits {
                event_bytes: 1048576,
                request_bytes: 1048576,
                json_depth: 32,
                query_bytes: 65536,
                query_results: 100,
                response_bytes: 4194304,
                filters: 10,
                header_conditions: 20,
                filter_values: 100,
                cursor_bytes: 4096,
                query_cursor_seconds: 1800,
                sync_cursor_seconds: 604800,
                snapshot_events: 10000,
                publication_states: 100000,
                future_skew_seconds: 1800,
            },
            kind_policies: KindPolicies {
                default: KindPolicy {
                    event_bytes: 1048576,
                    retention_seconds: Some(604800),
                    max_retention_seconds: Some(2592000),
                },
                overrides: vec![KindPolicyOverride {
                    kind: 1,
                    event_bytes: Some(1048576),
                    retention_seconds: Some(Some(2592000)),
                    max_retention_seconds: Some(Some(7776000)),
                }],
            },
            rate_limits: vec![
                RateLimit {
                    operation: Operation::Publish,
                    scope: RateScope::Ip,
                    requests: 60,
                    window_seconds: 60,
                },
                RateLimit {
                    operation: Operation::Query,
                    scope: RateScope::Ip,
                    requests: 120,
                    window_seconds: 60,
                },
                RateLimit {
                    operation: Operation::Sync,
                    scope: RateScope::Global,
                    requests: 600,
                    window_seconds: 60,
                },
            ],
            policies: Policies {
                early_removal: vec![RemovalReason::Policy, RemovalReason::Legal],
                description: "Public articles; local storage and bounded peer propagation.".into(),
            },
            propagation: Propagation {
                enabled: true,
                max_peers: 2,
                max_attempts: 6,
                max_age_seconds: 3600,
                queue_events: 1000,
                seen_seconds: 86400,
            },
            header_filters: [
                ("/publication", HeaderOp::Eq),
                ("/revision", HeaderOp::Eq),
                ("/status", HeaderOp::Eq),
                ("/topics", HeaderOp::Contains),
                ("/language", HeaderOp::Eq),
            ]
            .into_iter()
            .map(|(p, op)| HeaderCapability {
                kind: 1,
                path: p.into(),
                ops: vec![op],
            })
            .collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProblemCode {
    InvalidJson,
    InvalidRequest,
    InvalidEvent,
    InvalidId,
    InvalidSignature,
    UnsupportedVersion,
    UnsupportedKind,
    UnsupportedFilter,
    UnsupportedFeature,
    InvalidCursor,
    CursorExpired,
    LimitExceeded,
    ContentTooLarge,
    UnsupportedMediaType,
    NotFound,
    PolicyRejected,
    RateLimited,
    TemporarilyUnavailable,
}
impl ProblemCode {
    pub fn status(self) -> u16 {
        match self {
            Self::InvalidJson | Self::InvalidRequest | Self::InvalidCursor => 400,
            Self::CursorExpired => 410,
            Self::ContentTooLarge => 413,
            Self::UnsupportedMediaType => 415,
            Self::NotFound => 404,
            Self::PolicyRejected => 403,
            Self::RateLimited => 429,
            Self::TemporarilyUnavailable => 503,
            _ => 422,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Problem {
    pub r#type: String,
    pub title: String,
    pub status: u16,
    pub code: ProblemCode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pointer: Option<String>,
}
impl Problem {
    pub fn new(code: ProblemCode) -> Self {
        let value = serde_json::to_value(code).expect("enum serializes as a string");
        let text = value.as_str().expect("enum string");
        Self {
            r#type: format!("urn:murm:problem:{text}"),
            title: text.replace('_', " "),
            status: code.status(),
            code,
            detail: None,
            pointer: None,
        }
    }
}
impl From<ProtocolError> for ProblemCode {
    fn from(e: ProtocolError) -> Self {
        match e {
            ProtocolError::InvalidJson => Self::InvalidJson,
            ProtocolError::InvalidEvent | ProtocolError::InvalidPublication => Self::InvalidEvent,
            ProtocolError::UnsupportedVersion => Self::UnsupportedVersion,
            ProtocolError::InvalidId => Self::InvalidId,
            ProtocolError::InvalidSignature => Self::InvalidSignature,
            ProtocolError::InvalidRequest => Self::InvalidRequest,
            ProtocolError::UnsupportedFilter => Self::UnsupportedFilter,
            ProtocolError::UnsupportedFeature => Self::UnsupportedFeature,
            ProtocolError::LimitExceeded => Self::LimitExceeded,
            ProtocolError::ContentTooLarge => Self::ContentTooLarge,
        }
    }
}
