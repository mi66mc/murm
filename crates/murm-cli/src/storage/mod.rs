use crate::ClientError;
use rusqlite::Connection;
use std::{fs::{File,OpenOptions},path::PathBuf,sync::Arc,time::Duration};
use tokio::sync::{mpsc,oneshot};
mod migrations;
type Work=Box<dyn FnOnce(&mut Connection)+Send>;
struct Inner {
    sender:Option<mpsc::Sender<Work>>,
    thread:Option<std::thread::JoinHandle<()>>,
}
impl Drop for Inner {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(thread)=self.thread.take() {let _=thread.join();}
    }
}
#[derive(Clone)]
pub(crate) struct DbWorker {inner:Arc<Inner>}
impl DbWorker {
    pub fn open(path:PathBuf)->Result<Self,ClientError> {
        let lock=OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path.with_extension("lock"))?;
        fs2::FileExt::try_lock_exclusive(&lock).map_err(|_|ClientError::AlreadyRunning)?;
        let mut connection=Connection::open(&path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.pragma_update(None,"foreign_keys",true)?;
        connection.pragma_update(None,"journal_mode","WAL")?;
        connection.pragma_update(None,"synchronous","FULL")?;
        migrations::migrate(&mut connection)?;
        let (sender,mut receiver)=mpsc::channel::<Work>(64);
        let thread=std::thread::Builder::new().name("murm-client-sqlite".into()).spawn(move||{
            let _instance_lock:File=lock;
            while let Some(work)=receiver.blocking_recv(){work(&mut connection);}
        })?;
        Ok(Self{inner:Arc::new(Inner{sender:Some(sender),thread:Some(thread)})})
    }
    pub async fn call<T:Send+'static>(&self,f:impl FnOnce(&mut Connection)->Result<T,ClientError>+Send+'static)->Result<T,ClientError> {
        let (send,recv)=oneshot::channel();
        self.inner.sender.as_ref().ok_or(ClientError::Io)?.send(Box::new(move|c|{let _=send.send(f(c));})).await.map_err(|_|ClientError::Io)?;
        recv.await.map_err(|_|ClientError::Io)?
    }
}
