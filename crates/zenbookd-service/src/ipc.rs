use std::{
    fs,
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::Path,
    sync::{Arc, Mutex, RwLock},
    thread,
    time::{Duration, Instant},
};

use zenbookd_ipc::{Request, Response, ServiceStatus, socket_path};

use crate::{
    battery::Battery,
    config::{
        Config, PersistentState, flush_state, save_config, save_state, validate_charge_limit,
        validate_full_charge_hold, validate_full_charge_period,
    },
    policy::FullChargeClock,
    wake::Wake,
};

const BOOST_DURATION_HOURS: i64 = 24;
const CLIENT_TIMEOUT: Duration = Duration::from_secs(5);

type Reported = Arc<Mutex<Option<String>>>;

#[derive(Clone)]
pub struct Shared {
    pub config: Arc<RwLock<Config>>,
    pub battery: Arc<Battery>,
    pub state: Arc<Mutex<PersistentState>>,
    pub clock: Arc<Mutex<FullChargeClock>>,
    pub wake: Arc<Wake>,
    pub threshold_error: Reported,
    pub config_error: Reported,
}

pub fn run_server(shared: Shared) -> std::io::Result<()> {
    let socket_path = socket_path();
    let path = Path::new(&socket_path);

    if path.exists() {
        fs::remove_file(path)?;
    }

    let listener = UnixListener::bind(path)?;

    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o660);
    fs::set_permissions(path, perms)?;

    log::info!("IPC server listening on {socket_path}");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(err) = stream.set_read_timeout(Some(CLIENT_TIMEOUT)) {
                    log::error!("Failed to set IPC read timeout: {err}");
                    continue;
                }

                if let Err(err) = stream.set_write_timeout(Some(CLIENT_TIMEOUT)) {
                    log::error!("Failed to set IPC write timeout: {err}");
                    continue;
                }

                let shared = shared.clone();

                thread::spawn(move || {
                    if let Err(err) = handle_client(stream, shared) {
                        log::error!("Error handling IPC client: {err}");
                    }
                });
            }

            Err(err) => {
                log::error!("IPC accept error: {err}");
            }
        }
    }

    Ok(())
}

fn update_config(
    config: &RwLock<Config>,
    config_error: &Mutex<Option<String>>,
    apply: impl FnOnce(&mut Config),
) -> Response {
    let mut config = config.write().unwrap();

    let mut candidate = config.clone();
    apply(&mut candidate);

    match save_config(&candidate) {
        Ok(()) => {
            *config = candidate;
            *config_error.lock().unwrap() = None;

            Response::Ok
        }

        Err(err) => {
            log::error!("Failed to save config: {err}");

            Response::Error(format!("Failed to save config: {err}"))
        }
    }
}

fn handle_client(mut stream: UnixStream, shared: Shared) -> std::io::Result<()> {
    let Shared {
        config,
        battery,
        state,
        clock,
        wake,
        threshold_error,
        config_error,
    } = shared;

    let request: Request = match zenbookd_ipc::receive_message(&mut stream) {
        Ok(req) => req,

        Err(err) => {
            if let zenbookd_ipc::IpcError::Json(err) = &err {
                let response = Response::Error(format!("Invalid request: {err}"));

                let _ = zenbookd_ipc::send_message(&mut stream, &response);
            } else {
                log::warn!("Dropping IPC client: {err}");
            }

            return Ok(());
        }
    };

    let response = match request {
        Request::GetStatus => {
            let cfg = config.read().unwrap().clone();

            let (boost_until, last_full_charge, calibration_active) = {
                let state = state.lock().unwrap();
                let data = state.data();

                (
                    data.boost_until.map(|until| until.timestamp()),
                    data.last_full_charge.map(|last| last.timestamp()),
                    crate::policy::needs_full_charge(&cfg, data, chrono::Utc::now()),
                )
            };

            Response::Status(ServiceStatus {
                charge_limit: cfg.charge_limit,

                enable_periodic_full_charge: cfg.enable_periodic_full_charge,
                full_charge_period: cfg.full_charge_period,
                full_charge_hold: Some(cfg.full_charge_hold),

                battery_health: battery.health().ok(),
                battery_charge: battery.capacity().ok(),

                applied_threshold: battery.threshold().ok(),

                boost_until,
                last_full_charge,
                calibration_active,
                held_at_full: clock
                    .lock()
                    .unwrap()
                    .held(Instant::now())
                    .map(|held| held.as_secs()),

                threshold_error: threshold_error.lock().unwrap().clone(),
                config_error: config_error.lock().unwrap().clone(),
            })
        }

        Request::SetChargeLimit(limit) => {
            log::info!("Requested charge limit: {limit}");

            match validate_charge_limit(limit) {
                Err(err) => {
                    log::warn!("Rejected charge limit: {err}");

                    Response::Error(err.to_string())
                }

                Ok(()) => update_config(&config, &config_error, |cfg| cfg.charge_limit = limit),
            }
        }

        Request::SetPeriodicFullCharge(enable) => update_config(&config, &config_error, |cfg| {
            cfg.enable_periodic_full_charge = enable
        }),

        Request::SetFullChargePeriod(days) => match validate_full_charge_period(days) {
            Err(err) => {
                log::warn!("Rejected full charge period: {err}");

                Response::Error(err.to_string())
            }

            Ok(()) => update_config(&config, &config_error, |cfg| cfg.full_charge_period = days),
        },

        Request::SetFullChargeHold(minutes) => match validate_full_charge_hold(minutes) {
            Err(err) => {
                log::warn!("Rejected full charge hold: {err}");

                Response::Error(err.to_string())
            }

            Ok(()) => update_config(&config, &config_error, |cfg| cfg.full_charge_hold = minutes),
        },

        Request::SetWifiPowerSave(disable_on_ac) => update_config(&config, &config_error, |cfg| {
            cfg.disable_wifi_power_save_on_ac = disable_on_ac
        }),

        Request::ReloadConfig => match crate::config::load_config() {
            Ok(new) => {
                log::info!("Reloaded configuration from disk");

                *config.write().unwrap() = new;
                *config_error.lock().unwrap() = None;

                Response::Ok
            }

            Err(err) => {
                log::error!("Failed to reload config: {err}");

                *config_error.lock().unwrap() = Some(err.to_string());

                Response::Error(format!("Failed to reload config: {err}"))
            }
        },

        Request::SetBoost(enable) => {
            let until = if enable {
                let until = chrono::Utc::now() + chrono::Duration::hours(BOOST_DURATION_HOURS);

                log::info!("Boost enabled until {until} (or until held at full)");
                Some(until)
            } else {
                log::info!("Boost cancelled");
                None
            };

            let candidate = {
                let state = state.lock().unwrap();
                let mut candidate = state.data().clone();
                candidate.boost_until = until;
                candidate
            };

            match save_state(&candidate) {
                Ok(()) => {
                    {
                        let mut state = state.lock().unwrap();
                        state.data_mut().boost_until = until;
                        state.sync_after_write(&candidate);
                    }

                    flush_state(&state);

                    Response::Ok
                }

                Err(err) => {
                    log::error!("Failed to save state: {err}");

                    Response::Error(format!("Failed to save state: {err}"))
                }
            }
        }
    };

    wake.notify();

    if let Err(err) = zenbookd_ipc::send_message(&mut stream, &response) {
        log::error!("Error sending IPC response: {err}");
    }

    Ok(())
}
