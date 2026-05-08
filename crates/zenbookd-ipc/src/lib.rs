use std::io::{Read, Write};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IpcError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, IpcError>;

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    GetStatus,
    SetChargeLimit(u32),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub charge_limit: u32,

    pub enable_periodic_full_cycle: bool,
    pub full_cycle_period: u32,

    pub battery_health: Option<u32>,
    pub battery_charge: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    Status(ServiceStatus),
    Ok,
    Error(String),
}

pub const DEFAULT_SOCKET_PATH: &str = "/run/zenbookd.sock";

pub fn socket_path() -> String {
    std::env::var("ZENBOOKD_SOCKET").unwrap_or_else(|_| DEFAULT_SOCKET_PATH.to_string())
}

pub fn send_message<W: Write, T: Serialize>(mut writer: W, message: &T) -> Result<()> {
    let json = serde_json::to_vec(message)?;
    let len = json.len() as u32;

    writer.write_all(&len.to_be_bytes())?;
    writer.write_all(&json)?;

    writer.flush()?;

    Ok(())
}

pub fn receive_message<R: Read, T: for<'de> Deserialize<'de>>(mut reader: R) -> Result<T> {
    let mut len_bytes = [0u8; 4];
    reader.read_exact(&mut len_bytes)?;
    let len = u32::from_be_bytes(len_bytes) as usize;

    let mut buffer = vec![0u8; len];
    reader.read_exact(&mut buffer)?;

    let message = serde_json::from_slice(&buffer)?;
    Ok(message)
}
