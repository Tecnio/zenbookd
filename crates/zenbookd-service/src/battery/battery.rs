use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::battery::{BatteryError, BatteryReadError, ThresholdSetError};

const POWER_SUPPLY: &str = "/sys/class/power_supply/";
const POWER_SUPPLY_PREFIX: &str = "BAT";

const CAPACITY_PHYSICAL_KEY: &str = "energy_full";
const CAPACITY_DESIGN_KEY: &str = "energy_full_design";

const CAPACITY_KEY: &str = "capacity";

const THRESHOLD_KEY: &str = "charge_control_end_threshold";

#[derive(Debug)]
pub struct Battery {
    capacity_physical: PathBuf,
    capacity_design: PathBuf,

    capacity: PathBuf,

    threshold: PathBuf,
}

impl Battery {
    pub fn find() -> Result<Battery, BatteryError> {
        Self::find_in(Path::new(POWER_SUPPLY))
    }

    fn find_in(root: &Path) -> Result<Battery, BatteryError> {
        let mut paths: Vec<PathBuf> = fs::read_dir(root)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();

        paths.sort();

        let mut without_threshold = None;

        for path in paths {
            if !path.is_dir() {
                continue;
            }

            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };

            if !name.starts_with(POWER_SUPPLY_PREFIX) {
                continue;
            }

            let battery = Battery {
                capacity_physical: path.join(CAPACITY_PHYSICAL_KEY),
                capacity_design: path.join(CAPACITY_DESIGN_KEY),

                capacity: path.join(CAPACITY_KEY),

                threshold: path.join(THRESHOLD_KEY),
            };

            if battery.threshold.exists() {
                return Ok(battery);
            }

            without_threshold.get_or_insert(battery);
        }

        without_threshold.ok_or(BatteryError::NotFound)
    }

    pub fn health(&self) -> Result<u32, BatteryReadError> {
        let capacity_design = {
            let str = fs::read_to_string(&self.capacity_design)?;

            str.trim().parse::<u32>()?
        };

        let capacity_physical = {
            let str = fs::read_to_string(&self.capacity_physical)?;

            str.trim().parse::<u32>()?
        };

        if capacity_design == 0 {
            return Err(BatteryReadError::ZeroDesignCapacity);
        }

        let ratio = capacity_physical as f32 / capacity_design as f32;

        let percentage = (ratio * 100.0).round() as u32;

        Ok(percentage.min(100))
    }

    pub fn capacity(&self) -> Result<u32, BatteryReadError> {
        let str = fs::read_to_string(&self.capacity)?;
        let capacity = str.trim().parse::<u32>()?;

        Ok(capacity)
    }

    pub fn threshold(&self) -> Result<u32, BatteryReadError> {
        let str = fs::read_to_string(&self.threshold)?;
        let threshold = str.trim().parse::<u32>()?;

        Ok(threshold)
    }

    pub fn set_threshold(&self, threshold: u32) -> Result<(), ThresholdSetError> {
        if threshold == 0 || threshold > 100 {
            return Err(ThresholdSetError::InvalidValue(threshold));
        }

        fs::write(&self.threshold, threshold.to_string()).map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn supply(root: &Path, name: &str, attrs: &[(&str, &str)]) {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();

        for (key, value) in attrs {
            fs::write(dir.join(key), value).unwrap();
        }
    }

    #[test]
    fn finds_a_battery_among_other_supplies() {
        let tmp = tempfile::tempdir().unwrap();

        supply(tmp.path(), "ADP1", &[("type", "Mains\n")]);
        supply(tmp.path(), "BAT0", &[("capacity", "72\n")]);

        let battery = Battery::find_in(tmp.path()).unwrap();

        assert_eq!(battery.capacity().unwrap(), 72);
    }

    #[test]
    fn picks_the_lowest_numbered_battery() {
        let tmp = tempfile::tempdir().unwrap();

        supply(tmp.path(), "BAT1", &[("capacity", "10\n")]);
        supply(tmp.path(), "BAT0", &[("capacity", "72\n")]);

        let battery = Battery::find_in(tmp.path()).unwrap();

        assert_eq!(battery.capacity().unwrap(), 72);
    }

    #[test]
    fn prefers_the_battery_exposing_the_threshold() {
        let tmp = tempfile::tempdir().unwrap();

        supply(tmp.path(), "BAT0", &[("capacity", "10\n")]);
        supply(
            tmp.path(),
            "BAT1",
            &[
                ("capacity", "72\n"),
                ("charge_control_end_threshold", "80\n"),
            ],
        );

        let battery = Battery::find_in(tmp.path()).unwrap();

        assert_eq!(battery.capacity().unwrap(), 72);
    }

    #[test]
    fn falls_back_to_a_battery_without_the_threshold() {
        let tmp = tempfile::tempdir().unwrap();

        supply(tmp.path(), "BAT0", &[("capacity", "72\n")]);

        let battery = Battery::find_in(tmp.path()).unwrap();

        assert_eq!(battery.capacity().unwrap(), 72);
    }

    #[test]
    fn a_zero_design_capacity_is_an_error_rather_than_full_health() {
        let tmp = tempfile::tempdir().unwrap();

        supply(
            tmp.path(),
            "BAT0",
            &[("energy_full", "45000000\n"), ("energy_full_design", "0\n")],
        );

        let battery = Battery::find_in(tmp.path()).unwrap();

        assert!(matches!(
            battery.health(),
            Err(BatteryReadError::ZeroDesignCapacity)
        ));
    }

    #[test]
    fn reports_not_found_without_a_battery() {
        let tmp = tempfile::tempdir().unwrap();

        supply(tmp.path(), "ADP1", &[("type", "Mains\n")]);

        assert!(matches!(
            Battery::find_in(tmp.path()),
            Err(BatteryError::NotFound)
        ));
    }

    #[test]
    fn computes_health_as_a_percentage_of_design_capacity() {
        let tmp = tempfile::tempdir().unwrap();

        supply(
            tmp.path(),
            "BAT0",
            &[
                ("energy_full", "45000000\n"),
                ("energy_full_design", "50000000\n"),
            ],
        );

        let battery = Battery::find_in(tmp.path()).unwrap();

        assert_eq!(battery.health().unwrap(), 90);
    }

    #[test]
    fn writes_a_valid_threshold() {
        let tmp = tempfile::tempdir().unwrap();

        supply(
            tmp.path(),
            "BAT0",
            &[("charge_control_end_threshold", "80\n")],
        );

        let battery = Battery::find_in(tmp.path()).unwrap();

        battery.set_threshold(75).unwrap();

        assert_eq!(battery.threshold().unwrap(), 75);
    }

    #[test]
    fn refuses_a_threshold_the_kernel_would_reject() {
        let tmp = tempfile::tempdir().unwrap();

        supply(
            tmp.path(),
            "BAT0",
            &[("charge_control_end_threshold", "80\n")],
        );

        let battery = Battery::find_in(tmp.path()).unwrap();

        assert!(matches!(
            battery.set_threshold(0),
            Err(ThresholdSetError::InvalidValue(0))
        ));
        assert!(matches!(
            battery.set_threshold(101),
            Err(ThresholdSetError::InvalidValue(101))
        ));

        assert_eq!(battery.threshold().unwrap(), 80);
    }
}
