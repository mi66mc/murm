use crate::StoreError;
use rusqlite::Connection;
pub fn migrate(c:&mut Connection)->Result<(),StoreError> {
    let version:i64=c.pragma_query_value(None,"user_version",|r|r.get(0))?;
    if version>1 {return Err(StoreError::Config);}
    if version==1 {return Ok(());}
    let t=c.transaction()?;
    t.execute_batch("
        CREATE TABLE relay_meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
        CREATE TABLE events(
            admission_seq INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT NOT NULL UNIQUE,
            json BLOB NOT NULL,signer TEXT NOT NULL,kind INTEGER NOT NULL,created_at INTEGER NOT NULL,
            admitted_at INTEGER NOT NULL,retain_until INTEGER,max_until INTEGER,early_removal TEXT NOT NULL);
        CREATE INDEX events_order ON events(created_at DESC,id DESC);
        CREATE INDEX events_signer_kind ON events(signer,kind);
        CREATE TABLE publication_heads(
            signer TEXT NOT NULL,kind INTEGER NOT NULL,publication TEXT NOT NULL,
            revision INTEGER NOT NULL,id TEXT NOT NULL,status TEXT NOT NULL,
            PRIMARY KEY(signer,kind,publication));
        CREATE TABLE propagation_jobs(
            event_id TEXT NOT NULL,peer TEXT NOT NULL,attempts INTEGER NOT NULL DEFAULT 0,
            next_try INTEGER NOT NULL,deadline INTEGER NOT NULL,
            PRIMARY KEY(event_id,peer),FOREIGN KEY(event_id) REFERENCES events(id) ON DELETE CASCADE);
        CREATE TABLE propagation_budgets(event_id TEXT PRIMARY KEY,attempts INTEGER NOT NULL,deadline INTEGER NOT NULL);
        CREATE TABLE seen_ids(id TEXT PRIMARY KEY,expires_at INTEGER NOT NULL);
        CREATE TABLE peer_sync(peer TEXT NOT NULL,filter_hash TEXT NOT NULL,cursor TEXT,
            PRIMARY KEY(peer,filter_hash));
        CREATE TABLE sync_rejections(peer TEXT NOT NULL,event_id TEXT NOT NULL,code TEXT NOT NULL,
            PRIMARY KEY(peer,event_id));
        CREATE TABLE query_snapshots(id TEXT PRIMARY KEY,query_hash TEXT NOT NULL,ids TEXT NOT NULL,
            event_count INTEGER NOT NULL,expires_at INTEGER NOT NULL);
        PRAGMA user_version=1;
    ")?;
    let mut secret=[0u8;32];getrandom::fill(&mut secret).map_err(|_|StoreError::Unavailable)?;
    t.execute("INSERT INTO relay_meta(key,value) VALUES('epoch',?1)",[uuid::Uuid::new_v4().to_string()])?;
    t.execute("INSERT INTO relay_meta(key,value) VALUES('cursor_secret',?1)",[hex::encode(secret)])?;
    t.commit()?;Ok(())
}
