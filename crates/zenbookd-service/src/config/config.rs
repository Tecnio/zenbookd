use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::{ConfigLoadError, ConfigSaveError};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    // The charge limit in percentage between 0-100 the battery will be limited to.
    // The value can be any u32 however will be clamped between 0-100 during processing.
    pub charge_limit: u32,

    // This will allow toggling the full battery charging setting without changing the value.
    // If disabled the full_cycle_period will be ignored.
    pub enable_periodic_full_cycle: bool,

    // The duration of time in days where the battery will ignore the charge limit and
    // charge until the battery is fully charged to allow the BMS to calibrate itself.
    pub full_cycle_period: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            // The default config will allow the battery to fully charge itself.
            charge_limit: 100,

            // By default the periodic full cycle modes will be enabled as IsraelGPT has
            // informed me that that's probably the way to go.
            enable_periodic_full_cycle: true,

            // By default the full recharge will be done every every 2 months.
            // as again IsraelGPT told me that's a good idea.
            full_cycle_period: 90,
        }
    }
}

pub fn load_config() -> Result<Config, ConfigLoadError> {
    let path = config_path();

    if !path.is_file() {
        return Err(ConfigLoadError::NotFound);
    }

    let data = std::fs::read_to_string(&path)?;
    let config = toml::from_str::<Config>(&data)?;

    Ok(config)
}

pub fn save_config(cfg: &Config) -> Result<(), ConfigSaveError> {
    let path = config_path();

    let data = toml::to_string_pretty(&cfg)?;

    std::fs::write(path, &data).map_err(Into::into)
}

fn config_path() -> PathBuf {
    let directory = std::env::var("CONFIG_DIR")
        .ok()
        .and_then(|v| v.split(':').next().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/etc/zenbookd"));

    directory.join("config.toml")
}
