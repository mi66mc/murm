use crate::*;
use crate::{profiles::get_profile, sync::cache_event, vault::sign_on_connection};
use murm_protocol::*;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
pub type DraftId = uuid::Uuid;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Draft {
    pub id: DraftId,
    pub profile_id: ProfileId,
    pub publication: String,
    pub header: Map<String, Value>,
    pub content: Value,
    pub version: u64,
    pub queued_event_id: Option<EventId>,
}
/// Bounded list DTO: never loads the article source into a UI snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftSummary {
    pub id: DraftId,
    pub profile_id: ProfileId,
    pub publication: String,
    pub title: String,
    pub revision: Option<u64>,
    pub status: String,
    pub version: u64,
    pub queued_event_id: Option<EventId>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticleSummary {
    pub id: EventId,
    pub signer: SignerKey,
    pub publication: String,
    pub revision: u64,
    pub status: PublicationStatus,
    pub title: String,
    pub created_at: u64,
    pub conflicts: bool,
}
fn load(c: &Connection, profile: ProfileId, id: DraftId) -> Result<Draft, ClientError> {
    let row:Option<(String,String,String,u64,Option<String>)>=c.query_row(
        "SELECT publication,header,content,version,queued_event_id FROM drafts WHERE id=?1 AND profile_id=?2",params![id.to_string(),profile.to_string()],
        |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
    let (publication, header, content, version, queued) = row.ok_or(ClientError::NotFound)?;
    Ok(Draft {
        id,
        profile_id: profile,
        publication,
        header: serde_json::from_str(&header).map_err(|_| ClientError::InvalidInput)?,
        content: serde_json::from_str(&content).map_err(|_| ClientError::InvalidInput)?,
        version,
        queued_event_id: queued
            .map(|v| v.parse().map_err(ClientError::from))
            .transpose()?,
    })
}
fn insert(c: &Connection, draft: &Draft) -> Result<(), ClientError> {
    c.execute("INSERT INTO drafts(id,profile_id,publication,header,content,version,queued_event_id) VALUES(?1,?2,?3,?4,?5,?6,NULL)",
        params![draft.id.to_string(),draft.profile_id.to_string(),draft.publication,serde_json::to_string(&draft.header).map_err(|_|ClientError::InvalidInput)?,
            serde_json::to_string(&draft.content).map_err(|_|ClientError::InvalidInput)?,draft.version])?;
    Ok(())
}
fn next_revision(c: &Connection, key: SignerKey, publication: &str) -> Result<u64, ClientError> {
    let revision: Option<u64> = c
        .query_row(
            "SELECT revision FROM publication_heads WHERE signer=?1 AND kind=1 AND publication=?2",
            params![key.to_string(), publication],
            |r| r.get(0),
        )
        .optional()?;
    let current = revision.unwrap_or(0);
    if current >= MAX_SAFE_INTEGER {
        return Err(ClientError::Conflict);
    }
    Ok(current + 1)
}
fn summarize(c: &Connection, event: &VerifiedEvent) -> Result<ArticleSummary, ClientError> {
    let meta = validate_publication(event)?;
    let conflicts:u64=c.query_row("SELECT COUNT(*) FROM verified_events WHERE signer=?1 AND kind=1 AND json_extract(CAST(json AS TEXT),'$.header.publication')=?2 AND json_extract(CAST(json AS TEXT),'$.header.revision')=?3",
        params![meta.key.signer.to_string(),meta.key.publication,meta.revision],|r|r.get(0))?;
    Ok(ArticleSummary {
        id: meta.id,
        signer: meta.key.signer,
        publication: meta.key.publication,
        revision: meta.revision,
        status: meta.status,
        title: event
            .event()
            .header
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
        created_at: event.event().created_at,
        conflicts: conflicts > 1,
    })
}
impl ClientService {
    pub async fn list_draft_summaries(
        &self,
        profile: ProfileId,
    ) -> Result<Vec<DraftSummary>, ClientError> {
        self.db.call(move|c|{
            let mut statement=c.prepare("SELECT id,publication,
                substr(CAST(COALESCE(json_extract(header,'$.title'),'') AS TEXT),1,200),
                json_extract(header,'$.revision'),substr(CAST(COALESCE(json_extract(header,'$.status'),'') AS TEXT),1,16),
                version,queued_event_id FROM drafts WHERE profile_id=?1 ORDER BY rowid DESC LIMIT 1000")?;
            let rows=statement.query_map([profile.to_string()],|r|Ok((
                r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,
                r.get::<_,rusqlite::types::Value>(3)?,r.get::<_,String>(4)?,r.get::<_,u64>(5)?,r.get::<_,Option<String>>(6)?)))?;
            let mut summaries=Vec::new();
            for row in rows{
                let (id,publication,title,number,status,version,queued)=row?;
                let revision=match number{
                    rusqlite::types::Value::Integer(n) if n>0 && n as u64<=MAX_SAFE_INTEGER=>Some(n as u64),
                    rusqlite::types::Value::Real(n) if n>0.0 && n.fract()==0.0 && n<=MAX_SAFE_INTEGER as f64=>Some(n as u64),
                    _=>None,
                };
                summaries.push(DraftSummary{id:id.parse().map_err(|_|ClientError::InvalidInput)?,profile_id:profile,publication,title,revision,status,version,
                    queued_event_id:queued.map(|s|s.parse().map_err(ClientError::from)).transpose()?});
            }Ok(summaries)
        }).await
    }
    pub async fn create_draft(
        &self,
        profile: ProfileId,
        title: &str,
        source: &str,
    ) -> Result<Draft, ClientError> {
        if title.chars().count() > 200 || source.len() > 4194304 {
            return Err(ClientError::InvalidInput);
        }
        let publication = uuid::Uuid::new_v4().to_string();
        let draft = Draft {
            id: DraftId::new_v4(),
            profile_id: profile,
            publication: publication.clone(),
            header:
                json!({"publication":publication,"revision":1,"status":"published","title":title})
                    .as_object()
                    .ok_or(ClientError::InvalidInput)?
                    .clone(),
            content: json!(source),
            version: 1,
            queued_event_id: None,
        };
        self.db
            .call(move |c| {
                get_profile(c, profile)?;
                insert(c, &draft)?;
                Ok(draft)
            })
            .await
    }
    pub async fn load_draft(&self, profile: ProfileId, id: DraftId) -> Result<Draft, ClientError> {
        self.db.call(move |c| load(c, profile, id)).await
    }
    pub async fn list_drafts(&self, profile: ProfileId) -> Result<Vec<Draft>, ClientError> {
        self.db
            .call(move |c| {
                let ids = c
                    .prepare(
                        "SELECT id FROM drafts WHERE profile_id=?1 ORDER BY rowid DESC LIMIT 1000",
                    )?
                    .query_map([profile.to_string()], |r| r.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                ids.into_iter()
                    .map(|id| {
                        load(
                            c,
                            profile,
                            id.parse().map_err(|_| ClientError::InvalidInput)?,
                        )
                    })
                    .collect()
            })
            .await
    }
    pub async fn save_draft(
        &self,
        profile: ProfileId,
        mut draft: Draft,
    ) -> Result<Draft, ClientError> {
        if profile != draft.profile_id || draft.content.as_str().is_some_and(|s| s.len() > 4194304)
        {
            return Err(ClientError::InvalidInput);
        }
        self.db.call(move|c|{
            let t=c.transaction()?;let old=load(&t,profile,draft.id)?;
            if old.version!=draft.version || old.publication!=draft.publication{return Err(ClientError::Conflict);}
            if old.header==draft.header && old.content==draft.content{t.commit()?;return Ok(old);}
            if old.queued_event_id.is_some(){
                let p=get_profile(&t,profile)?;draft.header.insert("revision".into(),json!(next_revision(&t,p.public_key,&draft.publication)?));
            }
            draft.header.insert("publication".into(),json!(draft.publication));draft.queued_event_id=None;
            draft.version=draft.version.checked_add(1).filter(|n|*n<=MAX_SAFE_INTEGER).ok_or(ClientError::Conflict)?;
            t.execute("UPDATE drafts SET header=?3,content=?4,version=?5,queued_event_id=NULL WHERE id=?1 AND profile_id=?2",
                params![draft.id.to_string(),profile.to_string(),serde_json::to_string(&draft.header).map_err(|_|ClientError::InvalidInput)?,
                    serde_json::to_string(&draft.content).map_err(|_|ClientError::InvalidInput)?,draft.version])?;
            t.commit()?;Ok(draft)
        }).await
    }
    pub async fn edit_publication(
        &self,
        profile: ProfileId,
        publication: &str,
    ) -> Result<Draft, ClientError> {
        let publication = publication.to_owned();
        self.db.call(move|c|{
            let p=get_profile(c,profile)?;
            let bytes:Vec<u8>=c.query_row("SELECT e.json FROM publication_heads h JOIN verified_events e ON e.id=h.id WHERE h.signer=?1 AND h.kind=1 AND h.publication=?2",
                params![p.public_key.to_string(),publication],|r|r.get(0)).optional()?.ok_or(ClientError::NotFound)?;
            let event=parse_and_verify(&bytes,ParseLimits{max_bytes:bytes.len(),max_depth:128})?;
            let mut header=event.event().header.clone();header.insert("revision".into(),json!(next_revision(c,p.public_key,&publication)?));
            header.insert("status".into(),json!("published"));
            let content=if event.event().content.is_string(){event.event().content.clone()}else{json!("")};
            let draft=Draft{id:DraftId::new_v4(),profile_id:profile,publication,header,content,version:1,queued_event_id:None};
            insert(c,&draft)?;Ok(draft)
        }).await
    }
    /// Persist a signed immutable event and all destinations before any network send.
    pub async fn publish(&self, profile: ProfileId, id: DraftId) -> Result<EventId, ClientError> {
        let vault = self.vault.clone();
        let now = self.clock.now_unix();
        self.db.call(move|c|{
            let t=c.transaction()?;let p=get_profile(&t,profile)?;let draft=load(&t,profile,id)?;
            if p.mode!=ProfileMode::Signing{return Err(ClientError::ReadOnly);}
            if let Some(id)=draft.queued_event_id{return Ok(id);}
            let revision=draft.header.get("revision").and_then(Value::as_u64).ok_or(ClientError::InvalidInput)?;
            if revision!=next_revision(&t,p.public_key,&draft.publication)?{return Err(ClientError::Conflict);}
            let unsigned=UnsignedEvent{version:1,signer:p.public_key,created_at:now,kind:1,header:draft.header,content:draft.content};
            let signed=sign_on_connection(&t,&vault,profile,&unsigned)?;validate_publication(&signed)?;
            if p.relays.is_empty(){return Err(ClientError::InvalidInput);}
            let event_id=signed.event().id;let bytes=serde_json::to_vec(&signed).map_err(|_|ClientError::InvalidInput)?;
            cache_event(&t,&signed)?;
            t.execute("INSERT INTO outbox(event_id,profile_id,json,created_at) VALUES(?1,?2,?3,?4)",params![event_id.to_string(),profile.to_string(),bytes,now])?;
            for relay in p.relays{t.execute("INSERT INTO outbox_destinations(event_id,relay,state) VALUES(?1,?2,'pending')",params![event_id.to_string(),relay])?;}
            t.execute("UPDATE drafts SET queued_event_id=?3,version=version+1 WHERE id=?1 AND profile_id=?2",params![id.to_string(),profile.to_string(),event_id.to_string()])?;
            t.commit()?;Ok(event_id)
        }).await
    }
    pub async fn withdraw(
        &self,
        profile: ProfileId,
        publication: &str,
    ) -> Result<EventId, ClientError> {
        let mut draft = self.edit_publication(profile, publication).await?;
        draft.header.insert("status".into(), json!("withdrawn"));
        draft.content = Value::Null;
        let draft = self.save_draft(profile, draft).await?;
        self.publish(profile, draft.id).await
    }
    pub async fn restore(
        &self,
        profile: ProfileId,
        publication: &str,
        source: &str,
    ) -> Result<EventId, ClientError> {
        let mut draft = self.edit_publication(profile, publication).await?;
        draft.content = json!(source);
        let draft = self.save_draft(profile, draft).await?;
        self.publish(profile, draft.id).await
    }
    pub async fn list_articles(
        &self,
        include_withdrawn: bool,
    ) -> Result<Vec<ArticleSummary>, ClientError> {
        self.db.call(move|c|{
            let sql=if include_withdrawn{"SELECT e.json FROM publication_heads h JOIN verified_events e ON e.id=h.id ORDER BY e.created_at DESC,e.id DESC LIMIT 1000"}
                else{"SELECT e.json FROM publication_heads h JOIN verified_events e ON e.id=h.id WHERE h.status='published' ORDER BY e.created_at DESC,e.id DESC LIMIT 1000"};
            let mut statement=c.prepare(sql)?;let mut rows=statement.query([])?;
            let mut result=Vec::new();
            while let Some(row)=rows.next()?{
                let bytes:Vec<u8>=row.get(0)?;
                let event=parse_and_verify(&bytes,ParseLimits{max_bytes:bytes.len(),max_depth:128})?;
                result.push(summarize(c,&event)?);
            }
            Ok(result)
        }).await
    }
    pub async fn article_history(
        &self,
        signer: SignerKey,
        publication: &str,
    ) -> Result<Vec<VerifiedEvent>, ClientError> {
        let publication = publication.to_owned();
        self.db.call(move|c|{
            let bytes=c.prepare("SELECT json FROM verified_events WHERE signer=?1 AND kind=1 AND json_extract(CAST(json AS TEXT),'$.header.publication')=?2
                ORDER BY json_extract(CAST(json AS TEXT),'$.header.revision') DESC,id DESC LIMIT 1000")?
                .query_map(params![signer.to_string(),publication],|r|r.get::<_,Vec<u8>>(0))?.collect::<Result<Vec<_>,_>>()?;
            bytes.into_iter().map(|b|parse_and_verify(&b,ParseLimits{max_bytes:b.len(),max_depth:128}).map_err(ClientError::from)).collect()
        }).await
    }
    pub async fn article_history_summaries(
        &self,
        signer: SignerKey,
        publication: &str,
    ) -> Result<Vec<ArticleSummary>, ClientError> {
        let publication = publication.to_owned();
        self.db.call(move|c|{
            let mut statement=c.prepare("SELECT json FROM verified_events WHERE signer=?1 AND kind=1 AND json_extract(CAST(json AS TEXT),'$.header.publication')=?2
                ORDER BY json_extract(CAST(json AS TEXT),'$.header.revision') DESC,id DESC LIMIT 1000")?;
            let mut rows=statement.query(params![signer.to_string(),publication])?;let mut summaries=Vec::new();
            while let Some(row)=rows.next()?{
                let bytes:Vec<u8>=row.get(0)?;
                let event=parse_and_verify(&bytes,ParseLimits{max_bytes:bytes.len(),max_depth:128})?;
                summaries.push(summarize(c,&event)?);
            }Ok(summaries)
        }).await
    }
}
