use crate::{RelayStore, StoreError};
use std::path::PathBuf;
use tokio::sync::{mpsc, oneshot};
type Work = Box<dyn FnOnce(&mut RelayStore) + Send>;
#[derive(Clone)]
pub struct DbWorker {
    sender: mpsc::Sender<Work>,
}
impl DbWorker {
    pub fn open(path: PathBuf) -> Result<Self, StoreError> {
        Self::open_with_capacity(path, 536870912)
    }
    pub fn open_with_capacity(path: PathBuf, bytes: u64) -> Result<Self, StoreError> {
        let mut store = RelayStore::open(&path)?;
        store.configure_capacity(bytes)?;
        let (sender, mut receiver) = mpsc::channel::<Work>(64);
        std::thread::Builder::new()
            .name("murm-relay-sqlite".into())
            .spawn(move || {
                while let Some(work) = receiver.blocking_recv() {
                    work(&mut store);
                }
            })?;
        Ok(Self { sender })
    }
    pub async fn call<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut RelayStore) -> Result<T, StoreError> + Send + 'static,
    ) -> Result<T, StoreError> {
        let (send, recv) = oneshot::channel();
        self.sender
            .send(Box::new(move |store| {
                let _ = send.send(f(store));
            }))
            .await
            .map_err(|_| StoreError::Unavailable)?;
        recv.await.map_err(|_| StoreError::Unavailable)?
    }
}
