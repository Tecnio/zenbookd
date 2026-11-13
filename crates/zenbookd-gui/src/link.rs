use zenbookd_client::{ClientError, connect_hint, sanitize};
use zenbookd_ipc::{Response, ServiceStatus};

const SERVICE_HINT: &str = "Try: systemctl status zenbookd.service";

#[derive(Debug, Clone, PartialEq)]
pub struct Fault {
    pub cause: String,
    pub hint: String,
}

impl Fault {
    pub fn from_error(error: &ClientError) -> Self {
        match error {
            ClientError::Connect { path, source } => Self {
                cause: sanitize(&source.to_string()),
                hint: connect_hint(source.kind(), path),
            },

            ClientError::Ipc(err) => Self {
                cause: sanitize(&err.to_string()),
                hint: SERVICE_HINT.to_string(),
            },
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Link {
    Connecting,
    Online(Box<ServiceStatus>),
    Unreachable(Fault),
}

impl Link {
    pub fn from_poll(result: Result<Response, ClientError>) -> Self {
        match result {
            Ok(Response::Status(status)) => Self::Online(Box::new(status)),

            Ok(Response::Error(message)) => Self::Unreachable(Fault {
                cause: sanitize(&message),
                hint: SERVICE_HINT.to_string(),
            }),

            Ok(Response::Ok) => Self::Unreachable(Fault {
                cause: "the service answered a status request without a status".to_string(),
                hint: SERVICE_HINT.to_string(),
            }),

            Err(error) => Self::Unreachable(Fault::from_error(&error)),
        }
    }

    pub fn status(&self) -> Option<&ServiceStatus> {
        match self {
            Self::Online(status) => Some(status),
            Self::Connecting | Self::Unreachable(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Failure,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub severity: Severity,
    pub headline: String,
    pub detail: String,
}

impl Notice {
    pub fn info(headline: impl Into<String>) -> Self {
        Self {
            severity: Severity::Info,
            headline: headline.into(),
            detail: String::new(),
        }
    }

    pub fn failure(headline: impl Into<String>, detail: &str) -> Self {
        Self {
            severity: Severity::Failure,
            headline: headline.into(),
            detail: sanitize(detail),
        }
    }

    fn warning(headline: &str, detail: &str) -> Self {
        Self {
            severity: Severity::Warning,
            headline: headline.to_string(),
            detail: sanitize(detail),
        }
    }
}

pub fn status_notices(status: &ServiceStatus) -> Vec<Notice> {
    let config = status
        .config_error
        .as_deref()
        .map(|error| Notice::warning("Config file could not be read", error));

    let threshold = status
        .threshold_error
        .as_deref()
        .map(|error| Notice::warning("Charge threshold could not be applied", error));

    config.into_iter().chain(threshold).collect()
}

pub fn command_notice(result: &Result<Response, ClientError>) -> Option<Notice> {
    match result {
        Ok(Response::Ok | Response::Status(_)) => None,

        Ok(Response::Error(message)) => {
            Some(Notice::failure("Service rejected the change", message))
        }

        Err(error) => Some(Notice::failure(
            "Lost contact with the service",
            &Fault::from_error(error).cause,
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, ErrorKind};

    use super::*;

    fn status() -> ServiceStatus {
        ServiceStatus {
            charge_limit: 80,
            enable_periodic_full_charge: true,
            full_charge_period: 30,
            battery_health: Some(94),
            battery_charge: Some(72),
            boost_until: None,
            applied_threshold: Some(80),
            last_full_charge: None,
            calibration_active: false,
            full_charge_hold: Some(60),
            held_at_full: None,
            threshold_error: None,
            config_error: None,
            wifi_power_save_on_ac: Some(true),
        }
    }

    #[test]
    fn a_status_reply_is_online() {
        let link = Link::from_poll(Ok(Response::Status(status())));

        assert_eq!(link.status().map(|status| status.charge_limit), Some(80));
    }

    #[test]
    fn a_missing_socket_names_the_service() {
        let error = ClientError::Connect {
            path: "/run/zenbookd/zenbookd.sock".to_string(),
            source: io::Error::from(ErrorKind::NotFound),
        };

        let link = Link::from_poll(Err(error));

        let Link::Unreachable(fault) = link else {
            panic!("expected an unreachable link");
        };

        assert!(fault.hint.contains("systemctl status"));
    }

    #[test]
    fn a_permission_error_points_at_the_group() {
        let error = ClientError::Connect {
            path: "/run/zenbookd/zenbookd.sock".to_string(),
            source: io::Error::from(ErrorKind::PermissionDenied),
        };

        let fault = Fault::from_error(&error);

        assert!(fault.hint.contains("usermod -aG zenbookd"));
    }

    #[test]
    fn daemon_errors_become_warnings_in_order() {
        let mut status = status();
        status.config_error = Some("line 3: bad\nvalue".to_string());
        status.threshold_error = Some("permission denied".to_string());

        let notices = status_notices(&status);

        assert_eq!(notices.len(), 2);
        assert_eq!(notices[0].detail, "line 3: bad value");
        assert_eq!(notices[1].severity, Severity::Warning);
    }

    #[test]
    fn a_rejected_command_is_a_failure() {
        let notice = command_notice(&Ok(Response::Error("out of range".to_string())));

        assert_eq!(
            notice.map(|notice| notice.severity),
            Some(Severity::Failure)
        );
        assert_eq!(command_notice(&Ok(Response::Ok)), None);
    }
}
