use crate::*;
use murm_protocol::*;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DestinationState {
    Pending,
    Stored,
    Refused,
    RetryAt,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DestinationStatus {
    pub relay: String,
    pub state: DestinationState,
    pub retain_until: Option<u64>,
    pub retry_at: Option<u64>,
    pub attempts: u64,
    pub last_error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxEntry {
    pub event_id: EventId,
    pub profile_id: ProfileId,
    pub created_at: u64,
    pub destinations: Vec<DestinationStatus>,
}
fn state(s: &str) -> Result<DestinationState, ClientError> {
    match s {
        "pending" => Ok(DestinationState::Pending),
        "stored" => Ok(DestinationState::Stored),
        "refused" => Ok(DestinationState::Refused),
        "retry_at" => Ok(DestinationState::RetryAt),
        _ => Err(ClientError::InvalidInput),
    }
}
impl ClientService {
    pub async fn outbox(&self, profile: ProfileId) -> Result<Vec<OutboxEntry>, ClientError> {
        self.db.call(move|c|{
            let rows=c.prepare("SELECT event_id,created_at FROM outbox WHERE profile_id=?1 ORDER BY created_at DESC,event_id DESC LIMIT 1000")?
                .query_map([profile.to_string()],|r|Ok((r.get::<_,String>(0)?,r.get::<_,u64>(1)?)))?.collect::<Result<Vec<_>,_>>()?;
            let mut result=Vec::new();for (id,created_at) in rows{
                let destinations=c.prepare("SELECT relay,state,retain_until,retry_at,attempts,last_error FROM outbox_destinations WHERE event_id=?1 ORDER BY relay")?
                    .query_map([&id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)))?
                    .map(|row|{let (relay,s,retain_until,retry_at,attempts,last_error)=row?;Ok(DestinationStatus{relay,state:state(&s)?,retain_until,retry_at,attempts,last_error})})
                    .collect::<Result<Vec<_>,ClientError>>()?;
                result.push(OutboxEntry{event_id:id.parse()?,profile_id:profile,created_at,destinations});
            }Ok(result)
        }).await
    }
    pub async fn retry_event(&self, profile: ProfileId, id: EventId) -> Result<(), ClientError> {
        let entry = self
            .outbox(profile)
            .await?
            .into_iter()
            .find(|e| e.event_id == id)
            .ok_or(ClientError::NotFound)?;
        for destination in entry.destinations {
            let relay = destination.relay.clone();
            let now = self.clock.now_unix();
            let event=self.db.call(move|c|{
                let t=c.transaction()?;
                let row:Option<(String,Option<u64>,u64,Vec<u8>)>=t.query_row("SELECT d.state,d.retry_at,d.attempts,o.json FROM outbox_destinations d JOIN outbox o ON o.event_id=d.event_id WHERE o.profile_id=?1 AND d.event_id=?2 AND d.relay=?3",
                    params![profile.to_string(),id.to_string(),relay],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
                let (s,retry,attempts,bytes)=row.ok_or(ClientError::NotFound)?;
                if s=="stored" || s=="refused" || retry.is_some_and(|t|t>now){return Ok(None);}
                if attempts>=10{
                    t.execute("UPDATE outbox_destinations SET state='refused',last_error='retry budget exhausted' WHERE event_id=?1 AND relay=?2",params![id.to_string(),relay])?;t.commit()?;return Ok(None);
                }
                let event=parse_and_verify(&bytes,ParseLimits{max_bytes:bytes.len(),max_depth:128})?;
                t.execute("UPDATE outbox_destinations SET state='retry_at',retry_at=?3,attempts=attempts+1 WHERE event_id=?1 AND relay=?2",params![id.to_string(),relay,now.saturating_add(20)])?;
                t.commit()?;Ok(Some((event,attempts+1)))
            }).await?;
            let Some((event, attempts)) = event else {
                continue;
            };
            let result = RelayTransport::new(&destination.relay)?
                .submit(&event)
                .await;
            let now = self.clock.now_unix();
            let relay = destination.relay;
            self.db.call(move|c|{
                match result{
                    Ok(receipt)=>{c.execute("UPDATE outbox_destinations SET state='stored',retain_until=?3,retry_at=NULL,last_error=NULL WHERE event_id=?1 AND relay=?2",params![id.to_string(),relay,receipt.retain_until])?;}
                    Err(error)=>{
                        let permanent=matches!(error,ClientError::Relay(code) if !matches!(code,ProblemCode::RateLimited|ProblemCode::TemporarilyUnavailable));
                        let wait=if let ClientError::RetryAfter(wait)=error{wait}else{2u64.saturating_pow(attempts.min(10) as u32)};
                        let status=if permanent || attempts>=10{"refused"}else{"retry_at"};
                        c.execute("UPDATE outbox_destinations SET state=?3,retry_at=?4,last_error=?5 WHERE event_id=?1 AND relay=?2",
                            params![id.to_string(),relay,status,now.saturating_add(wait),error.to_string()])?;
                    }
                }Ok(())
            }).await?;
        }
        Ok(())
    }
    pub async fn retry_outbox(&self, profile: ProfileId) -> Result<(), ClientError> {
        for entry in self.outbox(profile).await?.into_iter().take(100) {
            self.retry_event(profile, entry.event_id).await?;
        }
        Ok(())
    }
}
