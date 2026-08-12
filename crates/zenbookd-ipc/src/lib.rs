use std::io::{Read, Write};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IpcError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Message too large: {0} bytes (maximum {MAX_MESSAGE_LEN})")]
    MessageTooLarge(usize),
}

pub type Result<T> = std::result::Result<T, IpcError>;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub enum Request {
    GetStatus,
    SetChargeLimit(u32),
    SetBoost(bool),
    SetPeriodicFullCharge(bool),
    SetFullChargePeriod(u32),
    SetWifiPowerSave(bool),
    ReloadConfig,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub charge_limit: u32,

    pub enable_periodic_full_charge: bool,
    pub full_charge_period: u32,

    pub battery_health: Option<u32>,
    pub battery_charge: Option<u32>,

    pub boost_until: Option<i64>,

    #[serde(default)]
    pub applied_threshold: Option<u32>,

    #[serde(default)]
    pub last_full_charge: Option<i64>,

    #[serde(default)]
    pub calibration_active: bool,

    #[serde(default)]
    pub threshold_error: Option<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub enum Response {
    Status(ServiceStatus),
    Ok,
    Error(String),
}

pub const DEFAULT_SOCKET_PATH: &str = "/run/zenbookd/zenbookd.sock";
pub const MAX_MESSAGE_LEN: usize = 64 * 1024;

pub fn socket_path() -> String {
    std::env::var("ZENBOOKD_SOCKET").unwrap_or_else(|_| DEFAULT_SOCKET_PATH.to_string())
}

pub fn send_message<W: Write, T: Serialize>(mut writer: W, message: &T) -> Result<()> {
    let json = serde_json::to_vec(message)?;

    if json.len() > MAX_MESSAGE_LEN {
        return Err(IpcError::MessageTooLarge(json.len()));
    }

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

    if len > MAX_MESSAGE_LEN {
        return Err(IpcError::MessageTooLarge(len));
    }

    let mut buffer = vec![0u8; len];
    reader.read_exact(&mut buffer)?;

    let message = serde_json::from_slice(&buffer)?;
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_a_request() {
        let mut buf = Vec::new();
        send_message(&mut buf, &Request::SetChargeLimit(80)).unwrap();

        let decoded: Request = receive_message(&buf[..]).unwrap();

        assert_eq!(decoded, Request::SetChargeLimit(80));
    }

    #[test]
    fn roundtrips_the_new_config_requests() {
        let mut buf = Vec::new();
        send_message(&mut buf, &Request::SetFullChargePeriod(30)).unwrap();

        let decoded: Request = receive_message(&buf[..]).unwrap();
        assert_eq!(decoded, Request::SetFullChargePeriod(30));

        let mut buf = Vec::new();
        send_message(&mut buf, &Request::ReloadConfig).unwrap();

        let decoded: Request = receive_message(&buf[..]).unwrap();
        assert_eq!(decoded, Request::ReloadConfig);
    }

    #[test]
    fn roundtrips_a_status_response() {
        let status = ServiceStatus {
            charge_limit: 80,
            enable_periodic_full_charge: true,
            full_charge_period: 30,
            battery_health: Some(94),
            battery_charge: Some(72),
            boost_until: Some(1_768_000_000),
            applied_threshold: Some(100),
            last_full_charge: Some(1_767_000_000),
            calibration_active: true,
            threshold_error: Some("permission denied".to_string()),
        };

        let mut buf = Vec::new();
        send_message(&mut buf, &Response::Status(status)).unwrap();

        let decoded: Response = receive_message(&buf[..]).unwrap();

        assert!(matches!(decoded, Response::Status(s) if s.applied_threshold == Some(100)));
    }

    #[test]
    fn status_from_an_older_daemon_deserializes_with_defaults() {
        let json = r#"{"Status":{"charge_limit":80,"enable_periodic_full_charge":true,"full_charge_period":30,"battery_health":94,"battery_charge":72,"boost_until":null}}"#;

        let decoded: Response = serde_json::from_str(json).unwrap();

        match decoded {
            Response::Status(s) => {
                assert_eq!(s.applied_threshold, None);
                assert!(!s.calibration_active);
            }

            _ => panic!("expected Response::Status"),
        }
    }

    #[test]
    fn length_prefix_matches_payload_length() {
        let mut buf = Vec::new();
        send_message(&mut buf, &Request::GetStatus).unwrap();

        let len = u32::from_be_bytes(buf[..4].try_into().unwrap()) as usize;

        assert_eq!(len, buf.len() - 4);
    }

    #[test]
    fn truncated_frame_is_an_io_error() {
        let mut buf = Vec::new();
        send_message(&mut buf, &Request::GetStatus).unwrap();
        buf.pop();

        let err = receive_message::<_, Request>(&buf[..]).unwrap_err();

        assert!(matches!(err, IpcError::Io(_)));
    }

    #[test]
    fn malformed_body_is_a_json_error() {
        let body = b"not json";

        let mut buf = (body.len() as u32).to_be_bytes().to_vec();
        buf.extend_from_slice(body);

        let err = receive_message::<_, Request>(&buf[..]).unwrap_err();

        assert!(matches!(err, IpcError::Json(_)));
    }

    #[test]
    fn oversized_length_prefix_is_rejected() {
        let mut buf = ((MAX_MESSAGE_LEN + 1) as u32).to_be_bytes().to_vec();
        buf.extend_from_slice(b"{}");

        let err = receive_message::<_, Request>(&buf[..]).unwrap_err();

        assert!(matches!(err, IpcError::MessageTooLarge(_)));
    }

    #[test]
    fn absurd_length_prefix_is_rejected_without_allocating() {
        let mut buf = u32::MAX.to_be_bytes().to_vec();
        buf.extend_from_slice(b"{}");

        let err = receive_message::<_, Request>(&buf[..]).unwrap_err();

        assert!(matches!(err, IpcError::MessageTooLarge(_)));
    }

    #[test]
    fn send_rejects_an_oversized_payload_without_writing_a_partial_frame() {
        let huge = Response::Error("x".repeat(MAX_MESSAGE_LEN));

        let mut buf = Vec::new();
        let err = send_message(&mut buf, &huge).unwrap_err();

        assert!(matches!(err, IpcError::MessageTooLarge(_)));
        assert!(buf.is_empty());
    }
}
