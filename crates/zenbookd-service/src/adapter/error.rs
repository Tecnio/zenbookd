use thiserror::Error;

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("AC adapter not found")]
    NotFound,

    #[error("Failed to parse name of file/folder")]
    NameParseError,

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum AdapterReadError {
    #[error("Number parse error: {0}")]
    ParseError(#[from] std::num::ParseIntError),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}
