use crate::ClientError;
use rusqlite::Connection;
pub fn migrate(c: &mut Connection) -> Result<(), ClientError> {
    let version: i64 = c.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version > 1 {
        return Err(ClientError::InvalidInput);
    }
    if version == 1 {
        return Ok(());
    }
    let t = c.transaction()?;
    t.execute_batch("
        CREATE TABLE client_meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
        CREATE TABLE profiles(id TEXT PRIMARY KEY,name TEXT NOT NULL,public_key TEXT NOT NULL UNIQUE,
            mode INTEGER NOT NULL CHECK(mode IN(0,1)));
        CREATE TABLE vault(id INTEGER PRIMARY KEY CHECK(id=1),version INTEGER NOT NULL,params TEXT NOT NULL,
            salt BLOB NOT NULL,nonce BLOB NOT NULL,ciphertext BLOB NOT NULL);
        CREATE TABLE profile_secrets(profile_id TEXT PRIMARY KEY REFERENCES profiles(id) ON DELETE CASCADE,
            nonce BLOB NOT NULL,ciphertext BLOB NOT NULL);
        CREATE TABLE profile_relays(profile_id TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
            url TEXT NOT NULL,info TEXT,last_error TEXT,PRIMARY KEY(profile_id,url));
        CREATE TABLE drafts(id TEXT PRIMARY KEY,profile_id TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
            publication TEXT NOT NULL,header TEXT NOT NULL,content TEXT NOT NULL,version INTEGER NOT NULL,queued_event_id TEXT);
        CREATE TABLE verified_events(id TEXT PRIMARY KEY,json BLOB NOT NULL,signer TEXT NOT NULL,kind INTEGER NOT NULL,created_at INTEGER NOT NULL);
        CREATE INDEX verified_events_author ON verified_events(signer,kind,created_at DESC,id DESC);
        CREATE TABLE publication_heads(signer TEXT NOT NULL,kind INTEGER NOT NULL,publication TEXT NOT NULL,
            revision INTEGER NOT NULL,id TEXT NOT NULL,status TEXT NOT NULL,PRIMARY KEY(signer,kind,publication));
        CREATE TABLE sync_cursors(profile_id TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
            relay TEXT NOT NULL,filter_hash TEXT NOT NULL,cursor TEXT NOT NULL,PRIMARY KEY(profile_id,relay,filter_hash));
        CREATE TABLE sync_rejections(profile_id TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
            relay TEXT NOT NULL,event_id TEXT NOT NULL,code TEXT NOT NULL,PRIMARY KEY(profile_id,relay,event_id));
        CREATE TABLE relay_backoff(profile_id TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
            relay TEXT NOT NULL,retry_at INTEGER NOT NULL,PRIMARY KEY(profile_id,relay));
        CREATE TABLE outbox(event_id TEXT PRIMARY KEY,profile_id TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
            json BLOB NOT NULL,created_at INTEGER NOT NULL);
        CREATE TABLE outbox_destinations(event_id TEXT NOT NULL REFERENCES outbox(event_id) ON DELETE CASCADE,
            relay TEXT NOT NULL,state TEXT NOT NULL,retain_until INTEGER,retry_at INTEGER,attempts INTEGER NOT NULL DEFAULT 0,last_error TEXT,
            PRIMARY KEY(event_id,relay));
        PRAGMA user_version=1;
    ")?;
    t.commit()?;
    Ok(())
}
