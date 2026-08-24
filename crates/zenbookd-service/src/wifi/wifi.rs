use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

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
        Self::find_in(Path::new(NET))
    }

    fn find_in(root: &Path) -> Result<Wifi, WifiError> {
        let mut paths: Vec<PathBuf> = fs::read_dir(root)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();

        paths.sort();

        for path in paths {
            if !path.join(WIRELESS_KEY).exists() {
                continue;
            }

            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };

            let wifi = Wifi {
                interface: name.to_string(),
            };

            return Ok(wifi);
        }

        Err(WifiError::NotFound)
    }

    pub fn interface(&self) -> &str {
        &self.interface
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

        parse_power_save(&stdout)
            .ok_or_else(|| WifiReadError::UnexpectedOutput(stdout.trim().to_string()))
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

fn parse_power_save(stdout: &str) -> Option<bool> {
    match stdout.split(':').nth(1)?.trim() {
        "on" => Some(true),
        "off" => Some(false),

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_value_after_the_colon() {
        assert_eq!(parse_power_save("Power save: on\n"), Some(true));
        assert_eq!(parse_power_save("Power save: off\n"), Some(false));
    }

    #[test]
    fn rejects_output_it_does_not_recognise() {
        assert_eq!(parse_power_save("command not found"), None);
        assert_eq!(parse_power_save("Power save: maybe"), None);
        assert_eq!(parse_power_save(""), None);
    }

    #[test]
    fn finds_the_wireless_interface() {
        let tmp = tempfile::tempdir().unwrap();

        fs::create_dir_all(tmp.path().join("eth0")).unwrap();
        fs::create_dir_all(tmp.path().join("wlan0").join(WIRELESS_KEY)).unwrap();

        let wifi = Wifi::find_in(tmp.path()).unwrap();

        assert_eq!(wifi.interface(), "wlan0");
    }

    #[test]
    fn reports_not_found_without_a_wireless_interface() {
        let tmp = tempfile::tempdir().unwrap();

        fs::create_dir_all(tmp.path().join("eth0")).unwrap();
        fs::create_dir_all(tmp.path().join("lo")).unwrap();

        assert!(matches!(
            Wifi::find_in(tmp.path()),
            Err(WifiError::NotFound)
        ));
    }
}
