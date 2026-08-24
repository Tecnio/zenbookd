mod status;
mod ui;

use std::{io::ErrorKind, os::unix::net::UnixStream, process::ExitCode, time::Duration};

use clap::{
    Parser, Subcommand,
    builder::styling::{AnsiColor, Effects, Style, Styles},
};
use thiserror::Error;
use zenbookd_ipc::{Request, Response, socket_path};

const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Green.on_default())
    .placeholder(AnsiColor::White.on_default().effects(Effects::DIMMED))
    .error(AnsiColor::Red.on_default().effects(Effects::BOLD))
    .valid(AnsiColor::Green.on_default())
    .invalid(AnsiColor::Yellow.on_default());

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

const HEADER: Style = AnsiColor::Cyan.on_default().effects(Effects::BOLD);
const LITERAL: Style = AnsiColor::Green.on_default();

fn examples() -> String {
    let commands = [
        ("status", "show charge, health and configuration"),
        ("set-limit 80", "hold the battery at 80%"),
        ("boost", "charge to 100% now, restore the limit after"),
        ("set-charge-period 30", "days between periodic full charges"),
        ("reload", "re-read config.toml without restarting"),
    ];

    let mut out = format!("{HEADER}Examples:{HEADER:#}");

    for (command, note) in commands {
        let padding = " ".repeat(24usize.saturating_sub(command.len()));

        out.push_str(&format!(
            "\n  {LITERAL}zenbookd {command}{LITERAL:#}{padding}{note}"
        ));
    }

    out
}

#[derive(Debug, Parser)]
#[command(name = "zenbookd")]
#[command(version)]
#[command(about = "Zenbook battery daemon CLI", long_about = None)]
#[command(styles = STYLES)]
#[command(after_help = examples())]
#[command(arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Show the current battery and service status
    Status,

    /// Set the maximum battery charge limit
    SetLimit {
        /// Charge limit percentage (1-100)
        #[arg(value_parser = clap::value_parser!(u32).range(1..=100))]
        limit: u32,
    },

    /// Charge to 100% for the next 24 hours or until full, then restore the limit
    Boost {
        /// Cancel an active boost and restore the charge limit immediately
        #[arg(long)]
        stop: bool,
    },

    /// Enable or disable the periodic full charge
    SetPeriodicCharge {
        #[arg(value_enum)]
        state: Toggle,
    },

    /// Set how many days between periodic full charges
    SetChargePeriod {
        #[arg(value_parser = clap::value_parser!(u32).range(1..=365))]
        days: u32,
    },

    /// Disable Wi-Fi power saving while on AC power
    SetWifiPowerSave {
        #[arg(value_enum)]
        state: Toggle,
    },

    /// Re-read /etc/zenbookd/config.toml without restarting the service
    Reload,
}

#[derive(Debug, Clone, Copy, PartialEq, clap::ValueEnum)]
enum Toggle {
    On,
    Off,
}

#[derive(Debug, Error)]
enum CliError {
    #[error("{source}")]
    Connect {
        path: String,
        source: std::io::Error,
    },

    #[error("{0}")]
    Ipc(#[from] zenbookd_ipc::IpcError),
}

fn send_request(request: Request) -> Result<Response, CliError> {
    let path = socket_path();

    let mut stream = UnixStream::connect(&path).map_err(|source| CliError::Connect {
        path: path.clone(),
        source,
    })?;

    stream
        .set_read_timeout(Some(REQUEST_TIMEOUT))
        .map_err(zenbookd_ipc::IpcError::Io)?;

    stream
        .set_write_timeout(Some(REQUEST_TIMEOUT))
        .map_err(zenbookd_ipc::IpcError::Io)?;

    zenbookd_ipc::send_message(&mut stream, &request)?;

    Ok(zenbookd_ipc::receive_message(&mut stream)?)
}

fn confirmation(command: &Commands) -> String {
    match command {
        Commands::SetLimit { limit } => format!("Charge limit set to {limit}%"),

        Commands::Boost { stop: false } => {
            "Boost enabled, charging to 100% until full or for 24h".to_string()
        }

        Commands::Boost { stop: true } => "Boost cancelled, charge limit restored".to_string(),

        Commands::SetPeriodicCharge { state: Toggle::On } => {
            "Periodic full charge enabled".to_string()
        }

        Commands::SetPeriodicCharge { state: Toggle::Off } => {
            "Periodic full charge disabled".to_string()
        }

        Commands::SetChargePeriod { days } => format!("Charge period set to {days} days"),

        Commands::SetWifiPowerSave { state: Toggle::On } => {
            "Wi-Fi power saving disabled on AC".to_string()
        }

        Commands::SetWifiPowerSave { state: Toggle::Off } => {
            "Wi-Fi power saving left untouched on AC".to_string()
        }

        Commands::Reload => "Configuration reloaded".to_string(),

        Commands::Status => unreachable!("status never returns Response::Ok"),
    }
}

fn report_connect_failure(path: &str, source: &std::io::Error) {
    let hint = match source.kind() {
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
    };

    ui::failure(
        "Cannot reach the zenbookd service",
        &source.to_string(),
        Some(&hint),
    );
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let request = match &cli.command {
        Commands::Status => Request::GetStatus,
        Commands::SetLimit { limit } => Request::SetChargeLimit(*limit),
        Commands::Boost { stop } => Request::SetBoost(!stop),
        Commands::SetPeriodicCharge { state } => {
            Request::SetPeriodicFullCharge(*state == Toggle::On)
        }
        Commands::SetChargePeriod { days } => Request::SetFullChargePeriod(*days),
        Commands::SetWifiPowerSave { state } => Request::SetWifiPowerSave(*state == Toggle::On),
        Commands::Reload => Request::ReloadConfig,
    };

    match send_request(request) {
        Ok(Response::Status(status)) => {
            status::report(&status);

            ExitCode::SUCCESS
        }

        Ok(Response::Ok) => {
            ui::success(&confirmation(&cli.command));

            ExitCode::SUCCESS
        }

        Ok(Response::Error(err)) => {
            ui::failure("Service rejected the request", &err, None);

            ExitCode::FAILURE
        }

        Err(CliError::Connect { path, source }) => {
            report_connect_failure(&path, &source);

            ExitCode::FAILURE
        }

        Err(CliError::Ipc(err)) => {
            ui::failure(
                "Lost contact with the zenbookd service",
                &err.to_string(),
                Some("Try: systemctl status zenbookd.service"),
            );

            ExitCode::FAILURE
        }
    }
}
