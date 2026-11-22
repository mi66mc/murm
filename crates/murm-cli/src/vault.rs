use crate::profiles::get_profile;
use crate::{ClientError, ClientService, ProfileId, ProfileMode, ProfileSummary};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use murm_protocol::*;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, OnceLock};
use zeroize::Zeroizing;

pub struct SecretString(pub(crate) Zeroizing<String>);
impl SecretString {
    pub fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }
}
pub struct SecretSeed(pub(crate) Zeroizing<[u8; 32]>);
impl SecretSeed {
    pub fn from_hex(value: &str) -> Result<Self, ClientError> {
        if value.len() != 64 {
            return Err(ClientError::InvalidKey);
        }
        let mut bytes = Zeroizing::new([0; 32]);
        hex::decode_to_slice(value, bytes.as_mut_slice()).map_err(|_| ClientError::InvalidKey)?;
        Ok(Self(bytes))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VaultActionId(uuid::Uuid);
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct VaultStatus {
    pub exists: bool,
    pub unlocked: bool,
}
#[derive(Debug, Clone)]
pub enum ClientEvent {
    VaultDerivationStarted(VaultActionId),
    VaultChanged(VaultStatus),
}
#[derive(Default)]
pub(crate) struct VaultState {
    action: Option<VaultActionId>,
    master: Option<Zeroizing<[u8; 32]>>,
}
fn state(m: &Mutex<VaultState>) -> std::sync::MutexGuard<'_, VaultState> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
const AAD: &[u8] = b"murm-client-vault:v1:argon2id19:m65536:t3:p1:l32";
fn parameters() -> String {
    r#"{"algorithm":"argon2id","version":19,"memory_kib":65536,"iterations":3,"parallelism":1,"output_bytes":32}"#.into()
}
#[derive(Clone, PartialEq, Eq)]
struct VaultRow {
    version: i64,
    params: String,
    salt: Vec<u8>,
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
}
fn read_row(c: &Connection) -> Result<Option<VaultRow>, ClientError> {
    Ok(c.query_row(
        "SELECT version,params,salt,nonce,ciphertext FROM vault WHERE id=1",
        [],
        |r| {
            Ok(VaultRow {
                version: r.get(0)?,
                params: r.get(1)?,
                salt: r.get(2)?,
                nonce: r.get(3)?,
                ciphertext: r.get(4)?,
            })
        },
    )
    .optional()?)
}
fn check_row(row: &VaultRow) -> Result<(), ClientError> {
    if row.version != 1
        || row.salt.len() != 16
        || row.nonce.len() != 24
        || row.ciphertext.len() != 48
    {
        return Err(ClientError::CorruptVault);
    }
    let actual = parse_json(
        row.params.as_bytes(),
        ParseLimits {
            max_bytes: 512,
            max_depth: 2,
        },
    )
    .map_err(|_| ClientError::CorruptVault)?;
    let expected: serde_json::Value =
        serde_json::from_str(&parameters()).map_err(|_| ClientError::CorruptVault)?;
    if actual != expected {
        return Err(ClientError::CorruptVault);
    }
    Ok(())
}
fn random<const N: usize>() -> Result<[u8; N], ClientError> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(|_| ClientError::Io)?;
    Ok(bytes)
}
fn encrypt(
    key: &[u8; 32],
    plaintext: &[u8],
    aad: &[u8],
) -> Result<(Vec<u8>, Vec<u8>), ClientError> {
    let nonce = random::<24>()?;
    let cipher = XChaCha20Poly1305::new_from_slice(key).map_err(|_| ClientError::CorruptVault)?;
    let ciphertext = cipher
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| ClientError::CorruptVault)?;
    Ok((nonce.to_vec(), ciphertext))
}
fn decrypt(
    key: &[u8; 32],
    nonce: &[u8],
    ciphertext: &[u8],
    aad: &[u8],
) -> Result<Zeroizing<[u8; 32]>, ClientError> {
    let nonce: [u8; 24] = nonce.try_into().map_err(|_| ClientError::CorruptVault)?;
    if ciphertext.len() != 48 {
        return Err(ClientError::CorruptVault);
    }
    let cipher = XChaCha20Poly1305::new_from_slice(key).map_err(|_| ClientError::CorruptVault)?;
    let bytes = Zeroizing::new(
        cipher
            .decrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: ciphertext,
                    aad,
                },
            )
            .map_err(|_| ClientError::WrongPassword)?,
    );
    let mut output = Zeroizing::new([0; 32]);
    output.copy_from_slice(&bytes);
    Ok(output)
}
fn profile_aad(id: ProfileId, key: SignerKey) -> Vec<u8> {
    format!("murm-client-profile:v1:{id}:{key}").into_bytes()
}
fn ensure_action(v: &VaultState, action: VaultActionId) -> Result<(), ClientError> {
    if v.action != Some(action) {
        return Err(ClientError::Cancelled);
    }
    Ok(())
}
fn check_new_password(password: &SecretString) -> Result<(), ClientError> {
    if password.0.chars().count() < 12 || password.0.len() > 4096 {
        return Err(ClientError::InvalidInput);
    }
    Ok(())
}
async fn derive(
    password: SecretString,
    salt: Vec<u8>,
    events: tokio::sync::broadcast::Sender<ClientEvent>,
    action: VaultActionId,
) -> Result<Zeroizing<[u8; 32]>, ClientError> {
    if password.0.len() > 4096 || salt.len() != 16 {
        return Err(ClientError::InvalidInput);
    }
    static SLOTS: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
    let permit = SLOTS
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(2)))
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| ClientError::Io)?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let _ = events.send(ClientEvent::VaultDerivationStarted(action));
        let mut key = Zeroizing::new([0; 32]);
        let params = Params::new(65536, 3, 1, Some(32)).map_err(|_| ClientError::CorruptVault)?;
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password_into(password.0.as_bytes(), &salt, key.as_mut_slice())
            .map_err(|_| ClientError::CorruptVault)?;
        Ok(key)
    })
    .await
    .map_err(|_| ClientError::Io)?
}
impl ClientService {
    pub fn subscribe_events(&self) -> tokio::sync::broadcast::Receiver<ClientEvent> {
        self.events.subscribe()
    }
    pub fn begin_vault_action(&self) -> VaultActionId {
        let action = VaultActionId(uuid::Uuid::new_v4());
        state(&self.vault).action = Some(action);
        action
    }
    pub fn lock(&self) {
        let mut v = state(&self.vault);
        v.action = None;
        v.master = None;
    }
    pub fn cancel_vault_action(&self, action: VaultActionId) {
        let mut v = state(&self.vault);
        if v.action == Some(action) {
            v.action = None;
            v.master = None;
        }
    }
    pub fn is_unlocked(&self) -> bool {
        state(&self.vault).master.is_some()
    }
    pub async fn vault_status(&self) -> Result<VaultStatus, ClientError> {
        let exists = self.db.call(|c| Ok(read_row(c)?.is_some())).await?;
        Ok(VaultStatus {
            exists,
            unlocked: self.is_unlocked(),
        })
    }
    pub async fn initialize_vault(
        &self,
        password: SecretString,
        action: VaultActionId,
    ) -> Result<VaultStatus, ClientError> {
        check_new_password(&password)?;
        if self.db.call(|c| Ok(read_row(c)?.is_some())).await? {
            return Err(ClientError::Conflict);
        }
        let salt = random::<16>()?.to_vec();
        let key = derive(password, salt.clone(), self.events.clone(), action).await?;
        let master = Zeroizing::new(random::<32>()?);
        let (nonce, ciphertext) = encrypt(&key, &*master, AAD)?;
        let vault = self.vault.clone();
        self.db.call(move|c|{
            let mut v=state(&vault);ensure_action(&v,action)?;
            let t=c.transaction()?;
            if read_row(&t)?.is_some(){return Err(ClientError::Conflict);}
            t.execute("INSERT INTO vault(id,version,params,salt,nonce,ciphertext) VALUES(1,1,?1,?2,?3,?4)",params![parameters(),salt,nonce,ciphertext])?;
            t.commit()?;v.master=Some(master);Ok(())
        }).await?;
        let status = self.vault_status().await?;
        let _ = self.events.send(ClientEvent::VaultChanged(status));
        Ok(status)
    }
    pub async fn unlock(
        &self,
        password: SecretString,
        action: VaultActionId,
    ) -> Result<VaultStatus, ClientError> {
        let row = self
            .db
            .call(|c| read_row(c)?.ok_or(ClientError::VaultNotInitialized))
            .await?;
        check_row(&row)?;
        let key = derive(password, row.salt.clone(), self.events.clone(), action).await?;
        let master = decrypt(&key, &row.nonce, &row.ciphertext, AAD)?;
        let vault = self.vault.clone();
        self.db
            .call(move |c| {
                let mut v = state(&vault);
                ensure_action(&v, action)?;
                if read_row(c)?.as_ref() != Some(&row) {
                    return Err(ClientError::Conflict);
                }
                v.master = Some(master);
                Ok(())
            })
            .await?;
        let status = self.vault_status().await?;
        let _ = self.events.send(ClientEvent::VaultChanged(status));
        Ok(status)
    }
    pub async fn change_password(
        &self,
        current: SecretString,
        new: SecretString,
        action: VaultActionId,
    ) -> Result<(), ClientError> {
        check_new_password(&new)?;
        let row = self
            .db
            .call(|c| read_row(c)?.ok_or(ClientError::VaultNotInitialized))
            .await?;
        check_row(&row)?;
        let key = derive(current, row.salt.clone(), self.events.clone(), action).await?;
        let master = decrypt(&key, &row.nonce, &row.ciphertext, AAD)?;
        let salt = random::<16>()?.to_vec();
        let key = derive(new, salt.clone(), self.events.clone(), action).await?;
        let (nonce, ciphertext) = encrypt(&key, &*master, AAD)?;
        let vault = self.vault.clone();
        self.db
            .call(move |c| {
                let mut v = state(&vault);
                ensure_action(&v, action)?;
                let t = c.transaction()?;
                if read_row(&t)?.as_ref() != Some(&row) {
                    return Err(ClientError::Conflict);
                }
                t.execute(
                    "UPDATE vault SET params=?1,salt=?2,nonce=?3,ciphertext=?4 WHERE id=1",
                    params![parameters(), salt, nonce, ciphertext],
                )?;
                t.commit()?;
                v.master = Some(master);
                Ok(())
            })
            .await
    }
    pub(crate) async fn create_signing_profile(
        &self,
        name: String,
        seed: SecretSeed,
        expected: Option<SignerKey>,
    ) -> Result<ProfileSummary, ClientError> {
        let key = derive_public_key(&seed.0);
        if expected.is_some_and(|k| k != key) {
            return Err(ClientError::KeyMismatch);
        }
        let vault = self.vault.clone();
        let mut profile = self
            .db
            .call(move |c| {
                let v = state(&vault);
                if read_row(c)?.is_none() {
                    return Err(ClientError::VaultNotInitialized);
                }
                let master = v.master.as_ref().ok_or(ClientError::Locked)?;
                let t = c.transaction()?;
                let old: Option<String> = t
                    .query_row(
                        "SELECT id FROM profiles WHERE public_key=?1",
                        [key.to_string()],
                        |r| r.get(0),
                    )
                    .optional()?;
                let id = if let Some(old) = old {
                    old.parse().map_err(|_| ClientError::InvalidInput)?
                } else {
                    let id = ProfileId::new_v4();
                    t.execute(
                        "INSERT INTO profiles(id,name,public_key,mode) VALUES(?1,?2,?3,0)",
                        params![id.to_string(), name, key.to_string()],
                    )?;
                    id
                };
                let exists = t
                    .query_row(
                        "SELECT 1 FROM profile_secrets WHERE profile_id=?1",
                        [id.to_string()],
                        |_| Ok(()),
                    )
                    .optional()?
                    .is_some();
                if !exists {
                    let (nonce, ciphertext) = encrypt(master, &*seed.0, &profile_aad(id, key))?;
                    t.execute(
                        "INSERT INTO profile_secrets(profile_id,nonce,ciphertext) VALUES(?1,?2,?3)",
                        params![id.to_string(), nonce, ciphertext],
                    )?;
                    t.execute("UPDATE profiles SET mode=1 WHERE id=?1", [id.to_string()])?;
                }
                t.commit()?;
                get_profile(c, id)
            })
            .await?;
        profile.locked = !self.is_unlocked();
        Ok(profile)
    }
    pub async fn sign(
        &self,
        profile: ProfileId,
        draft: UnsignedEvent,
    ) -> Result<VerifiedEvent, ClientError> {
        let vault = self.vault.clone();
        self.db
            .call(move |c| sign_on_connection(c, &vault, profile, &draft))
            .await
    }
}
pub(crate) fn sign_on_connection(
    c: &Connection,
    vault: &Mutex<VaultState>,
    profile: ProfileId,
    draft: &UnsignedEvent,
) -> Result<VerifiedEvent, ClientError> {
    let p = get_profile(c, profile)?;
    if p.mode != ProfileMode::Signing {
        return Err(ClientError::ReadOnly);
    }
    if draft.signer != p.public_key {
        return Err(ClientError::KeyMismatch);
    }
    let v = state(vault);
    let master = v.master.as_ref().ok_or(ClientError::Locked)?;
    let (nonce, ciphertext): (Vec<u8>, Vec<u8>) = c.query_row(
        "SELECT nonce,ciphertext FROM profile_secrets WHERE profile_id=?1",
        [profile.to_string()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let seed = decrypt(
        master,
        &nonce,
        &ciphertext,
        &profile_aad(profile, p.public_key),
    )?;
    Ok(sign_event(draft, &seed)?)
}
