use std::{
    fs,
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::Path,
    sync::{Arc, RwLock},
};

use zenbookd_ipc::{Request, Response, ServiceStatus, socket_path};

use crate::{
    battery::Battery,
    config::{Config, load_state, save_config, save_state},
    wake::Wake,
};

const BOOST_DURATION_HOURS: i64 = 24;

pub fn run_server(
    config: Arc<RwLock<Config>>,
    battery: Arc<Battery>,
    wake: Arc<Wake>,
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
                if let Err(err) = handle_client(
                    stream,
                    Arc::clone(&config),
                    Arc::clone(&battery),
                    Arc::clone(&wake),
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
    wake: Arc<Wake>,
) -> std::io::Result<()> {
    let request: Request = match zenbookd_ipc::receive_message(&mut stream) {
        Ok(req) => req,

        Err(err) => {
            if let zenbookd_ipc::IpcError::Json(err) = err {
                let response = Response::Error(format!("Invalid request: {err}"));

                let _ = zenbookd_ipc::send_message(&mut stream, &response);
            }

            return Ok(());
        }
    };

    let response = match request {
        Request::GetStatus => {
            let config = config.read().unwrap();

            let boost_until = load_state()
                .unwrap_or_default()
                .boost_until
                .map(|until| until.timestamp());

            Response::Status(ServiceStatus {
                charge_limit: config.charge_limit,

                enable_periodic_full_charge: config.enable_periodic_full_charge,
                full_charge_period: config.full_charge_period,

                battery_health: battery.health().ok(),
                battery_charge: battery.capacity().ok(),

                boost_until,
            })
        }

        Request::SetChargeLimit(limit) => {
            log::info!("Requested charge limit: {}", limit);

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

        Request::SetBoost(enable) => {
            let mut state = load_state().unwrap_or_default();

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
