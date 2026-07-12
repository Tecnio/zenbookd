mod adapter;
mod battery;
mod config;
mod ipc;
mod wake;
mod wifi;

use std::{
    sync::{Arc, RwLock},
    thread,
    time::Duration,
};

use crate::{
    adapter::Adapter,
    battery::Battery,
    config::{Config, load_config, load_state, save_state},
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

    let wake = Arc::new(Wake::new());

    let battery_clone = Arc::clone(&battery);
    let config_clone = Arc::clone(&config);
    let wake_clone = Arc::clone(&wake);

    thread::spawn(move || {
        monitor_battery(battery_clone, config_clone, wake_clone);
    });

    let config_clone = Arc::clone(&config);
    let wake_clone = Arc::clone(&wake);

    thread::spawn(move || {
        monitor_power(config_clone, wake_clone);
    });

    if let Err(err) = ipc::run_server(config, battery, wake) {
        log::error!("Failed to start IPC server: {err}");
        std::process::exit(1);
    }
}

fn monitor_battery(battery: Arc<Battery>, config: Arc<RwLock<Config>>, wake: Arc<Wake>) {
    log::info!("Started battery monitoring thread");

    let mut last_seen = 0;

    loop {
        let (charge_limit, enable_periodic_full_charge, full_charge_period) = {
            let cfg = config.read().unwrap();

            (
                cfg.charge_limit,
                cfg.enable_periodic_full_charge,
                cfg.full_charge_period,
            )
        };

        let current_capacity = match battery.capacity() {
            Ok(cap) => cap,

            Err(err) => {
                log::error!("Failed to read battery capacity: {err}");
                wake.wait_timeout(&mut last_seen, Duration::from_secs(60));
                continue;
            }
        };

        let mut state = load_state().unwrap_or_default();
        let mut state_dirty = false;

        let now = chrono::Utc::now();

        if current_capacity >= 100 {
            // Avoid constant updates if staying at 100
            if state
                .last_full_charge
                .is_none_or(|last| (now - last).num_minutes() > 60)
            {
                state.last_full_charge = Some(now);
                state_dirty = true;

                log::info!("Updated last full charge timestamp");
            }
        }

        let boost_active = match state.boost_until {
            Some(until) if now < until && current_capacity < 100 => true,

            Some(_) => {
                state.boost_until = None;
                state_dirty = true;

                log::info!("Boost finished, restoring charge limit");
                false
            }

            None => false,
        };

        if state_dirty && let Err(err) = save_state(&state) {
            log::error!("Failed to save state: {err}");
        }

        let mut target_threshold = charge_limit;

        if enable_periodic_full_charge {
            let needs_full_charge = match state.last_full_charge {
                Some(last) => {
                    let days_since = (now - last).num_days();

                    days_since >= full_charge_period as i64
                }

                None => true, // Never had a full charge or state lost
            };

            if needs_full_charge {
                log::debug!("Periodic full charge needed, setting threshold to 100");
                target_threshold = 100;
            }
        }

        if boost_active {
            log::debug!("Boost active, setting threshold to 100");
            target_threshold = 100;
        }

        let current_threshold = battery.threshold().unwrap_or(100);

        if current_threshold != target_threshold {
            log::info!(
                "Changing charge threshold from {} to {}",
                current_threshold,
                target_threshold
            );

            if let Err(err) = battery.set_threshold(target_threshold) {
                log::error!("Failed to set charge threshold: {err}");
            }
        }

        wake.wait_timeout(&mut last_seen, Duration::from_secs(30));
    }
}

fn monitor_power(config: Arc<RwLock<Config>>, wake: Arc<Wake>) {
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

        let mut state = load_state().unwrap_or_default();
        let mut state_dirty = false;

        if !enabled {
            if let Some(original) = state.wifi_power_save_restore.take() {
                log::info!("Wi-Fi power saving feature disabled, restoring original state");

                if let Err(err) = wifi.set_power_save(original) {
                    log::error!("Failed to restore Wi-Fi power save: {err}");
                } else {
                    state_dirty = true;
                }
            }

            if state_dirty && let Err(err) = save_state(&state) {
                log::error!("Failed to save state: {err}");
            }

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

        if state_dirty && let Err(err) = save_state(&state) {
            log::error!("Failed to save state: {err}");
        }

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
