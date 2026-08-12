use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::{ConfigLoadError, ConfigSaveError};

pub const MIN_CHARGE_LIMIT: u32 = 1;
pub const MAX_CHARGE_LIMIT: u32 = 100;

const DEFAULT_CONFIG_TEMPLATE: &str = "\
# zenbookd configuration

# The charge limit in percentage between 1-100.
charge_limit = 80

# Whether to periodically charge to 100% to calibrate the BMS.
enable_periodic_full_charge = true

# The period in days for the full charge.
full_charge_period = 30

# When enabled, Wi-Fi power saving is disabled while on AC power.
disable_wifi_power_save_on_ac = true
";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    // The percentage the battery is held at. Values outside MIN_CHARGE_LIMIT..=MAX_CHARGE_LIMIT
    // are clamped on load; 0 is invalid because the kernel rejects a zero end-threshold.
    pub charge_limit: u32,

    // This will allow toggling the full battery charging setting without changing the value.
    // If disabled the full_charge_period will be ignored.
    pub enable_periodic_full_charge: bool,

    // The duration of time in days where the battery will ignore the charge limit and
    // charge until the battery is fully charged to allow the BMS to calibrate itself.
    pub full_charge_period: u32,

    // When enabled the Wi-Fi power saving features are disabled while the device is plugged
    // into AC power and restored to their original state once unplugged.
    #[serde(default = "default_true")]
    pub disable_wifi_power_save_on_ac: bool,
}

fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            // The default config will allow the battery to fully charge itself.
            charge_limit: 100,

            // By default the periodic full charge modes will be enabled as IsraelGPT has
            // informed me that that's probably the way to go.
            enable_periodic_full_charge: true,

            // By default the full recharge will be done every every 2 months.
            // as again IsraelGPT told me that's a good idea.
            full_charge_period: 90,

            disable_wifi_power_save_on_ac: true,
        }
    }
}

impl Config {
    fn clamp_charge_limit(&mut self) {
        let clamped = self.charge_limit.clamp(MIN_CHARGE_LIMIT, MAX_CHARGE_LIMIT);

        if clamped != self.charge_limit {
            log::warn!(
                "Charge limit {} is out of range, using {}",
                self.charge_limit,
                clamped
            );

            self.charge_limit = clamped;
        }
    }
}

pub fn validate_charge_limit(limit: u32) -> Result<(), String> {
    if (MIN_CHARGE_LIMIT..=MAX_CHARGE_LIMIT).contains(&limit) {
        Ok(())
    } else {
        Err(format!(
            "Charge limit must be between {MIN_CHARGE_LIMIT} and {MAX_CHARGE_LIMIT}, got {limit}"
        ))
    }
}

pub const MIN_FULL_CHARGE_PERIOD: u32 = 1;
pub const MAX_FULL_CHARGE_PERIOD: u32 = 365;

pub fn validate_full_charge_period(days: u32) -> Result<(), String> {
    if (MIN_FULL_CHARGE_PERIOD..=MAX_FULL_CHARGE_PERIOD).contains(&days) {
        Ok(())
    } else {
        Err(format!(
            "Full charge period must be between {MIN_FULL_CHARGE_PERIOD} and {MAX_FULL_CHARGE_PERIOD}, got {days}"
        ))
    }
}

pub fn load_config() -> Result<Config, ConfigLoadError> {
    load_config_from(&config_path())
}

pub fn load_config_from(path: &Path) -> Result<Config, ConfigLoadError> {
    if !path.is_file() {
        return Err(ConfigLoadError::NotFound);
    }

    let data = std::fs::read_to_string(path)?;
    let mut config = toml::from_str::<Config>(&data)?;

    config.clamp_charge_limit();

    Ok(config)
}

pub fn save_config(cfg: &Config) -> Result<(), ConfigSaveError> {
    save_config_to(&config_path(), cfg)
}

pub fn save_config_to(path: &Path, cfg: &Config) -> Result<(), ConfigSaveError> {
    use toml_edit::{DocumentMut, value};

    let mut doc = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| text.parse::<DocumentMut>().ok())
        .unwrap_or_else(|| DEFAULT_CONFIG_TEMPLATE.parse::<DocumentMut>().unwrap());

    doc["charge_limit"] = value(cfg.charge_limit as i64);
    doc["enable_periodic_full_charge"] = value(cfg.enable_periodic_full_charge);
    doc["full_charge_period"] = value(cfg.full_charge_period as i64);
    doc["disable_wifi_power_save_on_ac"] = value(cfg.disable_wifi_power_save_on_ac);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(path, doc.to_string()).map_err(Into::into)
}

