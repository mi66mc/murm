use crate::StoreError;
use rusqlite::Connection;
use std::{path::Path,time::Duration};
mod migrations;

pub struct RelayStore { pub(crate) connection:Connection }
impl RelayStore {
    pub fn open(path:&Path)->Result<Self,StoreError> {
        let mut connection=Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.pragma_update(None,"foreign_keys",true)?;
        connection.pragma_update(None,"journal_mode","WAL")?;
        connection.pragma_update(None,"synchronous","FULL")?;
        migrations::migrate(&mut connection)?;
        Ok(Self{connection})
    }
    pub fn epoch(&self)->Result<String,StoreError> {
        Ok(self.connection.query_row("SELECT value FROM relay_meta WHERE key='epoch'",[],|r|r.get(0))?)
    }
}
