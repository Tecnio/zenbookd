use thiserror::Error;

#[derive(Debug, Error)]
pub enum WifiError {
    #[error("Wireless interface not found")]
    NotFound,

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum WifiReadError {
    #[error("Command failed: {0}")]
    CommandFailed(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum WifiSetError {
    #[error("Command failed: {0}")]
    CommandFailed(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}
