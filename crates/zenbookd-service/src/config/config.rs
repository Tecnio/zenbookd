use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};

use crate::config::{ConfigLoadError, ConfigSaveError, OutOfRange, atomic};

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

# How long in minutes the battery has to sit at 100% before a full charge counts.
full_charge_hold = 60

# When enabled, Wi-Fi power saving is disabled while on AC power.
disable_wifi_power_save_on_ac = true
";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub charge_limit: u32,

    pub enable_periodic_full_charge: bool,

    pub full_charge_period: u32,

    #[serde(default = "default_full_charge_hold")]
    pub full_charge_hold: u32,

    #[serde(default = "default_true")]
    pub disable_wifi_power_save_on_ac: bool,
}

fn default_true() -> bool {
    true
}

fn default_full_charge_hold() -> u32 {
    DEFAULT_FULL_CHARGE_HOLD
}

impl Default for Config {
    fn default() -> Self {
        Self {
            charge_limit: 100,

            enable_periodic_full_charge: true,

            full_charge_period: 90,

            full_charge_hold: DEFAULT_FULL_CHARGE_HOLD,

            disable_wifi_power_save_on_ac: true,
        }
    }
}

impl Config {
    pub fn full_charge_hold(&self) -> Duration {
        Duration::from_secs(u64::from(self.full_charge_hold) * 60)
    }

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

    fn clamp_full_charge_period(&mut self) {
        let clamped = self
            .full_charge_period
            .clamp(MIN_FULL_CHARGE_PERIOD, MAX_FULL_CHARGE_PERIOD);

        if clamped != self.full_charge_period {
            log::warn!(
                "Full charge period {} is out of range, using {}",
                self.full_charge_period,
                clamped
            );

            self.full_charge_period = clamped;
        }
    }

    fn clamp_full_charge_hold(&mut self) {
        let clamped = self
            .full_charge_hold
            .clamp(MIN_FULL_CHARGE_HOLD, MAX_FULL_CHARGE_HOLD);

        if clamped != self.full_charge_hold {
            log::warn!(
                "Full charge hold {} is out of range, using {}",
                self.full_charge_hold,
                clamped
            );

            self.full_charge_hold = clamped;
        }
    }
}

pub fn validate_charge_limit(limit: u32) -> Result<(), OutOfRange> {
    if (MIN_CHARGE_LIMIT..=MAX_CHARGE_LIMIT).contains(&limit) {
        Ok(())
    } else {
        Err(OutOfRange::ChargeLimit(limit))
    }
}

pub const MIN_FULL_CHARGE_PERIOD: u32 = 1;
pub const MAX_FULL_CHARGE_PERIOD: u32 = 365;

pub fn validate_full_charge_period(days: u32) -> Result<(), OutOfRange> {
    if (MIN_FULL_CHARGE_PERIOD..=MAX_FULL_CHARGE_PERIOD).contains(&days) {
        Ok(())
    } else {
        Err(OutOfRange::FullChargePeriod(days))
    }
}

pub const MIN_FULL_CHARGE_HOLD: u32 = 1;
pub const MAX_FULL_CHARGE_HOLD: u32 = 720;
pub const DEFAULT_FULL_CHARGE_HOLD: u32 = 60;

