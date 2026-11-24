use crate::*;
use crate::{profiles::get_profile, transport::validate_url};
use murm_protocol::*;
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
pub(crate) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub(crate) fn filter_hash(filters: &FilterSet) -> Result<String, ClientError> {
    Ok(hex::encode(Sha256::digest(
        serde_jcs::to_vec(filters).map_err(|_| ClientError::InvalidInput)?,
    )))
}
pub(crate) fn cache_event(t: &Transaction<'_>, e: &VerifiedEvent) -> Result<bool, ClientError> {
    let event = e.event();
    let meta = validate_publication(e)?;
    if t.query_row(
        "SELECT 1 FROM verified_events WHERE id=?1",
        [event.id.to_string()],
        |_| Ok(()),
    )
    .optional()?
    .is_some()
    {
        return Ok(false);
    }
    let bytes = serde_json::to_vec(e).map_err(|_| ClientError::InvalidInput)?;
    let stored: Option<String> = t
        .query_row(
            "SELECT value FROM client_meta WHERE key='cache_bytes'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let used = stored.and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    let count: u64 = t.query_row("SELECT COUNT(*) FROM verified_events", [], |r| r.get(0))?;
    if count >= 100000 || used.saturating_add(bytes.len() as u64) > 536870912 {
        return Err(ClientError::Conflict);
    }
    let known = t
        .query_row(
            "SELECT 1 FROM publication_heads WHERE signer=?1 AND kind=?2 AND publication=?3",
            params![event.signer.to_string(), event.kind, meta.key.publication],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    let heads: u64 = t.query_row("SELECT COUNT(*) FROM publication_heads", [], |r| r.get(0))?;
    if !known && heads >= 100000 {
        return Err(ClientError::Conflict);
    }
    t.execute(
        "INSERT INTO verified_events(id,json,signer,kind,created_at) VALUES(?1,?2,?3,?4,?5)",
        params![
            event.id.to_string(),
            bytes,
            event.signer.to_string(),
            event.kind,
            event.created_at
        ],
    )?;
    t.execute("INSERT INTO client_meta(key,value) VALUES('cache_bytes',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[(used+bytes.len() as u64).to_string()])?;
    t.execute("INSERT INTO publication_heads(signer,kind,publication,revision,id,status) VALUES(?1,?2,?3,?4,?5,?6)
        ON CONFLICT(signer,kind,publication) DO UPDATE SET revision=excluded.revision,id=excluded.id,status=excluded.status
        WHERE excluded.revision>publication_heads.revision OR (excluded.revision=publication_heads.revision AND excluded.id>publication_heads.id)",
        params![event.signer.to_string(),event.kind,meta.key.publication,meta.revision,event.id.to_string(),if meta.status==PublicationStatus::Published{"published"}else{"withdrawn"}])?;
    Ok(true)
}
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SyncSummary {
    pub received: usize,
    pub accepted: usize,
    pub rejected: usize,
    pub has_more: bool,
}
impl ClientService {
    pub async fn add_relay(&self, profile: ProfileId, url: &str) -> Result<(), ClientError> {
        let url = validate_url(url)?;
        self.db
            .call(move |c| {
                get_profile(c, profile)?;
                c.execute(
                    "INSERT OR IGNORE INTO profile_relays(profile_id,url) VALUES(?1,?2)",
                    params![profile.to_string(), url],
                )?;
                Ok(())
            })
            .await
    }
    pub async fn remove_relay(&self, profile: ProfileId, url: &str) -> Result<(), ClientError> {
        let url = validate_url(url)?;
        self.db
            .call(move |c| {
                let transaction=c.transaction()?;
                transaction.execute(
                    "DELETE FROM profile_relays WHERE profile_id=?1 AND url=?2",
                    params![profile.to_string(), url],
                )?;
                transaction.execute("UPDATE outbox_destinations SET state='refused',retry_at=NULL,last_error='relay removed locally'
                    WHERE relay=?2 AND state IN ('pending','retry_at') AND event_id IN (SELECT event_id FROM outbox WHERE profile_id=?1)",
                    params![profile.to_string(),url])?;
                transaction.commit()?;
                Ok(())
            })
            .await
    }
    async fn require_relay(&self, profile: ProfileId, url: &str) -> Result<(), ClientError> {
        let url = url.to_owned();
        self.db
            .call(move |c| {
                get_profile(c, profile)?;
                c.query_row(
                    "SELECT 1 FROM profile_relays WHERE profile_id=?1 AND url=?2",
                    params![profile.to_string(), url],
                    |_| Ok(()),
                )
                .optional()?
                .ok_or(ClientError::NotFound)
            })
            .await
    }
    pub async fn relay_info(
        &self,
        profile: ProfileId,
        url: &str,
    ) -> Result<RelayInfo, ClientError> {
        let url = validate_url(url)?;
        self.require_relay(profile, &url).await?;
        let info = RelayTransport::new(&url)?.info().await?;
        let value = serde_json::to_string(&info).map_err(|_| ClientError::InvalidInput)?;
        self.db.call(move|c|{c.execute("UPDATE profile_relays SET info=?3,last_error=NULL WHERE profile_id=?1 AND url=?2",params![profile.to_string(),url,value])?;Ok(())}).await?;
        Ok(info)
    }
    pub async fn cached_event(&self, id: EventId) -> Result<Option<VerifiedEvent>, ClientError> {
        self.db
            .call(move |c| {
                let bytes: Option<Vec<u8>> = c
                    .query_row(
                        "SELECT json FROM verified_events WHERE id=?1",
                        [id.to_string()],
                        |r| r.get(0),
                    )
                    .optional()?;
                bytes
                    .map(|b| {
                        parse_and_verify(
                            &b,
                            ParseLimits {
                                max_bytes: b.len(),
                                max_depth: 128,
                            },
                        )
                        .map_err(ClientError::from)
                    })
                    .transpose()
            })
            .await
    }
    pub async fn fetch_event(
        &self,
        profile: ProfileId,
        url: &str,
        id: EventId,
    ) -> Result<VerifiedEvent, ClientError> {
        let url = validate_url(url)?;
        self.require_relay(profile, &url).await?;
        let e = RelayTransport::new(&url)?.fetch(id).await?;
        let event = e.clone();
        self.db
            .call(move |c| {
                let t = c.transaction()?;
                cache_event(&t, &event)?;
                t.commit()?;
                Ok(())
            })
            .await?;
        Ok(e)
    }
    pub async fn query_relay(
        &self,
        profile: ProfileId,
        url: &str,
        q: EventQuery,
    ) -> Result<VerifiedQuery, ClientError> {
        let url = validate_url(url)?;
        let info = self.relay_info(profile, &url).await?;
        let result = RelayTransport::new(&url)?.query(&q, &info).await?;
        let events = result.events.clone();
        self.db
            .call(move |c| {
                let t = c.transaction()?;
                for e in events {
                    cache_event(&t, &e)?;
                }
                t.commit()?;
                Ok(())
            })
            .await?;
        Ok(result)
    }
    pub async fn sync_profile(
        &self,
        profile: ProfileId,
        url: &str,
    ) -> Result<SyncSummary, ClientError> {
        let filters = vec![Filter {
            kinds: Some(vec![1]),
            ..Default::default()
        }];
        self.sync_filtered(profile, url, filters).await
    }
    pub async fn sync_filtered(
        &self,
        profile: ProfileId,
        url: &str,
        filters: FilterSet,
    ) -> Result<SyncSummary, ClientError> {
        let url = validate_url(url)?;
        self.require_relay(profile, &url).await?;
        let signature = filter_hash(&filters)?;
        let peer = url.clone();
        let now = self.clock.now_unix();
        let wait = self
            .db
            .call(move |c| {
                let until: Option<u64> = c
                    .query_row(
                        "SELECT retry_at FROM relay_backoff WHERE profile_id=?1 AND relay=?2",
                        params![profile.to_string(), peer],
                        |r| r.get(0),
                    )
                    .optional()?;
                Ok(until.unwrap_or(0).saturating_sub(now))
            })
            .await?;
        if wait > 0 {
            return Err(ClientError::RetryAfter(wait));
        }
        let result = self.sync_inner(profile, &url, filters, signature).await;
        if let Err(ClientError::RetryAfter(wait)) = &result {
            let until = now.saturating_add(*wait);
            let peer = url.clone();
            self.db.call(move|c|{
                c.execute("INSERT INTO relay_backoff(profile_id,relay,retry_at) VALUES(?1,?2,?3) ON CONFLICT(profile_id,relay) DO UPDATE SET retry_at=excluded.retry_at",params![profile.to_string(),peer,until])?;Ok(())
            }).await?;
        }
        result
    }
    async fn sync_inner(
        &self,
        profile: ProfileId,
        url: &str,
        filters: FilterSet,
        signature: String,
    ) -> Result<SyncSummary, ClientError> {
        let info = self.relay_info(profile, url).await?;
        let transport = RelayTransport::new(url)?;
        let peer = url.to_owned();
        let sig = signature.clone();
        let mut cursor=self.db.call(move|c|{
            Ok(c.query_row("SELECT cursor FROM sync_cursors WHERE profile_id=?1 AND relay=?2 AND filter_hash=?3",
                params![profile.to_string(),peer,sig],|r|r.get::<_,String>(0)).optional()?)
        }).await?;
        let mut summary = SyncSummary::default();
        let mut reset = false;
        for _ in 0..16 {
            let request = SyncRequest {
                filters: filters.clone(),
                cursor: cursor.clone(),
                limit: info.limits.query_results.min(100),
            };
            let batch = match transport.sync(&request, &info).await {
                Err(ClientError::Relay(ProblemCode::CursorExpired)) if !reset => {
                    cursor = None;
                    reset = true;
                    continue;
                }
                other => other?,
            };
            summary.received += batch.events.len() + batch.rejections.len();
            summary.rejected += batch.rejections.len();
            summary.has_more = batch.has_more;
            cursor = Some(batch.cursor.clone());
            let peer = url.to_owned();
            let sig = signature.clone();
            let accepted=self.db.call(move|c|{
                get_profile(c,profile)?;let t=c.transaction()?;let mut accepted=0;
                for e in &batch.events{accepted+=usize::from(cache_event(&t,e)?);}
                for (id,code) in batch.rejections {
                    t.execute("INSERT INTO sync_rejections(profile_id,relay,event_id,code) VALUES(?1,?2,?3,?4) ON CONFLICT(profile_id,relay,event_id) DO UPDATE SET code=excluded.code",
                        params![profile.to_string(),peer,id.to_string(),serde_json::to_string(&code).map_err(|_|ClientError::InvalidInput)?])?;
                }
                t.execute("INSERT INTO sync_cursors(profile_id,relay,filter_hash,cursor) VALUES(?1,?2,?3,?4) ON CONFLICT(profile_id,relay,filter_hash) DO UPDATE SET cursor=excluded.cursor",
                    params![profile.to_string(),peer,sig,batch.cursor])?;t.commit()?;Ok(accepted)
            }).await?;
            summary.accepted += accepted;
            if !summary.has_more {
                break;
            }
        }
        Ok(summary)
    }
}
