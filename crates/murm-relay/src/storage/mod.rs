use crate::StoreError;
use rusqlite::Connection;
use std::{path::Path, time::Duration};
pub(crate) mod events;
mod migrations;
mod query;

pub struct RelayStore {
    pub(crate) connection: Connection,
    capacity: Option<u64>,
}
impl RelayStore {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.pragma_update(None, "foreign_keys", true)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        migrations::migrate(&mut connection)?;
        Ok(Self {
            connection,
            capacity: None,
        })
    }
    pub(crate) fn configure_capacity(&mut self, bytes: u64) -> Result<(), StoreError> {
        if self.capacity == Some(bytes) {
            return Ok(());
        }
        let page_size: u64 = self
            .connection
            .pragma_query_value(None, "page_size", |r| r.get(0))?;
        let pages = bytes / page_size;
        if pages == 0 {
            return Err(StoreError::Config);
        }
        self.capacity = None;
        self.connection
            .pragma_update(None, "max_page_count", pages)?;
        let actual: u64 = self
            .connection
            .pragma_query_value(None, "max_page_count", |r| r.get(0))?;
        if actual > pages {
            return Err(StoreError::Config);
        }
        self.capacity = Some(bytes);
        Ok(())
    }
    pub fn epoch(&self) -> Result<String, StoreError> {
        Ok(self.connection.query_row(
            "SELECT value FROM relay_meta WHERE key='epoch'",
            [],
            |r| r.get(0),
        )?)
    }
}
