use std::{
    fs,
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::Path,
    sync::{Arc, Mutex, RwLock},
    thread,
    time::Duration,
};

use zenbookd_ipc::{Request, Response, ServiceStatus, socket_path};

use crate::{
    battery::Battery,
    config::{
        Config, State, save_config, save_state, validate_charge_limit, validate_full_charge_period,
    },
    wake::Wake,
};

const BOOST_DURATION_HOURS: i64 = 24;
const CLIENT_TIMEOUT: Duration = Duration::from_secs(5);

type Reported = Arc<Mutex<Option<String>>>;

pub fn run_server(
    config: Arc<RwLock<Config>>,
    battery: Arc<Battery>,
    state: Arc<Mutex<State>>,
    wake: Arc<Wake>,
    threshold_error: Reported,
    config_error: Reported,
) -> std::io::Result<()> {
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

                let config = Arc::clone(&config);
                let battery = Arc::clone(&battery);
                let state = Arc::clone(&state);
                let wake = Arc::clone(&wake);
                let threshold_error = Arc::clone(&threshold_error);
                let config_error = Arc::clone(&config_error);

                thread::spawn(move || {
                    if let Err(err) = handle_client(
                        stream,
                        config,
                        battery,
                        state,
                        wake,
                        threshold_error,
                        config_error,
                    ) {
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

fn handle_client(
    mut stream: UnixStream,
    config: Arc<RwLock<Config>>,
    battery: Arc<Battery>,
    state: Arc<Mutex<State>>,
    wake: Arc<Wake>,
    threshold_error: Reported,
    config_error: Reported,
) -> std::io::Result<()> {
    let request: Request = match zenbookd_ipc::receive_message(&mut stream) {
        Ok(req) => req,

        Err(err) => {
            // An oversized or truncated frame leaves the stream desynced, so the
            // only safe reply is none — close and let the client reconnect.
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

                (
                    state.boost_until.map(|until| until.timestamp()),
                    state.last_full_charge.map(|last| last.timestamp()),
                    crate::policy::needs_full_charge(&cfg, &state, chrono::Utc::now()),
                )
            };

            Response::Status(ServiceStatus {
                charge_limit: cfg.charge_limit,

                enable_periodic_full_charge: cfg.enable_periodic_full_charge,
                full_charge_period: cfg.full_charge_period,

                battery_health: battery.health().ok(),
                battery_charge: battery.capacity().ok(),

                applied_threshold: battery.threshold().ok(),

                boost_until,
                last_full_charge,
                calibration_active,

                threshold_error: threshold_error.lock().unwrap().clone(),
                config_error: config_error.lock().unwrap().clone(),
            })
        }

        Request::SetChargeLimit(limit) => {
            log::info!("Requested charge limit: {limit}");

            match validate_charge_limit(limit) {
                Err(err) => {
                    log::warn!("Rejected charge limit: {err}");

                    Response::Error(err)
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

                Response::Error(err)
            }

            Ok(()) => update_config(&config, &config_error, |cfg| cfg.full_charge_period = days),
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
            let mut state = state.lock().unwrap();

            state.boost_until = if enable {
                let until = chrono::Utc::now() + chrono::Duration::hours(BOOST_DURATION_HOURS);

                log::info!("Boost enabled until {until} (or until fully charged)");
                Some(until)
            } else {
                log::info!("Boost cancelled");
                None
            };

            match save_state(&state) {
                Ok(_) => Response::Ok,

                Err(err) => {
                    log::error!("Failed to save state: {err}");

                    Response::Error(format!("Failed to save state: {err}"))
                }
            }
        }
    };

    // Force every monitor thread to re-evaluate now instead of waiting for its
    // next poll tick, so a command takes effect immediately.
    wake.notify();

    if let Err(err) = zenbookd_ipc::send_message(&mut stream, &response) {
        log::error!("Error sending IPC response: {err}");
    }

    Ok(())
}
