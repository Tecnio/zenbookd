use std::{fs, path::PathBuf};

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
        let read_dir = fs::read_dir(POWER_SUPPLY)?;

        for entry in read_dir.filter_map(Result::ok) {
            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or(BatteryError::NameParseError)?;

            if !name.starts_with(POWER_SUPPLY_PREFIX) {
                continue;
            }

            let battery = Battery {
                capacity_physical: path.join(CAPACITY_PHYSICAL_KEY),
                capacity_design: path.join(CAPACITY_DESIGN_KEY),

                capacity: path.join(CAPACITY_KEY),

                threshold: path.join(THRESHOLD_KEY),
            };

            return Ok(battery);
        }

        Err(BatteryError::NotFound)
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

        let ratio = capacity_physical as f32 / capacity_design as f32;

        let percentage = (ratio * 100.0).round() as u32;
        let percentage = percentage.clamp(0, 100);

        Ok(percentage)
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
