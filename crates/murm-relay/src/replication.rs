use crate::cursor::hash;
use crate::storage::events::{admit_in_transaction, expire};
use crate::*;
use murm_protocol::*;
use rusqlite::{OptionalExtension, params};
use serde_json::json;
use std::{sync::Arc, time::Duration};

/// Fully checked before any database mutation. Metadata is not signed.
pub struct ValidatedSyncBatch {
    events: Vec<VerifiedEvent>,
    rejections: Vec<(EventId, ProblemCode)>,
    cursor: String,
    pub has_more: bool,
}
impl ValidatedSyncBatch {
    pub fn parse(
        bytes: &[u8],
        source: &RelayInfo,
        limits: ParseLimits,
    ) -> Result<Self, StoreError> {
        let mut outer_limits = limits;
        outer_limits.max_depth = outer_limits.max_depth.saturating_add(2);
        let value = parse_json(bytes, outer_limits)?;
        let cursor = value
            .get("cursor")
            .and_then(|v| v.as_str())
            .filter(|s| s.len() as u64 <= source.limits.cursor_bytes)
            .ok_or(ProtocolError::InvalidRequest)?
            .to_owned();
        let has_more = value
            .get("has_more")
            .and_then(|v| v.as_bool())
            .ok_or(ProtocolError::InvalidRequest)?;
        let values = value
            .get("events")
            .and_then(|v| v.as_array())
            .ok_or(ProtocolError::InvalidRequest)?;
        let mut events = Vec::new();
        let mut rejections = Vec::new();
        for v in values {
            let bytes = serde_json::to_vec(v).map_err(|_| StoreError::Unavailable)?;
            let e = parse_and_verify(
                &bytes,
                ParseLimits {
                    max_bytes: limits.max_bytes,
                    max_depth: limits.max_depth,
                },
            )?;
            if e.event().kind == 1 && validate_publication(&e).is_err() {
                if source.validated_kinds.contains(&1) {
                    return Err(ProtocolError::InvalidPublication.into());
                }
                rejections.push((e.event().id, ProblemCode::InvalidEvent));
            } else {
                events.push(e);
            }
        }
        Ok(Self {
            events,
            rejections,
            cursor,
            has_more,
        })
    }
}
pub struct SyncImportReport {
    pub accepted: usize,
    pub rejected: usize,
    pub has_more: bool,
}
impl RelayStore {
    pub fn import_sync_batch(
        &mut self,
        peer: &str,
        filters: &FilterSet,
        batch: ValidatedSyncBatch,
        c: &RelayConfig,
        now: u64,
    ) -> Result<SyncImportReport, StoreError> {
        self.configure_capacity(c.max_storage_bytes)?;
        let signature = hash(filters)?;
        let t = self.connection.transaction()?;
        expire(&t, now)?;
        let mut rejected = batch.rejections;
        let mut accepted = 0;
        for event in batch.events {
            match admit_in_transaction(&t, &event, c, now) {
                Ok(_) => accepted += 1,
                Err(StoreError::Rejected(
                    code @ (ProblemCode::PolicyRejected
                    | ProblemCode::UnsupportedKind
                    | ProblemCode::ContentTooLarge
                    | ProblemCode::LimitExceeded),
                )) => rejected.push((event.event().id, code)),
                Err(e) => return Err(e),
            }
        }
        for (id, code) in &rejected {
            t.execute("INSERT INTO sync_rejections(peer,event_id,code) VALUES(?1,?2,?3) ON CONFLICT(peer,event_id) DO UPDATE SET code=excluded.code",
                params![peer,id.to_string(),serde_json::to_string(code).map_err(|_|StoreError::Unavailable)?])?;
        }
        t.execute("INSERT INTO peer_sync(peer,filter_hash,cursor) VALUES(?1,?2,?3) ON CONFLICT(peer,filter_hash) DO UPDATE SET cursor=excluded.cursor",
            params![peer,signature,batch.cursor])?;
        t.commit()?;
        Ok(SyncImportReport {
            accepted,
            rejected: rejected.len(),
            has_more: batch.has_more,
        })
    }
}
struct Job {
    id: EventId,
    peer: String,
    bytes: Vec<u8>,
    attempt: u64,
    deadline: u64,
}
impl RelayStore {
    fn claim_job(&mut self, c: &RelayConfig, now: u64) -> Result<Option<Job>, StoreError> {
        let t = self.connection.transaction()?;
        expire(&t, now)?;
        t.execute("DELETE FROM propagation_jobs WHERE event_id IN(SELECT event_id FROM propagation_budgets WHERE attempts>=?1)",[c.info.propagation.max_attempts])?;
        let row=t.query_row("SELECT j.event_id,j.peer,e.json,j.attempts,j.deadline
            FROM propagation_jobs j JOIN propagation_budgets b ON b.event_id=j.event_id JOIN events e ON e.id=j.event_id
            WHERE j.next_try<=?1 AND j.deadline>?1 AND b.attempts<?2 ORDER BY j.next_try,j.event_id,j.peer LIMIT 1",
            params![now,c.info.propagation.max_attempts],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,Vec<u8>>(2)?,r.get::<_,u64>(3)?,r.get::<_,u64>(4)?))).optional()?;
        let Some((id, peer, bytes, attempt, deadline)) = row else {
            t.commit()?;
            return Ok(None);
        };
        t.execute(
            "UPDATE propagation_budgets SET attempts=attempts+1 WHERE event_id=?1",
            [&id],
        )?;
        t.execute("UPDATE propagation_jobs SET attempts=attempts+1,next_try=?3 WHERE event_id=?1 AND peer=?2",
            params![id,peer,now.saturating_add(c.peer_timeout_seconds+1).min(deadline)])?;
        t.commit()?;
        Ok(Some(Job {
            id: id.parse().map_err(StoreError::from)?,
            peer,
            bytes,
            attempt: attempt + 1,
            deadline,
        }))
    }
    fn finish_job(&mut self, job: &Job, retry: Option<u64>, now: u64) -> Result<(), StoreError> {
        if let Some(wait) = retry {
            let backoff = 2u64.saturating_pow(job.attempt.min(10) as u32);
            let next = now.saturating_add(wait.max(backoff));
            if next < job.deadline {
                self.connection.execute(
                    "UPDATE propagation_jobs SET next_try=?3 WHERE event_id=?1 AND peer=?2",
                    params![job.id.to_string(), job.peer, next],
                )?;
                return Ok(());
            }
        }
        self.connection.execute(
            "DELETE FROM propagation_jobs WHERE event_id=?1 AND peer=?2",
            params![job.id.to_string(), job.peer],
        )?;
        Ok(())
    }
}
pub struct Replicator {
    service: Arc<RelayService>,
    client: reqwest::Client,
}
pub async fn read_bounded(
    mut response: reqwest::Response,
    max: u64,
) -> Result<Vec<u8>, StoreError> {
    if response.content_length().is_some_and(|n| n > max) {
        return Err(ProtocolError::ContentTooLarge.into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| StoreError::Unavailable)?
    {
        if (bytes.len() as u64).saturating_add(chunk.len() as u64) > max {
            return Err(ProtocolError::ContentTooLarge.into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
impl Replicator {
    pub fn new(service: Arc<RelayService>) -> Result<Self, StoreError> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(service.config.peer_timeout_seconds))
            .build()
            .map_err(|_| StoreError::Config)?;
        Ok(Self { service, client })
    }
    pub async fn propagate_once(&self) -> Result<bool, StoreError> {
        let c = self.service.config.clone();
        let now = self.service.clock.now_unix();
        if !c.info.propagation.enabled {
            return Ok(false);
        }
        let config = c.clone();
        let Some(job) = self
            .service
            .db
            .call(move |s| s.claim_job(&config, now))
            .await?
        else {
            return Ok(false);
        };
        let remaining = job.deadline.saturating_sub(now);
        let response = self
            .client
            .post(format!("{}/events", job.peer.trim_end_matches('/')))
            .header("content-type", "application/json")
            .timeout(Duration::from_secs(
                c.peer_timeout_seconds.min(remaining).max(1),
            ))
            .body(job.bytes.clone())
            .send()
            .await;
        let retry = match response {
            Err(_) => Some(0),
            Ok(response) => {
                let status = response.status().as_u16();
                let wait = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0);
                if matches!(status, 200 | 201) {
                    let bytes = read_bounded(response, 8192).await?;
                    let valid = parse_json(
                        &bytes,
                        ParseLimits {
                            max_bytes: 8192,
                            max_depth: 8,
                        },
                    )
                    .ok()
                    .and_then(|v| serde_json::from_value::<StoreReceipt>(v).ok())
                    .is_some_and(|r| {
                        r.id == job.id
                            && ((status == 201 && r.status == StoreStatus::Stored)
                                || (status == 200 && r.status == StoreStatus::AlreadyStored))
                    });
                    if valid { None } else { Some(0) }
                } else if matches!(status, 408 | 429 | 500..=599) {
                    Some(wait)
                } else {
                    None
                }
            }
        };
        let now = self.service.clock.now_unix();
        self.service
            .db
            .call(move |s| s.finish_job(&job, retry, now))
            .await?;
        Ok(true)
    }
    async fn backoff(&self, peer: &str, until: Option<u64>) -> Result<u64, StoreError> {
        let key = format!("peer_retry:{}", hash(&peer)?);
        self.service.db.call(move|s|{
            if let Some(until)=until {
                s.connection.execute("INSERT INTO relay_meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,until.to_string()])?;
            }
            let value:Option<String>=s.connection.query_row("SELECT value FROM relay_meta WHERE key=?1",[key],|r|r.get(0)).optional()?;
            Ok(value.and_then(|v|v.parse().ok()).unwrap_or(0))
        }).await
    }
    pub async fn sync_once(&self, peer: &str) -> Result<bool, StoreError> {
        validate_relay_url(peer)?;
        if !self.service.config.peers.iter().any(|p| p == peer) {
            return Err(StoreError::Config);
        }
        let now = self.service.clock.now_unix();
        if self.backoff(peer, None).await? > now {
            return Ok(false);
        }
        let info_response = self
            .client
            .get(format!("{}/info", peer.trim_end_matches('/')))
            .send()
            .await
            .map_err(|_| StoreError::Unavailable)?;
        if info_response.status() == 429 {
            let wait = info_response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(60);
            self.backoff(peer, Some(now.saturating_add(wait))).await?;
            return Ok(false);
        }
        if !info_response.status().is_success() {
            return Err(StoreError::Unavailable);
        }
        let bytes = read_bounded(info_response, 65536).await?;
        let source: RelayInfo = serde_json::from_value(parse_json(
            &bytes,
            ParseLimits {
                max_bytes: 65536,
                max_depth: 32,
            },
        )?)
        .map_err(|_| StoreError::Config)?;
        source.validate()?;
        let filters: FilterSet =
            serde_json::from_value(json!([{"kinds":[1]}])).map_err(|_| StoreError::Config)?;
        let filter_hash = hash(&filters)?;
        let peer_owned = peer.to_owned();
        let cursor: Option<String> = self
            .service
            .db
            .call(move |s| {
                Ok(s.connection
                    .query_row(
                        "SELECT cursor FROM peer_sync WHERE peer=?1 AND filter_hash=?2",
                        params![peer_owned, filter_hash],
                        |r| r.get::<_, Option<String>>(0),
                    )
                    .optional()?
                    .flatten())
            })
            .await?;
        let mut query = SyncRequest {
            filters: filters.clone(),
            cursor,
            limit: source
                .limits
                .query_results
                .min(self.service.config.info.limits.query_results),
        };
        let mut post = !source.features.sync;
        let mut reset = false;
        for _ in 0..3 {
            let response = self
                .client
                .request(
                    if post {
                        reqwest::Method::POST
                    } else {
                        reqwest::Method::from_bytes(b"QUERY").map_err(|_| StoreError::Config)?
                    },
                    format!("{}/events/sync", peer.trim_end_matches('/')),
                )
                .json(&query)
                .send()
                .await
                .map_err(|_| StoreError::Unavailable)?;
            let status = response.status().as_u16();
            if !post && matches!(status, 405 | 501) {
                post = true;
                continue;
            }
            if status == 429 {
                let wait = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(60);
                self.backoff(peer, Some(now.saturating_add(wait))).await?;
                return Ok(false);
            }
            let max = self
                .service
                .config
                .info
                .limits
                .response_bytes
                .min(source.limits.response_bytes);
            let bytes = read_bounded(response, max).await?;
            if status == 410
                && !reset
                && serde_json::from_value::<Problem>(parse_json(
                    &bytes,
                    ParseLimits {
                        max_bytes: max as usize,
                        max_depth: 32,
                    },
                )?)
                .is_ok_and(|p| p.code == ProblemCode::CursorExpired)
            {
                query.cursor = None;
                reset = true;
                continue;
            }
            if status != 200 {
                return Err(StoreError::Unavailable);
            }
            let batch = ValidatedSyncBatch::parse(
                &bytes,
                &source,
                ParseLimits {
                    max_bytes: max as usize,
                    max_depth: self.service.config.info.limits.json_depth as usize,
                },
            )?;
            let more = batch.has_more;
            let c = self.service.config.clone();
            let peer = peer.to_owned();
            let now = self.service.clock.now_unix();
            self.service
                .db
                .call(move |s| s.import_sync_batch(&peer, &filters, batch, &c, now))
                .await?;
            return Ok(more);
        }
        Err(StoreError::Unavailable)
    }
    pub async fn run(self) {
        let mut propagation = tokio::time::interval(Duration::from_millis(200));
        let mut sync = tokio::time::interval(Duration::from_secs(
            self.service.config.sync_interval_seconds,
        ));
        loop {
            tokio::select! {
                _=propagation.tick()=>{let _=self.propagate_once().await;}
                _=sync.tick()=>{
                    for peer in &self.service.config.peers {
                        for _ in 0..8 {if !self.sync_once(peer).await.unwrap_or(false){break;}}
                    }
                }
            }
        }
    }
}
