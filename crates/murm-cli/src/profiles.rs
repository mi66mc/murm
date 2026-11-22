use crate::{ClientService,ClientError};
use murm_protocol::{SignerKey,validate_signer_key};
use rusqlite::{params,OptionalExtension,Connection};
use serde::{Serialize,Deserialize};
pub type ProfileId=uuid::Uuid;
#[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="snake_case")]
pub enum ProfileMode{ReadOnly,Signing}
#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct ProfileSummary {
    pub id:ProfileId,pub name:String,pub public_key:SignerKey,pub mode:ProfileMode,pub locked:bool,pub relays:Vec<String>,
}
pub enum ProfileInput{
    Generate,
    ImportPublic(SignerKey),
    ImportSecret{seed:crate::SecretSeed,public_key:Option<SignerKey>},
}
pub(crate) fn validate_name(name:&str)->Result<(),ClientError>{
    if name.is_empty() || name.chars().count()>80 || name.chars().any(char::is_control){return Err(ClientError::InvalidInput);}Ok(())
}
pub(crate) fn get_profile(c:&Connection,id:ProfileId)->Result<ProfileSummary,ClientError>{
    let (name,public,mode):(String,String,i64)=c.query_row("SELECT name,public_key,mode FROM profiles WHERE id=?1",[id.to_string()],
        |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?.ok_or(ClientError::NotFound)?;
    let relays=c.prepare("SELECT url FROM profile_relays WHERE profile_id=?1 ORDER BY url")?.query_map([id.to_string()],|r|r.get(0))?.collect::<Result<Vec<_>,_>>()?;
    Ok(ProfileSummary{id,name,public_key:public.parse().map_err(|_|ClientError::InvalidKey)?,
        mode:if mode==1{ProfileMode::Signing}else{ProfileMode::ReadOnly},locked:mode==1,relays})
}
impl ClientService {
    pub async fn list_profiles(&self)->Result<Vec<ProfileSummary>,ClientError>{
        let mut profiles=self.db.call(|c|{
            let ids=c.prepare("SELECT id FROM profiles ORDER BY name,id")?.query_map([],|r|r.get::<_,String>(0))?.collect::<Result<Vec<_>,_>>()?;
            ids.into_iter().map(|s|get_profile(c,s.parse().map_err(|_|ClientError::InvalidInput)?)).collect::<Result<Vec<_>,ClientError>>()
        }).await?;
        let unlocked=self.is_unlocked();
        for p in &mut profiles {p.locked=p.mode==ProfileMode::Signing && !unlocked;}
        Ok(profiles)
    }
    pub async fn profile(&self,id:ProfileId)->Result<ProfileSummary,ClientError>{
        let mut p=self.db.call(move|c|get_profile(c,id)).await?;
        p.locked=p.mode==ProfileMode::Signing && !self.is_unlocked();Ok(p)
    }
    pub async fn active_profile(&self)->Result<Option<ProfileSummary>,ClientError>{
        let mut profile=self.db.call(|c|{
            let id:Option<String>=c.query_row("SELECT value FROM client_meta WHERE key='active_profile'",[],|r|r.get(0)).optional()?;
            id.map(|id|get_profile(c,id.parse().map_err(|_|ClientError::InvalidInput)?)).transpose()
        }).await?;
        if let Some(p)=&mut profile {p.locked=p.mode==ProfileMode::Signing && !self.is_unlocked();}
        Ok(profile)
    }
    pub async fn create_profile(&self,name:&str,input:ProfileInput)->Result<ProfileSummary,ClientError>{
        validate_name(name)?;let name=name.to_owned();
        let key=match input {
            ProfileInput::Generate=>{
                let mut seed=zeroize::Zeroizing::new([0u8;32]);
                getrandom::fill(seed.as_mut_slice()).map_err(|_|ClientError::Io)?;
                return self.create_signing_profile(name,crate::SecretSeed(seed),None).await;
            }
            ProfileInput::ImportSecret{seed,public_key}=>return self.create_signing_profile(name,seed,public_key).await,
            ProfileInput::ImportPublic(key)=>key,
        };
        validate_signer_key(&key).map_err(|_|ClientError::InvalidKey)?;
        self.db.call(move|c|{
            let t=c.transaction()?;
            let old:Option<String>=t.query_row("SELECT id FROM profiles WHERE public_key=?1",[key.to_string()],|r|r.get(0)).optional()?;
            let id=if let Some(old)=old{old.parse().map_err(|_|ClientError::InvalidInput)?}else{
                let id=ProfileId::new_v4();t.execute("INSERT INTO profiles(id,name,public_key,mode) VALUES(?1,?2,?3,0)",params![id.to_string(),name,key.to_string()])?;id
            };
            t.commit()?;get_profile(c,id)
        }).await
    }
    pub async fn select_profile(&self,id:ProfileId)->Result<(),ClientError>{
        self.db.call(move|c|{
            get_profile(c,id)?;
            c.execute("INSERT INTO client_meta(key,value) VALUES('active_profile',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[id.to_string()])?;
            Ok(())
        }).await
    }
    pub async fn rename_profile(&self,id:ProfileId,name:&str)->Result<(),ClientError>{
        validate_name(name)?;let name=name.to_owned();
        self.db.call(move|c|{
            if c.execute("UPDATE profiles SET name=?2 WHERE id=?1",params![id.to_string(),name])?==0{return Err(ClientError::NotFound);}Ok(())
        }).await
    }
    pub async fn remove_profile(&self,id:ProfileId)->Result<(),ClientError>{
        self.db.call(move|c|{
            let t=c.transaction()?;
            t.execute("DELETE FROM client_meta WHERE key='active_profile' AND value=?1",[id.to_string()])?;
            if t.execute("DELETE FROM profiles WHERE id=?1",[id.to_string()])?==0{return Err(ClientError::NotFound);}
            t.commit()?;Ok(())
        }).await
    }
}
