//! Local profile, vault and publication services shared by commands and TUI.
mod app;
mod articles;
pub mod commands;
mod editor;
mod error;
mod outbox;
mod profiles;
mod storage;
mod sync;
mod transport;
pub mod tui;
mod vault;
pub use app::{ClientClock, ClientService};
pub use articles::{ArticleSummary, Draft, DraftId};
pub use editor::EditorConfig;
pub use error::ClientError;
pub use outbox::{DestinationState, DestinationStatus, OutboxEntry};
pub use profiles::*;
pub use sync::SyncSummary;
pub use transport::{NetworkOptions, RelayTransport, VerifiedQuery, VerifiedSync};
pub use vault::{ClientEvent, SecretSeed, SecretString, VaultActionId, VaultStatus};

pub fn default_data_dir() -> Result<std::path::PathBuf, ClientError> {
    directories::ProjectDirs::from("org", "Murm", "murm")
        .map(|p| p.data_dir().to_owned())
        .ok_or(ClientError::Io)
}