fn config_path() -> PathBuf {
    let directory = std::env::var("CONFIG_DIR")
        .ok()
        .and_then(|v| v.split(':').next().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/etc/zenbookd"));

    directory.join("config.toml")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn defaults_are_conservative() {
        let cfg = Config::default();

        assert_eq!(cfg.charge_limit, 100);
        assert!(cfg.enable_periodic_full_charge);
        assert_eq!(cfg.full_charge_period, 90);
        assert!(cfg.disable_wifi_power_save_on_ac);
    }

    #[test]
    fn installed_config_without_wifi_key_defaults_it_on() {
        let text = "\
charge_limit = 80
enable_periodic_full_charge = true
full_charge_period = 30
";

        let cfg: Config = toml::from_str(text).unwrap();

        assert!(cfg.disable_wifi_power_save_on_ac);
    }

    #[test]
    fn missing_required_field_is_rejected() {
        assert!(toml::from_str::<Config>("charge_limit = 80\n").is_err());
    }

    #[test]
    fn roundtrips_through_toml() {
        let cfg = Config {
            charge_limit: 75,
            enable_periodic_full_charge: false,
            full_charge_period: 45,
            disable_wifi_power_save_on_ac: false,
        };

        let text = toml::to_string_pretty(&cfg).unwrap();

        assert_eq!(toml::from_str::<Config>(&text).unwrap(), cfg);
    }

    #[test]
    fn charge_limit_above_the_maximum_is_clamped_down() {
        let mut cfg = Config {
            charge_limit: 5000,
            ..Config::default()
        };

        cfg.clamp_charge_limit();

        assert_eq!(cfg.charge_limit, 100);
    }

    #[test]
    fn zero_charge_limit_is_clamped_up_to_the_minimum() {
        let mut cfg = Config {
            charge_limit: 0,
            ..Config::default()
        };

        cfg.clamp_charge_limit();

        assert_eq!(cfg.charge_limit, 1);
    }

    #[test]
    fn in_range_charge_limit_is_left_alone() {
        let mut cfg = Config {
            charge_limit: 80,
            ..Config::default()
        };

        cfg.clamp_charge_limit();

        assert_eq!(cfg.charge_limit, 80);
    }

    #[test]
    fn validation_accepts_the_whole_valid_range() {
        assert!(validate_charge_limit(MIN_CHARGE_LIMIT).is_ok());
        assert!(validate_charge_limit(80).is_ok());
        assert!(validate_charge_limit(MAX_CHARGE_LIMIT).is_ok());
    }

    #[test]
    fn validation_rejects_values_the_hardware_would_refuse() {
        assert!(validate_charge_limit(0).is_err());
        assert!(validate_charge_limit(101).is_err());
        assert!(validate_charge_limit(u32::MAX).is_err());
    }

    #[test]
    fn validation_rejects_a_zero_day_period() {
        assert!(validate_full_charge_period(0).is_err());
    }

    #[test]
    fn validation_accepts_the_whole_valid_period_range() {
        assert!(validate_full_charge_period(MIN_FULL_CHARGE_PERIOD).is_ok());
        assert!(validate_full_charge_period(30).is_ok());
        assert!(validate_full_charge_period(MAX_FULL_CHARGE_PERIOD).is_ok());
    }

    #[test]
    fn saving_preserves_comments_in_the_existing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");

        fs::write(
            &path,
            "\
# zenbookd configuration

# The charge limit in percentage between 1-100.
charge_limit = 80

# Whether to periodically charge to 100% to calibrate the BMS.
enable_periodic_full_charge = true

# The period in days for the full charge.
full_charge_period = 30
",
        )
        .unwrap();

        let cfg = Config {
            charge_limit: 70,
            ..Config::default()
        };

        save_config_to(&path, &cfg).unwrap();

        let text = fs::read_to_string(&path).unwrap();

        assert!(text.contains("# The charge limit in percentage between 1-100."));
        assert!(text.contains("charge_limit = 70"));
    }

    #[test]
    fn saving_preserves_unknown_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");

        fs::write(
            &path,
            "\
charge_limit = 80
enable_periodic_full_charge = true
full_charge_period = 30
disable_wifi_power_save_on_ac = true
some_future_key = \"keep me\"
",
        )
        .unwrap();

        save_config_to(&path, &Config::default()).unwrap();

        let text = fs::read_to_string(&path).unwrap();

        assert!(text.contains("some_future_key = \"keep me\""));
    }

    #[test]
    fn saving_creates_a_commented_file_when_none_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nested").join("config.toml");

        save_config_to(&path, &Config::default()).unwrap();

        let text = fs::read_to_string(&path).unwrap();

        assert!(path.exists());
        assert!(text.contains('#'));
    }

    #[test]
    fn saving_over_an_unparseable_file_still_writes_the_values() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");

        fs::write(&path, "!!! not toml").unwrap();

        let cfg = Config {
            charge_limit: 65,
            enable_periodic_full_charge: false,
            full_charge_period: 45,
            disable_wifi_power_save_on_ac: false,
        };

        save_config_to(&path, &cfg).unwrap();

        let loaded = load_config_from(&path).unwrap();

        assert_eq!(loaded, cfg);
    }

    #[test]
    fn saved_config_round_trips_through_load() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");

        let cfg = Config {
            charge_limit: 55,
            enable_periodic_full_charge: true,
            full_charge_period: 60,
            disable_wifi_power_save_on_ac: false,
        };

        save_config_to(&path, &cfg).unwrap();

        assert_eq!(load_config_from(&path).unwrap(), cfg);
    }
}
