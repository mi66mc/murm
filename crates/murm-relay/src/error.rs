use murm_protocol::{ProblemCode, ProtocolError};
use thiserror::Error;
#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database unavailable")]
    Database(#[from] rusqlite::Error),
    #[error("filesystem unavailable")]
    Io(#[from] std::io::Error),
    #[error("protocol validation failed: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("request refused: {0:?}")]
    Rejected(ProblemCode),
    #[error("invalid configuration")]
    Config,
    #[error("worker unavailable")]
    Unavailable,
}
impl StoreError {
    pub fn code(&self) -> ProblemCode {
        match self {
            Self::Protocol(e) => e.clone().into(),
            Self::Rejected(c) => *c,
            Self::Config => ProblemCode::InvalidRequest,
            _ => ProblemCode::TemporarilyUnavailable,
        }
    }
}
