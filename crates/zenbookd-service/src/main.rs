mod adapter;
mod battery;
mod config;
mod ipc;
mod policy;
mod wake;
mod wifi;

use std::{
    sync::{Arc, Mutex, RwLock},
    thread,
    time::{Duration, Instant},
};

use crate::{
    adapter::Adapter,
    battery::Battery,
    config::{Config, PersistentState, State, flush_state, load_config, load_state},
    wake::Wake,
    wifi::Wifi,
};

const POWER_POLL_INTERVAL: Duration = Duration::from_secs(5);
const WIFI_RECHECK_INTERVAL: Duration = Duration::from_secs(60);
const IW_FAILURES_BEFORE_REDISCOVER: u8 = 3;

type Reported = Arc<Mutex<Option<String>>>;

fn main() {
    env_logger::builder()
        .filter_module("zenbookd_service", log::LevelFilter::Debug)
        .format_timestamp(None)
        .init();

    let (cfg, config_error) = load_initial_config();

    let battery = Arc::new(Battery::find().expect("Failed to detect battery"));
    let config = Arc::new(RwLock::new(cfg));
    let state = Arc::new(Mutex::new(PersistentState::new(load_initial_state())));

    let wake = Arc::new(Wake::new());

    let threshold_error: Reported = Arc::new(Mutex::new(None));
    let config_error: Reported = Arc::new(Mutex::new(config_error));

    let battery_clone = Arc::clone(&battery);
    let config_clone = Arc::clone(&config);
    let state_clone = Arc::clone(&state);
    let wake_clone = Arc::clone(&wake);
    let threshold_error_clone = Arc::clone(&threshold_error);

    thread::spawn(move || {
        monitor_battery(
            battery_clone,
            config_clone,
            state_clone,
            wake_clone,
            threshold_error_clone,
        );
    });

    let config_clone = Arc::clone(&config);
    let state_clone = Arc::clone(&state);
    let wake_clone = Arc::clone(&wake);

    thread::spawn(move || {
        monitor_power(config_clone, state_clone, wake_clone);
    });

    if let Err(err) = ipc::run_server(config, battery, state, wake, threshold_error, config_error) {
        log::error!("Failed to start IPC server: {err}");
        std::process::exit(1);
    }
}

fn load_initial_config() -> (Config, Option<String>) {
    match load_config() {
        Ok(cfg) => (cfg, None),

        Err(err) => {
            use config::ConfigLoadError::*;

            let reported = match err {
                NotFound => {
                    log::warn!("No config file found");

                    None
                }

                Invalid(err) => {
                    log::error!("Invalid or malformed config file: {err}");

                    Some(err.to_string())
                }

                IoError(err) => {
                    log::error!("Failed to read config file: {err}");

                    Some(err.to_string())
                }
            };

            log::debug!("Using defaults...");

            (Config::default(), reported)
        }
    }
}

fn load_initial_state() -> State {
    match load_state() {
        Ok(state) => state,

        Err(err) => {
            use config::ConfigLoadError::*;

            match err {
                Invalid(err) => {
                    log::error!("Invalid or malformed state file, starting fresh: {err}")
                }
                IoError(err) => log::error!("Failed to read state file, starting fresh: {err}"),

                NotFound => log::debug!("No state file found, starting fresh"),
            }

            State::default()
        }
    }
}

fn monitor_battery(
    battery: Arc<Battery>,
    config: Arc<RwLock<Config>>,
    state: Arc<Mutex<PersistentState>>,
    wake: Arc<Wake>,
    threshold_error: Reported,
) {
    log::info!("Started battery monitoring thread");

    let mut last_seen = 0;

    loop {
        let cfg = config.read().unwrap().clone();

        let current_capacity = match battery.capacity() {
            Ok(cap) => cap,

            Err(err) => {
                log::error!("Failed to read battery capacity: {err}");
                flush_state(&state);
                wake.wait_timeout(&mut last_seen, Duration::from_secs(60));
                continue;
            }
        };

        let target_threshold = {
            let mut state = state.lock().unwrap();

            let decision =
                policy::decide(&cfg, state.data_mut(), current_capacity, chrono::Utc::now());

            if decision.state_dirty {
                state.mark_dirty();
            }

            decision.target_threshold
        };

        flush_state(&state);

        let applied = match battery.threshold() {
            Ok(threshold) => Some(threshold),

            Err(err) => {
                log::error!("Failed to read charge threshold: {err}");
                *threshold_error.lock().unwrap() = Some(err.to_string());

                None
            }
        };

        if applied == Some(target_threshold) {
            *threshold_error.lock().unwrap() = None;
        } else {
            match applied {
                Some(current) => {
                    log::info!("Changing charge threshold from {current} to {target_threshold}")
                }

                None => log::info!("Applying charge threshold {target_threshold}"),
            }

            match battery.set_threshold(target_threshold) {
                Ok(()) if applied.is_some() => *threshold_error.lock().unwrap() = None,
                Ok(()) => {}

                Err(err) => {
                    log::error!("Failed to set charge threshold: {err}");

                    *threshold_error.lock().unwrap() = Some(err.to_string());
                }
            }
        }

        flush_state(&state);
        wake.wait_timeout(&mut last_seen, Duration::from_secs(30));
    }
}

