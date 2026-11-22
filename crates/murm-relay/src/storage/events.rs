use super::RelayStore;
use crate::{RelayConfig, StoreError};
use murm_protocol::*;
use rusqlite::{Connection, OptionalExtension, Transaction, params};

pub(crate) fn expire(c: &Connection, now: u64) -> Result<(), StoreError> {
    c.execute(
        "DELETE FROM events WHERE max_until IS NOT NULL AND max_until<=?1",
        [now],
    )?;
    c.execute("DELETE FROM query_snapshots WHERE expires_at<=?1", [now])?;
    c.execute("DELETE FROM propagation_jobs WHERE deadline<=?1", [now])?;
    c.execute("DELETE FROM seen_ids WHERE expires_at<=?1", [now])?;
    c.execute("DELETE FROM propagation_budgets WHERE deadline<=?1", [now])?;
    Ok(())
}
pub(crate) fn check_admission(
    event: &VerifiedEvent,
    c: &RelayConfig,
    now: u64,
) -> Result<PublicationMeta, StoreError> {
    let e = event.event();
    if e.kind != 1 {
        return Err(StoreError::Rejected(ProblemCode::UnsupportedKind));
    }
    let meta = validate_publication(event)?;
    let bytes = serde_json::to_vec(e)
        .map_err(|_| StoreError::Unavailable)?
        .len() as u64;
    if bytes > c.info.effective_policy(e.kind).event_bytes
        || bytes.saturating_add(1024) > c.info.limits.response_bytes
    {
        return Err(StoreError::Rejected(ProblemCode::ContentTooLarge));
    }
    if e.created_at > now.saturating_add(c.info.limits.future_skew_seconds) {
        return Err(StoreError::Rejected(ProblemCode::PolicyRejected));
    }
    Ok(meta)
}
fn deadline(now: u64, duration: Option<u64>) -> Result<Option<u64>, StoreError> {
    duration
        .map(|d| {
            now.checked_add(d)
                .filter(|n| *n <= MAX_SAFE_INTEGER)
                .ok_or(StoreError::Config)
        })
        .transpose()
}
pub(crate) fn admit_in_transaction(
    t: &Transaction<'_>,
    event: &VerifiedEvent,
    c: &RelayConfig,
    now: u64,
) -> Result<StoreReceipt, StoreError> {
    let meta = check_admission(event, c, now)?;
    let e = event.event();
    let id = e.id.to_string();
    if let Some(retain_until) = t
        .query_row("SELECT retain_until FROM events WHERE id=?1", [&id], |r| {
            r.get::<_, Option<u64>>(0)
        })
        .optional()?
    {
        return Ok(StoreReceipt {
            id: e.id,
            status: StoreStatus::AlreadyStored,
            retain_until,
        });
    }
    let known = t
        .query_row(
            "SELECT 1 FROM publication_heads WHERE signer=?1 AND kind=?2 AND publication=?3",
            params![e.signer.to_string(), e.kind, meta.key.publication],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !known {
        let count: u64 = t.query_row("SELECT COUNT(*) FROM publication_heads", [], |r| r.get(0))?;
        if count >= c.info.limits.publication_states {
            return Err(StoreError::Rejected(ProblemCode::LimitExceeded));
        }
    }
    let (count, used): (u64, u64) = t.query_row(
        "SELECT COUNT(*),COALESCE(SUM(length(json)),0) FROM events",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let bytes = serde_json::to_vec(e).map_err(|_| StoreError::Unavailable)?;
    if count >= c.max_storage_events
        || used.saturating_add(bytes.len() as u64) > c.max_storage_bytes
    {
        return Err(StoreError::Rejected(ProblemCode::TemporarilyUnavailable));
    }
    let policy = c.info.effective_policy(e.kind);
    let retain_until = deadline(now, policy.retention_seconds)?;
    let max_until = deadline(now, policy.max_retention_seconds)?;
    let exceptions = serde_json::to_string(&c.info.policies.early_removal)
        .map_err(|_| StoreError::Unavailable)?;
    t.execute("INSERT INTO events(id,json,signer,kind,created_at,admitted_at,retain_until,max_until,early_removal) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![id,bytes,e.signer.to_string(),e.kind,e.created_at,now,retain_until,max_until,exceptions])?;
    let status = if meta.status == PublicationStatus::Published {
        "published"
    } else {
        "withdrawn"
    };
    t.execute("INSERT INTO publication_heads(signer,kind,publication,revision,id,status) VALUES(?1,?2,?3,?4,?5,?6)
        ON CONFLICT(signer,kind,publication) DO UPDATE SET revision=excluded.revision,id=excluded.id,status=excluded.status
        WHERE excluded.revision>publication_heads.revision OR (excluded.revision=publication_heads.revision AND excluded.id>publication_heads.id)",
        params![e.signer.to_string(),e.kind,meta.key.publication,meta.revision,id,status])?;
    let suppressed = t
        .query_row(
            "SELECT 1 FROM seen_ids WHERE id=?1 AND expires_at>?2",
            params![id, now],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !suppressed {
        let p = &c.info.propagation;
        let expires = now
            .saturating_add(p.seen_seconds.max(p.max_age_seconds))
            .min(MAX_SAFE_INTEGER);
        t.execute("INSERT INTO seen_ids(id,expires_at) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET expires_at=excluded.expires_at",params![id,expires])?;
        let queued: u64 = t.query_row(
            "SELECT COUNT(DISTINCT event_id) FROM propagation_jobs",
            [],
            |r| r.get(0),
        )?;
        if p.enabled && queued < p.queue_events && p.max_attempts > 0 && p.max_age_seconds > 0 {
            let until = now.saturating_add(p.max_age_seconds).min(MAX_SAFE_INTEGER);
            t.execute("INSERT OR IGNORE INTO propagation_budgets(event_id,attempts,deadline) VALUES(?1,0,?2)",params![id,until])?;
            let mut peers = std::collections::HashSet::new();
            for peer in c
                .peers
                .iter()
                .filter(|p| peers.insert(p.as_str()))
                .take(p.max_peers as usize)
            {
                t.execute("INSERT OR IGNORE INTO propagation_jobs(event_id,peer,next_try,deadline) VALUES(?1,?2,?3,?4)",params![id,peer,now,until])?;
            }
        }
    }
    Ok(StoreReceipt {
        id: e.id,
        status: StoreStatus::Stored,
        retain_until,
    })
}
impl RelayStore {
    pub fn admit(
        &mut self,
        event: &VerifiedEvent,
        c: &RelayConfig,
        now: u64,
    ) -> Result<StoreReceipt, StoreError> {
        let t = self.connection.transaction()?;
        expire(&t, now)?;
        let receipt = admit_in_transaction(&t, event, c, now)?;
        t.commit()?;
        Ok(receipt)
    }
    pub fn fetch(&mut self, id: EventId, now: u64) -> Result<Option<VerifiedEvent>, StoreError> {
        expire(&self.connection, now)?;
        let data: Option<Vec<u8>> = self
            .connection
            .query_row(
                "SELECT json FROM events WHERE id=?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        data.map(|bytes| {
            parse_and_verify(
                &bytes,
                ParseLimits {
                    max_bytes: bytes.len(),
                    max_depth: 128,
                },
            )
            .map_err(StoreError::from)
        })
        .transpose()
    }
    pub fn evict(
        &mut self,
        id: EventId,
        reason: Option<RemovalReason>,
        now: u64,
    ) -> Result<(), StoreError> {
        let t = self.connection.transaction()?;
        let row: Option<(Option<u64>, String)> = t
            .query_row(
                "SELECT retain_until,early_removal FROM events WHERE id=?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((minimum, exceptions)) = row {
            let allowed: Vec<RemovalReason> =
                serde_json::from_str(&exceptions).map_err(|_| StoreError::Unavailable)?;
            if reason.is_some_and(|r| !allowed.contains(&r))
                || (minimum.is_some_and(|v| now < v) && reason.is_none())
            {
                return Err(StoreError::Rejected(ProblemCode::PolicyRejected));
            }
            t.execute("DELETE FROM events WHERE id=?1", [id.to_string()])?;
        }
        t.commit()?;
        Ok(())
    }
}
