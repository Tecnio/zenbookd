use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use crate::wifi::{WifiError, WifiReadError, WifiSetError};

const NET: &str = "/sys/class/net/";
const WIRELESS_KEY: &str = "phy80211";

const IW: &str = "iw";
const IW_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
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

            if is_p2p_interface(name) {
                continue;
            }

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
        let output = run_iw(&["dev", &self.interface, "get", "power_save"])?;

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

        let output = run_iw(&["dev", &self.interface, "set", "power_save", value])?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);

            return Err(WifiSetError::CommandFailed(stderr.trim().to_string()));
        }

        Ok(())
    }
}

fn is_p2p_interface(name: &str) -> bool {
    name.starts_with("p2p-")
}

fn run_iw(args: &[&str]) -> io::Result<Output> {
    let mut child = Command::new(IW)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let status = wait_for_exit(&mut child, IW_TIMEOUT)?;

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    if let Some(mut pipe) = child.stdout.take() {
        pipe.read_to_end(&mut stdout)?;
    }

    if let Some(mut pipe) = child.stderr.take() {
        pipe.read_to_end(&mut stderr)?;
    }

    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn wait_for_exit(
    child: &mut std::process::Child,
    timeout: Duration,
) -> io::Result<std::process::ExitStatus> {
    let deadline = Instant::now() + timeout;

    loop {
        match child.try_wait()? {
            Some(status) => return Ok(status),

            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();

                return Err(io::Error::new(io::ErrorKind::TimedOut, "iw timed out"));
            }

            None => thread::sleep(Duration::from_millis(20)),
        }
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

    #[test]
    fn skips_p2p_device_interfaces() {
        let tmp = tempfile::tempdir().unwrap();

        fs::create_dir_all(tmp.path().join("p2p-dev-wlan0").join(WIRELESS_KEY)).unwrap();
        fs::create_dir_all(tmp.path().join("wlan0").join(WIRELESS_KEY)).unwrap();

        let wifi = Wifi::find_in(tmp.path()).unwrap();

        assert_eq!(wifi.interface(), "wlan0");
    }

    #[test]
    fn skips_p2p_group_interfaces() {
        let tmp = tempfile::tempdir().unwrap();

        fs::create_dir_all(tmp.path().join("p2p-wlan0-0").join(WIRELESS_KEY)).unwrap();
        fs::create_dir_all(tmp.path().join("wlp2s0").join(WIRELESS_KEY)).unwrap();

        let wifi = Wifi::find_in(tmp.path()).unwrap();

        assert_eq!(wifi.interface(), "wlp2s0");
    }

    #[test]
    fn reports_not_found_when_only_p2p_interfaces_exist() {
        let tmp = tempfile::tempdir().unwrap();

        fs::create_dir_all(tmp.path().join("p2p-dev-wlan0").join(WIRELESS_KEY)).unwrap();

        assert!(matches!(
            Wifi::find_in(tmp.path()),
            Err(WifiError::NotFound)
        ));
    }

    #[test]
    fn wait_for_exit_returns_when_the_child_exits() {
        let mut child = Command::new("/bin/true").spawn().unwrap();

        assert!(
            wait_for_exit(&mut child, Duration::from_secs(2))
                .unwrap()
                .success()
        );
    }

    #[test]
    fn wait_for_exit_kills_a_stuck_child() {
        let mut child = Command::new("/bin/sleep").arg("10").spawn().unwrap();
        let err = wait_for_exit(&mut child, Duration::from_millis(200)).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
    }
}
