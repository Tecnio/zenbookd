use thiserror::Error;

#[derive(Debug, Error)]
pub enum BatteryError {
    #[error("Battery not found")]
    NotFound,

    #[error("Failed to parse name of file/folder")]
    NameParseError,

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum BatteryReadError {
    #[error("Number parse error: {0}")]
    ParseError(#[from] std::num::ParseIntError),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum ThresholdSetError {
    #[error("Invalid threshold '{0}' value must be between 0-100'")]
    InvalidValue(u32),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}
