mod adapter;
mod battery;
mod config;
mod ipc;
mod wifi;

use std::{
    sync::{Arc, RwLock, mpsc},
    thread,
    time::Duration,
};

use crate::{
    adapter::Adapter,
    battery::Battery,
    config::{Config, load_config, load_state, save_state},
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

    let (tx, rx) = mpsc::channel();

    let battery_clone = Arc::clone(&battery);
    let config_clone = Arc::clone(&config);

    thread::spawn(move || {
        monitor_battery(battery_clone, config_clone, rx);
    });

    match (Adapter::find(), Wifi::find()) {
        (Ok(adapter), Ok(wifi)) => {
            let adapter = Arc::new(adapter);
            let wifi = Arc::new(wifi);
            let config_clone = Arc::clone(&config);

            thread::spawn(move || {
                monitor_power(adapter, wifi, config_clone);
            });
        }

        (adapter, wifi) => {
            if let Err(err) = adapter {
                log::warn!("AC adapter not available, skipping Wi-Fi power saving: {err}");
            }

            if let Err(err) = wifi {
                log::warn!("Wireless interface not available, skipping Wi-Fi power saving: {err}");
            }
        }
    }

    if let Err(err) = ipc::run_server(config, battery, tx) {
        log::error!("Failed to start IPC server: {err}");
        std::process::exit(1);
    }
}

fn monitor_battery(battery: Arc<Battery>, config: Arc<RwLock<Config>>, rx: mpsc::Receiver<()>) {
    log::info!("Started battery monitoring thread");

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
                let _ = rx.recv_timeout(Duration::from_secs(60));
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

        let _ = rx.recv_timeout(Duration::from_secs(30));

        while rx.try_recv().is_ok() {}
    }
}

fn monitor_power(adapter: Arc<Adapter>, wifi: Arc<Wifi>, config: Arc<RwLock<Config>>) {
    log::info!("Started power monitoring thread");

    loop {
        let enabled = config.read().unwrap().disable_wifi_power_save_on_ac;

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

            thread::sleep(POWER_POLL_INTERVAL);
            continue;
        }

        let online = match adapter.online() {
            Ok(online) => online,

            Err(err) => {
                log::error!("Failed to read AC adapter state: {err}");
                thread::sleep(POWER_POLL_INTERVAL);
                continue;
            }
        };

        match (online, state.wifi_power_save_restore) {
            (true, None) => {
                let current = match wifi.power_save() {
                    Ok(current) => current,

                    Err(err) => {
                        log::error!("Failed to read Wi-Fi power save: {err}");
                        thread::sleep(POWER_POLL_INTERVAL);
                        continue;
                    }
                };

                if current {
                    log::info!("On AC power, disabling Wi-Fi power save");

                    if let Err(err) = wifi.set_power_save(false) {
                        log::error!("Failed to disable Wi-Fi power save: {err}");
                    } else {
                        state.wifi_power_save_restore = Some(current);
                        state_dirty = true;
                    }
                }
            }

            (false, Some(original)) => {
                log::info!("On battery power, restoring Wi-Fi power save");

                if let Err(err) = wifi.set_power_save(original) {
                    log::error!("Failed to restore Wi-Fi power save: {err}");
                } else {
                    state.wifi_power_save_restore = None;
                    state_dirty = true;
                }
            }

            _ => {}
        }

        if state_dirty && let Err(err) = save_state(&state) {
            log::error!("Failed to save state: {err}");
        }

        thread::sleep(POWER_POLL_INTERVAL);
    }
}
