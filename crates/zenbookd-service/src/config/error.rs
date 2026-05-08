use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigLoadError {
    #[error("Config file not found")]
    NotFound,

    #[error("Invalid or malformed config file: {0}")]
    Invalid(#[from] toml::de::Error),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum ConfigSaveError {
    #[error("Failed to serialize config: {0}")]
    TomlError(#[from] toml::ser::Error),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}
