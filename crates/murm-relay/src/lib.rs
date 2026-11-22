//! Persistent publication-test relay. Local policy is never event authenticity.
mod config;
mod error;
mod worker;
pub mod storage;
pub use config::*;
pub use error::*;
pub use storage::RelayStore;
pub use worker::DbWorker;
