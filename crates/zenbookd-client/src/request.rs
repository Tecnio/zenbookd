use std::{
    io::{self, ErrorKind},
    os::unix::net::UnixStream,
    time::Duration,
};

use thiserror::Error;
use zenbookd_ipc::{IpcError, Request, Response, socket_path};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("{source}")]
    Connect { path: String, source: io::Error },

    #[error("{0}")]
    Ipc(#[from] IpcError),
}

pub fn send_request(request: Request) -> Result<Response, ClientError> {
    let path = socket_path();

    let mut stream = UnixStream::connect(&path).map_err(|source| ClientError::Connect {
        path: path.clone(),
        source,
    })?;

    stream
        .set_read_timeout(Some(REQUEST_TIMEOUT))
        .map_err(IpcError::Io)?;

    stream
        .set_write_timeout(Some(REQUEST_TIMEOUT))
        .map_err(IpcError::Io)?;

    zenbookd_ipc::send_message(&mut stream, &request)?;

    Ok(zenbookd_ipc::receive_message(&mut stream)?)
}

pub fn connect_hint(kind: ErrorKind, path: &str) -> String {
    match kind {
        ErrorKind::NotFound => {
            "The service does not appear to be running. Try: systemctl status zenbookd.service"
                .to_string()
        }

        ErrorKind::PermissionDenied => format!(
            "The socket at {path} is group-owned by zenbookd. \
             Try: sudo usermod -aG zenbookd $USER, then log back in"
        ),

        ErrorKind::ConnectionRefused => {
            "The socket exists but nothing is listening. Try: systemctl restart zenbookd.service"
                .to_string()
        }

        _ => "Try: systemctl status zenbookd.service".to_string(),
    }
}
