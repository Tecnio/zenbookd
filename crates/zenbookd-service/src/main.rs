mod battery;
mod config;
mod ipc;

use std::{
    sync::{Arc, RwLock, mpsc},
    thread,
    time::Duration,
};

use crate::{
    battery::Battery,
    config::{Config, load_config, load_state, save_state},
};

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

    if let Err(err) = ipc::run_server(config, battery, tx) {
        log::error!("Failed to start IPC server: {err}");
        std::process::exit(1);
    }
}

fn monitor_battery(battery: Arc<Battery>, config: Arc<RwLock<Config>>, rx: mpsc::Receiver<()>) {
    log::info!("Started battery monitoring thread");

    loop {
        let (charge_limit, enable_periodic_full_cycle, full_cycle_period) = {
            let cfg = config.read().unwrap();

            (
                cfg.charge_limit,
                cfg.enable_periodic_full_cycle,
                cfg.full_cycle_period,
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
                .last_full_cycle
                .is_none_or(|last| (now - last).num_minutes() > 60)
            {
                state.last_full_cycle = Some(now);
                state_dirty = true;

                log::info!("Updated last full charge cycle timestamp");
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

        if enable_periodic_full_cycle {
            let needs_full_cycle = match state.last_full_cycle {
                Some(last) => {
                    let days_since = (now - last).num_days();

                    days_since >= full_cycle_period as i64
                }

                None => true, // Never had a full cycle or state lost
            };

            if needs_full_cycle {
                log::debug!("Periodic full cycle needed, setting threshold to 100");
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
