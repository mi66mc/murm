use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProtocolError {
    #[error("invalid JSON")]
    InvalidJson,
    #[error("invalid event envelope")]
    InvalidEvent,
    #[error("unsupported version")]
    UnsupportedVersion,
    #[error("event ID mismatch")]
    InvalidId,
    #[error("invalid signature or signing key")]
    InvalidSignature,
    #[error("invalid publication")]
    InvalidPublication,
    #[error("invalid request")]
    InvalidRequest,
    #[error("unsupported filter")]
    UnsupportedFilter,
    #[error("unsupported feature")]
    UnsupportedFeature,
    #[error("limit exceeded")]
    LimitExceeded,
    #[error("content too large")]
    ContentTooLarge,
}
