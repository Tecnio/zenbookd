use std::{
    fs,
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::Path,
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use zenbookd_ipc::{Request, Response, ServiceStatus, socket_path};

use crate::{
    battery::Battery,
    config::{Config, State, save_config, save_state, validate_charge_limit},
    wake::Wake,
};

const BOOST_DURATION_HOURS: i64 = 24;
const CLIENT_TIMEOUT: Duration = Duration::from_secs(5);

pub fn run_server(
    config: Arc<RwLock<Config>>,
    battery: Arc<Battery>,
    state: Arc<Mutex<State>>,
    wake: Arc<Wake>,
    threshold_error: Arc<Mutex<Option<String>>>,
) -> std::io::Result<()> {
    let socket_path = socket_path();
    let path = Path::new(&socket_path);

    if path.exists() {
        fs::remove_file(path)?;
    }

    let listener = UnixListener::bind(path)?;

    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o666);
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

                if let Err(err) = handle_client(
                    stream,
                    Arc::clone(&config),
                    Arc::clone(&battery),
                    Arc::clone(&state),
                    Arc::clone(&wake),
                    Arc::clone(&threshold_error),
                ) {
                    log::error!("Error handling IPC client: {err}");
                }
            }

            Err(err) => {
                log::error!("IPC accept error: {err}");
            }
        }
    }

    Ok(())
}

fn handle_client(
    mut stream: UnixStream,
    config: Arc<RwLock<Config>>,
    battery: Arc<Battery>,
    state: Arc<Mutex<State>>,
    wake: Arc<Wake>,
    threshold_error: Arc<Mutex<Option<String>>>,
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
            })
        }

        Request::SetChargeLimit(limit) => {
            log::info!("Requested charge limit: {}", limit);

            if let Err(err) = validate_charge_limit(limit) {
                log::warn!("Rejected charge limit: {err}");

                Response::Error(err)
            } else {
                let mut config = config.write().unwrap();
                config.charge_limit = limit;

                let result = save_config(&config);
                drop(config);

                match result {
                    Ok(_) => Response::Ok,

                    Err(err) => {
                        log::error!("Failed to save config: {err}");

                        Response::Error(format!("Failed to save config: {err}"))
                    }
                }
            }
        }

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
