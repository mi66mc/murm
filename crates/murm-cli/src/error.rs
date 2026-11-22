use thiserror::Error;
#[derive(Debug,Error)]
pub enum ClientError {
    #[error("vault is not initialized")] VaultNotInitialized,
    #[error("vault is locked")] Locked,
    #[error("read-only profile cannot sign")] ReadOnly,
    #[error("invalid key format")] InvalidKey,
    #[error("public key does not match private key")] KeyMismatch,
    #[error("wrong password or unauthentic vault")] WrongPassword,
    #[error("vault is corrupt or unsupported")] CorruptVault,
    #[error("not found")] NotFound,
    #[error("state changed; reload before editing")] Conflict,
    #[error("protocol error: {0}")] Protocol(#[from] murm_protocol::ProtocolError),
    #[error("network unavailable")] Network,
    #[error("database unavailable")] Database(#[from] rusqlite::Error),
    #[error("filesystem unavailable")] Io,
    #[error("another client is already using this database")] AlreadyRunning,
    #[error("invalid input")] InvalidInput,
    #[error("operation cancelled")] Cancelled,
    #[error("relay refused the request: {0:?}")] Relay(murm_protocol::ProblemCode),
    #[error("wait {0} seconds before retrying")] RetryAfter(u64),
}
impl From<std::io::Error> for ClientError {fn from(_:std::io::Error)->Self{Self::Io}}
