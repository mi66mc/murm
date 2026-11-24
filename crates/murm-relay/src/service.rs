use crate::*;
use murm_protocol::*;
use std::sync::Arc;
pub struct RelayService {
    pub config: Arc<RelayConfig>,
    pub(crate) db: DbWorker,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) slots: Arc<tokio::sync::Semaphore>,
    pub(crate) rates: std::sync::Mutex<crate::http::Rates>,
}
impl RelayService {
    pub fn open(config: RelayConfig, clock: Arc<dyn Clock>) -> Result<Self, StoreError> {
        config.validate()?;
        let db =
            DbWorker::open_with_capacity(config.database_path.clone(), config.max_storage_bytes)?;
        Ok(Self {
            config: Arc::new(config),
            db,
            clock,
            slots: Arc::new(tokio::sync::Semaphore::new(32)),
            rates: Default::default(),
        })
    }
    pub async fn submit(&self, bytes: Vec<u8>) -> Result<StoreReceipt, StoreError> {
        let config = self.config.clone();
        let event = tokio::task::spawn_blocking(move || {
            if bytes.len() as u64 > config.info.limits.event_bytes {
                return Err(ProtocolError::ContentTooLarge);
            }
            let event = parse_and_verify(&bytes, config.info.parse_limits())?;
            if bytes.len() as u64 > config.info.effective_policy(event.event().kind).event_bytes {
                return Err(ProtocolError::ContentTooLarge);
            }
            Ok(event)
        })
        .await
        .map_err(|_| StoreError::Unavailable)??;
        let c = self.config.clone();
        let now = self.clock.now_unix();
        self.db
            .call(move |store| store.admit(&event, &c, now))
            .await
    }
    pub async fn fetch(&self, id: EventId) -> Result<VerifiedEvent, StoreError> {
        let now = self.clock.now_unix();
        self.db
            .call(move |store| {
                store
                    .fetch(id, now)?
                    .ok_or(StoreError::Rejected(ProblemCode::NotFound))
            })
            .await
    }
    pub async fn query(&self, q: EventQuery) -> Result<QueryResponse, StoreError> {
        let c = self.config.clone();
        let now = self.clock.now_unix();
        self.db.call(move |store| store.query(&q, &c, now)).await
    }
    pub async fn sync(&self, q: SyncRequest) -> Result<SyncResponse, StoreError> {
        let c = self.config.clone();
        let now = self.clock.now_unix();
        self.db.call(move |store| store.sync(&q, &c, now)).await
    }
}
