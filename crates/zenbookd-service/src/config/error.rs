use thiserror::Error;

use crate::config::config::{
    MAX_CHARGE_LIMIT, MAX_FULL_CHARGE_HOLD, MAX_FULL_CHARGE_PERIOD, MIN_CHARGE_LIMIT,
    MIN_FULL_CHARGE_HOLD, MIN_FULL_CHARGE_PERIOD,
};

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

#[derive(Debug, Error)]
pub enum OutOfRange {
    #[error("Charge limit must be between {MIN_CHARGE_LIMIT} and {MAX_CHARGE_LIMIT}, got {0}")]
    ChargeLimit(u32),

    #[error(
        "Full charge period must be between {MIN_FULL_CHARGE_PERIOD} and {MAX_FULL_CHARGE_PERIOD}, got {0}"
    )]
    FullChargePeriod(u32),

    #[error(
        "Full charge hold must be between {MIN_FULL_CHARGE_HOLD} and {MAX_FULL_CHARGE_HOLD} minutes, got {0}"
    )]
    FullChargeHold(u32),
}