pub fn validate_full_charge_hold(minutes: u32) -> Result<(), OutOfRange> {
    if (MIN_FULL_CHARGE_HOLD..=MAX_FULL_CHARGE_HOLD).contains(&minutes) {
        Ok(())
    } else {
        Err(OutOfRange::FullChargeHold(minutes))
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
    config.clamp_full_charge_period();
    config.clamp_full_charge_hold();

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
    doc["full_charge_hold"] = value(cfg.full_charge_hold as i64);
    doc["disable_wifi_power_save_on_ac"] = value(cfg.disable_wifi_power_save_on_ac);

    atomic::write(path, &doc.to_string()).map_err(Into::into)
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
    use std::{fs, os::unix::fs::PermissionsExt};

    use super::*;

    #[test]
    fn defaults_are_conservative() {
        let cfg = Config::default();

        assert_eq!(cfg.charge_limit, 100);
        assert!(cfg.enable_periodic_full_charge);
        assert_eq!(cfg.full_charge_period, 90);
        assert_eq!(cfg.full_charge_hold, 60);
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
        assert_eq!(cfg.full_charge_hold, DEFAULT_FULL_CHARGE_HOLD);
    }

    #[test]
    fn full_charge_hold_is_read_as_minutes() {
        let cfg = Config {
            full_charge_hold: 90,
            ..Config::default()
        };

        assert_eq!(cfg.full_charge_hold(), Duration::from_secs(90 * 60));
    }

    #[test]
    fn out_of_range_full_charge_hold_is_clamped() {
        let mut zero = Config {
            full_charge_hold: 0,
            ..Config::default()
        };
        let mut huge = Config {
            full_charge_hold: 5000,
            ..Config::default()
        };

        zero.clamp_full_charge_hold();
        huge.clamp_full_charge_hold();

        assert_eq!(zero.full_charge_hold, MIN_FULL_CHARGE_HOLD);
        assert_eq!(huge.full_charge_hold, MAX_FULL_CHARGE_HOLD);
    }

    #[test]
    fn validation_bounds_the_full_charge_hold() {
        assert!(validate_full_charge_hold(MIN_FULL_CHARGE_HOLD).is_ok());
        assert!(validate_full_charge_hold(MAX_FULL_CHARGE_HOLD).is_ok());

        assert!(matches!(
            validate_full_charge_hold(0),
            Err(OutOfRange::FullChargeHold(0))
        ));
        assert!(matches!(
            validate_full_charge_hold(MAX_FULL_CHARGE_HOLD + 1),
            Err(OutOfRange::FullChargeHold(_))
        ));
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
            full_charge_hold: 30,
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
    fn zero_full_charge_period_is_clamped_up() {
        let mut cfg = Config {
            full_charge_period: 0,
            ..Config::default()
        };

        cfg.clamp_full_charge_period();

        assert_eq!(cfg.full_charge_period, MIN_FULL_CHARGE_PERIOD);
    }

    #[test]
    fn full_charge_period_above_the_maximum_is_clamped_down() {
        let mut cfg = Config {
            full_charge_period: 5000,
            ..Config::default()
        };

        cfg.clamp_full_charge_period();

        assert_eq!(cfg.full_charge_period, MAX_FULL_CHARGE_PERIOD);
    }

    #[test]
    fn zero_full_charge_period_is_clamped_on_load() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");

        fs::write(
            &path,
            "\
charge_limit = 80
enable_periodic_full_charge = true
full_charge_period = 0
",
        )
        .unwrap();

        assert_eq!(
            load_config_from(&path).unwrap().full_charge_period,
            MIN_FULL_CHARGE_PERIOD
        );
    }

    #[test]
    fn oversized_full_charge_period_is_clamped_on_load() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");

        fs::write(
            &path,
            "\
charge_limit = 80
enable_periodic_full_charge = true
full_charge_period = 5000
",
        )
        .unwrap();

        assert_eq!(
            load_config_from(&path).unwrap().full_charge_period,
            MAX_FULL_CHARGE_PERIOD
        );
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
            full_charge_hold: 30,
            disable_wifi_power_save_on_ac: false,
        };

        save_config_to(&path, &cfg).unwrap();

        let loaded = load_config_from(&path).unwrap();

        assert_eq!(loaded, cfg);
    }

    #[test]
    fn a_failed_save_leaves_the_previous_file_intact() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");

        save_config_to(
            &path,
            &Config {
                charge_limit: 70,
                ..Config::default()
            },
        )
        .unwrap();

        let mut perms = fs::metadata(tmp.path()).unwrap().permissions();
        perms.set_mode(0o500);
        fs::set_permissions(tmp.path(), perms).unwrap();

        let result = save_config_to(
            &path,
            &Config {
                charge_limit: 90,
                ..Config::default()
            },
        );

        let mut perms = fs::metadata(tmp.path()).unwrap().permissions();
        perms.set_mode(0o700);
        fs::set_permissions(tmp.path(), perms).unwrap();

        if result.is_err() {
            assert_eq!(load_config_from(&path).unwrap().charge_limit, 70);
        }
    }

    #[test]
    fn saved_config_round_trips_through_load() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");

        let cfg = Config {
            charge_limit: 55,
            enable_periodic_full_charge: true,
            full_charge_period: 60,
            full_charge_hold: 120,
            disable_wifi_power_save_on_ac: false,
        };

        save_config_to(&path, &cfg).unwrap();

        assert_eq!(load_config_from(&path).unwrap(), cfg);
    }
}
