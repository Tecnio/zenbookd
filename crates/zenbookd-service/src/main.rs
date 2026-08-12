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
    time::Duration,
};

use crate::{
    adapter::Adapter,
    battery::Battery,
    config::{Config, State, load_config, load_state, persist_state},
    wake::Wake,
    wifi::Wifi,
};

const POWER_POLL_INTERVAL: Duration = Duration::from_secs(5);

fn main() {
    env_logger::builder()
        .filter_module("zenbookd_service", log::LevelFilter::Debug)
        .format_timestamp(None)
        .init();

    let cfg = match load_config() {
        Ok(cfg) => cfg,

        Err(err) => {
            use config::ConfigLoadError::*;

            match err {
                Invalid(err) => log::error!("Invalid or malformed config file: {err}"),
                IoError(err) => log::error!("Failed to read config file: {err}"),

                NotFound => log::warn!("No config file found"),
            }

            log::debug!("Using defaults...");
            Default::default()
        }
    };

    let battery = Arc::new(Battery::find().expect("Failed to detect battery"));
    let config = Arc::new(RwLock::new(cfg));
    let state = Arc::new(Mutex::new(load_initial_state()));

    let wake = Arc::new(Wake::new());
    let threshold_error: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

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

    if let Err(err) = ipc::run_server(config, battery, state, wake, threshold_error) {
        log::error!("Failed to start IPC server: {err}");
        std::process::exit(1);
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
    state: Arc<Mutex<State>>,
    wake: Arc<Wake>,
    threshold_error: Arc<Mutex<Option<String>>>,
) {
    log::info!("Started battery monitoring thread");

    let mut last_seen = 0;

    loop {
        let cfg = config.read().unwrap().clone();

        let current_capacity = match battery.capacity() {
            Ok(cap) => cap,

            Err(err) => {
                log::error!("Failed to read battery capacity: {err}");
                wake.wait_timeout(&mut last_seen, Duration::from_secs(60));
                continue;
            }
        };

        let target_threshold = {
            let mut state = state.lock().unwrap();

            let decision = policy::decide(&cfg, &mut state, current_capacity, chrono::Utc::now());

            if decision.state_dirty {
                persist_state(&state);
            }

            decision.target_threshold
        };

        let current_threshold = battery.threshold().unwrap_or(100);

        if current_threshold != target_threshold {
            log::info!(
                "Changing charge threshold from {} to {}",
                current_threshold,
                target_threshold
            );

            if let Err(err) = battery.set_threshold(target_threshold) {
                log::error!("Failed to set charge threshold: {err}");

                *threshold_error.lock().unwrap() = Some(err.to_string());
            }
        } else {
            *threshold_error.lock().unwrap() = None;
        }

        wake.wait_timeout(&mut last_seen, Duration::from_secs(30));
    }
}

fn monitor_power(config: Arc<RwLock<Config>>, state: Arc<Mutex<State>>, wake: Arc<Wake>) {
    log::info!("Started power monitoring thread");

    let mut last_seen = 0;

    let mut devices = None;
    let mut reported = false;

    loop {
        let enabled = config.read().unwrap().disable_wifi_power_save_on_ac;

        if devices.is_none() && enabled {
            devices = find_devices(&mut reported);
        }

        let Some((adapter, wifi)) = &devices else {
            wake.wait_timeout(&mut last_seen, POWER_POLL_INTERVAL);
            continue;
        };

        if !enabled {
            let mut state = state.lock().unwrap();

            // Only clear the stored value once the interface has actually been
            // handed back, so a failed restore is retried on the next tick.
            if let Some(original) = state.wifi_power_save_restore {
                log::info!("Wi-Fi power saving feature disabled, restoring original state");

                if let Err(err) = wifi.set_power_save(original) {
                    log::error!("Failed to restore Wi-Fi power save: {err}");
                } else {
                    state.wifi_power_save_restore = None;
                    persist_state(&state);
                }
            }

            drop(state);

            wake.wait_timeout(&mut last_seen, POWER_POLL_INTERVAL);
            continue;
        }

        let online = match adapter.online() {
            Ok(online) => online,

            Err(err) => {
                log::error!("Failed to read AC adapter state: {err}");
                wake.wait_timeout(&mut last_seen, POWER_POLL_INTERVAL);
                continue;
            }
        };

        let current = match wifi.power_save() {
            Ok(current) => current,

            Err(err) => {
                log::error!("Failed to read Wi-Fi power save: {err}");
                wake.wait_timeout(&mut last_seen, POWER_POLL_INTERVAL);
                continue;
            }
        };

        let mut state = state.lock().unwrap();
        let mut state_dirty = false;

        // The interface resets power saving to the driver default on every boot, so what we want is
        // decided against the interface itself. The stored value is only the original to hand back
        // when leaving AC, never a record of what is currently applied.
        if online {
            if current {
                log::info!("On AC power, disabling Wi-Fi power save");

                if let Err(err) = wifi.set_power_save(false) {
                    log::error!("Failed to disable Wi-Fi power save: {err}");
                } else if state.wifi_power_save_restore.is_none() {
                    state.wifi_power_save_restore = Some(current);
                    state_dirty = true;
                }
            }
        } else if let Some(original) = state.wifi_power_save_restore {
            let restored = if current == original {
                true
            } else {
                log::info!("On battery power, restoring Wi-Fi power save");

                match wifi.set_power_save(original) {
                    Ok(()) => true,

                    Err(err) => {
                        log::error!("Failed to restore Wi-Fi power save: {err}");
                        false
                    }
                }
            };

            if restored {
                state.wifi_power_save_restore = None;
                state_dirty = true;
            }
        }

        if state_dirty {
            persist_state(&state);
        }

        drop(state);

        wake.wait_timeout(&mut last_seen, POWER_POLL_INTERVAL);
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
