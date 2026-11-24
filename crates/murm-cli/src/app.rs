use crate::{ClientError, storage::DbWorker};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct ClientService {
    pub(crate) db: DbWorker,
    pub(crate) path: PathBuf,
    pub(crate) vault: std::sync::Arc<std::sync::Mutex<crate::vault::VaultState>>,
    pub(crate) events: tokio::sync::broadcast::Sender<crate::vault::ClientEvent>,
    pub(crate) clock: std::sync::Arc<dyn ClientClock>,
}
impl ClientService {
    pub const DEFAULT_DATABASE_BYTES: u64 = 2147483648;
    pub fn open(data_dir: &Path) -> Result<Self, ClientError> {
        Self::open_db(&data_dir.join("client.sqlite"))
    }
    pub fn open_with_capacity(data_dir: &Path, bytes: u64) -> Result<Self, ClientError> {
        Self::open_db_with_capacity(&data_dir.join("client.sqlite"), bytes)
    }
    pub fn open_db(path: &Path) -> Result<Self, ClientError> {
        Self::open_db_with_capacity(path, Self::DEFAULT_DATABASE_BYTES)
    }
    pub fn open_db_with_capacity(path: &Path, bytes: u64) -> Result<Self, ClientError> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let (events, _) = tokio::sync::broadcast::channel(64);
        Ok(Self {
            db: DbWorker::open(path.to_owned(), bytes)?,
            path: path.to_owned(),
            vault: Default::default(),
            events,
            clock: std::sync::Arc::new(SystemClock),
        })
    }
    pub fn database_path(&self) -> &Path {
        &self.path
    }
    pub fn with_clock(mut self, clock: std::sync::Arc<dyn ClientClock>) -> Self {
        self.clock = clock;
        self
    }
}
pub trait ClientClock: Send + Sync {
    fn now_unix(&self) -> u64;
}
struct SystemClock;
impl ClientClock for SystemClock {
    fn now_unix(&self) -> u64 {
        crate::sync::now()
    }
}
