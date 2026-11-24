use crate::StoreError;
use murm_protocol::*;
use serde::{Deserialize, Serialize};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RelayConfig {
    pub database_path: PathBuf,
    pub listen: SocketAddr,
    pub peers: Vec<String>,
    pub info: RelayInfo,
    pub max_storage_events: u64,
    pub max_storage_bytes: u64,
    pub max_snapshots: u64,
    pub max_snapshot_events: u64,
    pub peer_timeout_seconds: u64,
    pub sync_interval_seconds: u64,
}
impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            database_path: "relay.sqlite".into(),
            listen: "127.0.0.1:8080".parse().expect("constant address"),
            peers: vec![],
            info: RelayInfo::default(),
            max_storage_events: 100000,
            max_storage_bytes: 536870912,
            max_snapshots: 100,
            max_snapshot_events: 100000,
            peer_timeout_seconds: 20,
            sync_interval_seconds: 10,
        }
    }
}
impl RelayConfig {
    pub fn load(path: &Path) -> Result<Self, StoreError> {
        let text = std::fs::read_to_string(path)?;
        if text.len() > 4194304 {
            return Err(StoreError::Config);
        }
        let c: Self = if text.trim_start().starts_with('{') {
            serde_json::from_value(parse_json(
                text.as_bytes(),
                ParseLimits {
                    max_bytes: 4194304,
                    max_depth: 64,
                },
            )?)
            .map_err(|_| StoreError::Config)?
        } else {
            toml::from_str(&text).map_err(|_| StoreError::Config)?
        };
        c.validate()?;
        Ok(c)
    }
    pub fn validate(&self) -> Result<(), StoreError> {
        self.info.validate()?;
        if !self.listen.ip().is_loopback()
            || self.max_storage_events == 0
            || self.max_storage_bytes == 0
            || self.max_snapshots == 0
            || self.max_snapshot_events == 0
            || self.peer_timeout_seconds == 0
            || self.sync_interval_seconds == 0
            || self.info.versions != [1]
            || self.info.profiles != ["publications-test-v1"]
            || self.info.accepted_kinds != [1]
            || self.info.validated_kinds != [1]
            || self.info.features.current_kinds != [1]
            || self.info.accept_unknown_kinds
            || self.info.features.lookup
            || self.info.features.relay_announcements
            || !self.info.features.http_query
            || !self.info.features.post_query
            || !self.info.features.sync
            || !self.info.features.post_sync
        {
            return Err(StoreError::Config);
        }
        let supported = RelayInfo::default().header_filters;
        if self.info.header_filters.len() != supported.len()
            || self.info.limits.json_depth > 64
            || self.info.limits.cursor_bytes < 1024
            || self.info.limits.filters.saturating_mul(
                self.info
                    .limits
                    .filter_values
                    .saturating_mul(2)
                    .saturating_add(self.info.limits.header_conditions.saturating_mul(3))
                    .saturating_add(2),
            ) > 30000
            || serde_json::to_vec(&self.info)
                .map_err(|_| StoreError::Config)?
                .len() as u64
                > self.info.limits.response_bytes
        {
            return Err(StoreError::Config);
        }
        for cap in supported {
            if !self
                .info
                .header_filters
                .iter()
                .any(|c| c.kind == cap.kind && c.path == cap.path && c.ops == cap.ops)
            {
                return Err(StoreError::Config);
            }
        }
        for url in &self.peers {
            validate_relay_url(url)?;
        }
        let now = SystemClock.now_unix();
        for kind in [0, 1] {
            let p = self.info.effective_policy(kind);
            for n in [p.retention_seconds, p.max_retention_seconds]
                .into_iter()
                .flatten()
            {
                if now.checked_add(n).is_none_or(|v| v > MAX_SAFE_INTEGER) {
                    return Err(StoreError::Config);
                }
            }
        }
        Ok(())
    }
}
pub fn validate_relay_url(value: &str) -> Result<reqwest::Url, StoreError> {
    let u = reqwest::Url::parse(value).map_err(|_| StoreError::Config)?;
    let loopback = u.host_str().is_some_and(|s| {
        s == "localhost"
            || s.trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
        || (u.scheme() != "https" && !(u.scheme() == "http" && loopback))
        || u.host().is_none()
        || u.path() != "/"
    {
        return Err(StoreError::Config);
    }
    Ok(u)
}
pub trait Clock: Send + Sync {
    fn now_unix(&self) -> u64;
}
pub struct SystemClock;
impl Clock for SystemClock {
    fn now_unix(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}
