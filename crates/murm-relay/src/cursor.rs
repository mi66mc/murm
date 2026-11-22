use crate::{RelayStore,StoreError};
use base64::{Engine,engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac,Mac};
use sha2::{Sha256,Digest};
use serde::{Deserialize,Serialize};
use murm_protocol::ProblemCode;

#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cursor {
    pub version:u8,pub mode:String,pub epoch:String,pub hash:String,pub expires:u64,
    pub time:u64,pub id:String,pub seq:u64,pub snapshot:Option<String>,pub position:usize,
}
pub(crate) fn hash(value:&impl Serialize)->Result<String,StoreError> {
    Ok(hex::encode(Sha256::digest(serde_jcs::to_vec(value).map_err(|_|StoreError::Unavailable)?)))
}
impl RelayStore {
    fn cursor_secret(&self)->Result<Vec<u8>,StoreError> {
        let s:String=self.connection.query_row("SELECT value FROM relay_meta WHERE key='cursor_secret'",[],|r|r.get(0))?;
        hex::decode(s).map_err(|_|StoreError::Unavailable)
    }
    pub(crate) fn token(&self,cursor:&Cursor,max:u64)->Result<String,StoreError> {
        let bytes=serde_json::to_vec(cursor).map_err(|_|StoreError::Unavailable)?;
        let mut mac=Hmac::<Sha256>::new_from_slice(&self.cursor_secret()?).map_err(|_|StoreError::Unavailable)?;
        mac.update(&bytes);
        let token=format!("{}.{}",URL_SAFE_NO_PAD.encode(&bytes),URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()));
        if token.len() as u64>max {return Err(StoreError::Rejected(ProblemCode::LimitExceeded));}
        Ok(token)
    }
    pub(crate) fn decode_cursor(&self,token:&str,mode:&str,hash:&str,now:u64,max:u64)->Result<Cursor,StoreError> {
        let bad=||StoreError::Rejected(ProblemCode::InvalidCursor);
        if token.len() as u64>max {return Err(bad());}
        let (body,tag)=token.split_once('.').ok_or_else(bad)?;
        let bytes=URL_SAFE_NO_PAD.decode(body).map_err(|_|bad())?;
        let tag=URL_SAFE_NO_PAD.decode(tag).map_err(|_|bad())?;
        let mut mac=Hmac::<Sha256>::new_from_slice(&self.cursor_secret()?).map_err(|_|StoreError::Unavailable)?;
        mac.update(&bytes);mac.verify_slice(&tag).map_err(|_|bad())?;
        let c:Cursor=serde_json::from_slice(&bytes).map_err(|_|bad())?;
        if c.version!=1 || c.mode!=mode || c.hash!=hash || c.epoch!=self.epoch()? {return Err(bad());}
        if c.expires<=now {return Err(StoreError::Rejected(ProblemCode::CursorExpired));}
        Ok(c)
    }
    pub(crate) fn new_cursor(&self,mode:&str,hash:String,now:u64,ttl:u64)->Result<Cursor,StoreError> {
        Ok(Cursor{version:1,mode:mode.into(),epoch:self.epoch()?,hash,expires:now.saturating_add(ttl),
            time:0,id:String::new(),seq:0,snapshot:None,position:0})
    }
}
