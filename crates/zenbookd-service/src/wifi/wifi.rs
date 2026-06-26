use std::{fs, process::Command};

use crate::wifi::{WifiError, WifiReadError, WifiSetError};

const NET: &str = "/sys/class/net/";
const WIRELESS_KEY: &str = "phy80211";

const IW: &str = "iw";

#[derive(Debug)]
pub struct Wifi {
    interface: String,
}

impl Wifi {
    pub fn find() -> Result<Wifi, WifiError> {
        let read_dir = fs::read_dir(NET)?;

        for entry in read_dir.filter_map(Result::ok) {
            let path = entry.path();

            if !path.join(WIRELESS_KEY).exists() {
                continue;
            }

            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or(WifiError::NameParseError)?;

            let wifi = Wifi {
                interface: name.to_string(),
            };

            return Ok(wifi);
        }

        Err(WifiError::NotFound)
    }

    pub fn power_save(&self) -> Result<bool, WifiReadError> {
        let output = Command::new(IW)
            .args(["dev", &self.interface, "get", "power_save"])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);

            return Err(WifiReadError::CommandFailed(stderr.trim().to_string()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        Ok(stdout.contains("on"))
    }

    pub fn set_power_save(&self, on: bool) -> Result<(), WifiSetError> {
        let value = if on { "on" } else { "off" };

        let output = Command::new(IW)
            .args(["dev", &self.interface, "set", "power_save", value])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);

            return Err(WifiSetError::CommandFailed(stderr.trim().to_string()));
        }

        Ok(())
    }
}