fn monitor_power(config: Arc<RwLock<Config>>, state: Arc<Mutex<PersistentState>>, wake: Arc<Wake>) {
    log::info!("Started power monitoring thread");

    let mut last_seen = 0;

    let mut devices = None;
    let mut reported = false;
    let mut iw_failures = 0;

    let mut last_online = None;
    let mut last_checked: Option<Instant> = None;

    loop {
        let enabled = config.read().unwrap().disable_wifi_power_save_on_ac;
        let pending = state
            .lock()
            .unwrap()
            .data()
            .wifi_power_save_restore
            .is_some();

        if devices.is_none() && (enabled || pending) {
            devices = find_devices(&mut reported);
        }

        let Some((adapter, wifi)) = devices.clone() else {
            flush_state(&state);
            wake.wait_timeout(&mut last_seen, POWER_POLL_INTERVAL);
            continue;
        };

        if !enabled {
            if pending {
                let original = state.lock().unwrap().data().wifi_power_save_restore;

                if let Some(original) = original {
                    log::info!("Wi-Fi power saving feature disabled, restoring original state");

                    match wifi.set_power_save(original) {
                        Ok(()) => {
                            iw_failures = 0;

                            let mut state = state.lock().unwrap();

                            if state.data().wifi_power_save_restore == Some(original) {
                                state.data_mut().wifi_power_save_restore = None;
                                state.mark_dirty();
                            }
                        }

                        Err(err) => {
                            log::error!("Failed to restore Wi-Fi power save: {err}");
                            note_iw_failure(&mut iw_failures, &mut devices, &mut reported);
                        }
                    }
                }
            }

            last_online = None;
            last_checked = None;

            flush_state(&state);
            wake.wait_timeout(&mut last_seen, POWER_POLL_INTERVAL);
            continue;
        }

        let online = match adapter.online() {
            Ok(online) => online,

            Err(err) => {
                log::error!("Failed to read AC adapter state: {err}");
                flush_state(&state);
                wake.wait_timeout(&mut last_seen, POWER_POLL_INTERVAL);
                continue;
            }
        };

        let stale = last_checked.is_none_or(|at| at.elapsed() >= WIFI_RECHECK_INTERVAL);

        if last_online == Some(online) && !stale {
            flush_state(&state);
            wake.wait_timeout(&mut last_seen, POWER_POLL_INTERVAL);
            continue;
        }

        let current = match wifi.power_save() {
            Ok(current) => {
                iw_failures = 0;
                current
            }

            Err(err) => {
                log::error!("Failed to read Wi-Fi power save: {err}");
                note_iw_failure(&mut iw_failures, &mut devices, &mut reported);
                flush_state(&state);
                wake.wait_timeout(&mut last_seen, POWER_POLL_INTERVAL);
                continue;
            }
        };

        let restore = state.lock().unwrap().data().wifi_power_save_restore;
        let mut settled = true;
        let mut new_restore: Option<Option<bool>> = None;

        // The interface resets power saving to the driver default on every boot, so what we want is
        // decided against the interface itself. The stored value is only the original to hand back
        // when leaving AC, never a record of what is currently applied.
        if online {
            if current {
                log::info!("On AC power, disabling Wi-Fi power save");

                if let Err(err) = wifi.set_power_save(false) {
                    log::error!("Failed to disable Wi-Fi power save: {err}");
                    settled = false;
                    note_iw_failure(&mut iw_failures, &mut devices, &mut reported);
                } else {
                    iw_failures = 0;

                    if restore.is_none() {
                        new_restore = Some(Some(current));
                    }
                }
            }
        } else if let Some(original) = restore {
            let restored = if current == original {
                true
            } else {
                log::info!("On battery power, restoring Wi-Fi power save");

                match wifi.set_power_save(original) {
                    Ok(()) => {
                        iw_failures = 0;
                        true
                    }

                    Err(err) => {
                        log::error!("Failed to restore Wi-Fi power save: {err}");
                        note_iw_failure(&mut iw_failures, &mut devices, &mut reported);
                        false
                    }
                }
            };

            if restored {
                new_restore = Some(None);
            } else {
                settled = false;
            }
        }

        if let Some(wifi_power_save_restore) = new_restore {
            let mut state = state.lock().unwrap();
            state.data_mut().wifi_power_save_restore = wifi_power_save_restore;
            state.mark_dirty();
        }

        if settled {
            last_online = Some(online);
            last_checked = Some(Instant::now());
        }

        flush_state(&state);
        wake.wait_timeout(&mut last_seen, POWER_POLL_INTERVAL);
    }
}

fn note_iw_failure(failures: &mut u8, devices: &mut Option<(Adapter, Wifi)>, reported: &mut bool) {
    *failures = failures.saturating_add(1);

    if *failures >= IW_FAILURES_BEFORE_REDISCOVER {
        *devices = None;
        *reported = false;
        *failures = 0;
    }
}

fn find_devices(reported: &mut bool) -> Option<(Adapter, Wifi)> {
    match (Adapter::find(), Wifi::find()) {
        (Ok(adapter), Ok(wifi)) => {
            log::info!("Using wireless interface {}", wifi.interface());
            *reported = false;

            Some((adapter, wifi))
        }

        (adapter, wifi) => {
            if !*reported {
                if let Err(err) = adapter {
                    log::warn!("AC adapter not available, waiting for it: {err}");
                }

                if let Err(err) = wifi {
                    log::warn!("Wireless interface not available, waiting for it: {err}");
                }

                *reported = true;
            }

            None
        }
    }
}
