//! Local profile, vault and publication services shared by commands and TUI.
mod app;
mod profiles;
mod error;
mod storage;
mod vault;
mod transport;
mod sync;
mod articles;
mod outbox;
pub use app::{ClientService,ClientClock};
pub use error::ClientError;
pub use profiles::*;
pub use vault::{SecretString,SecretSeed,VaultActionId,VaultStatus,ClientEvent};
pub use transport::{RelayTransport,NetworkOptions,VerifiedQuery,VerifiedSync};
pub use sync::SyncSummary;
pub use articles::{Draft,DraftId,ArticleSummary};
pub use outbox::{OutboxEntry,DestinationStatus,DestinationState};

pub fn default_data_dir()->Result<std::path::PathBuf,ClientError> {
    directories::ProjectDirs::from("org","Murm","murm").map(|p|p.data_dir().to_owned()).ok_or(ClientError::Io)
}
