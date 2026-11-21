mod completions;
mod status;
mod ui;

use std::{io, process::ExitCode};

use clap::{
    Parser, Subcommand,
    builder::styling::{AnsiColor, Effects, Style, Styles},
};
use zenbookd_client::{
    ClientError, connect_hint,
    refresh::{self, Pattern, RefreshError, Resolution},
    send_request,
};
use zenbookd_ipc::{Request, Response};

const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Green.on_default())
    .placeholder(AnsiColor::White.on_default().effects(Effects::DIMMED))
    .error(AnsiColor::Red.on_default().effects(Effects::BOLD))
    .valid(AnsiColor::Green.on_default())
    .invalid(AnsiColor::Yellow.on_default());

const HEADER: Style = AnsiColor::Cyan.on_default().effects(Effects::BOLD);
const LITERAL: Style = AnsiColor::Green.on_default();

fn examples() -> String {
    let commands = [
        ("status", "show charge, health and configuration"),
        ("set-limit 80", "hold the battery at 80%"),
        ("boost", "charge to 100% now, restore the limit after"),
        ("set-charge-period 30", "days between periodic full charges"),
        (
            "set-charge-hold 60",
            "minutes to sit at 100% for a full charge",
        ),
        ("reload", "re-read config.toml without restarting"),
        (
            "pixel-refresh cycle",
            "flash solid screens to clear burn-in",
        ),
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
    #[command(about = "Show the current battery and service status")]
    Status,

    #[command(about = "Set the maximum battery charge limit")]
    SetLimit {
        #[arg(
            help = "Charge limit percentage (1-100)",
            value_parser = clap::value_parser!(u32).range(1..=100)
        )]
        limit: u32,
    },

    #[command(
        about = "Charge to 100% and hold it there for the charge hold, then restore the limit"
    )]
    Boost {
        #[arg(
            long,
            help = "Cancel an active boost and restore the charge limit immediately"
        )]
        stop: bool,
    },

    #[command(about = "Enable or disable the periodic full charge")]
    SetPeriodicCharge {
        #[arg(value_enum)]
        state: Toggle,
    },

    #[command(about = "Set how many days between periodic full charges")]
    SetChargePeriod {
        #[arg(value_parser = clap::value_parser!(u32).range(1..=365))]
        days: u32,
    },

    #[command(about = "Set how many minutes the battery has to sit at 100% for a full charge")]
    SetChargeHold {
        #[arg(value_parser = clap::value_parser!(u32).range(1..=720))]
        minutes: u32,
    },

    #[command(about = "Disable Wi-Fi power saving while on AC power")]
    SetWifiPowerSave {
        #[arg(value_enum)]
        state: Toggle,
    },

    #[command(about = "Re-read /etc/zenbookd/config.toml without restarting the service")]
    Reload,

    #[command(about = "Play a fullscreen pattern to clear OLED image retention")]
    PixelRefresh {
        #[arg(value_enum, default_value = "noise")]
        pattern: PatternArg,

        #[arg(
            long,
            help = "Stop after this many minutes instead of waiting for q or Esc",
            value_parser = clap::value_parser!(u32).range(1..=720)
        )]
        minutes: Option<u32>,

        #[arg(
            long,
            default_value_t = 5,
            help = "Seconds each color is held in the cycle pattern",
            value_parser = clap::value_parser!(u32).range(1..=600)
        )]
        hold: u32,

        #[arg(
            long,
            value_name = "WxH",
            help = "Screen size to draw at instead of the detected native mode"
        )]
        size: Option<Resolution>,
    },

    #[command(hide = true)]
    Completions,
}

#[derive(Clone, Copy, Debug, PartialEq, clap::ValueEnum)]
enum PatternArg {
    #[value(help = "Fullscreen random noise, fast")]
    Noise,

    #[value(help = "Black, red, green, blue and white screens in turn, slow")]
    Cycle,
}

impl From<PatternArg> for Pattern {
    fn from(arg: PatternArg) -> Self {
        match arg {
            PatternArg::Noise => Pattern::Noise,
            PatternArg::Cycle => Pattern::Cycle,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, clap::ValueEnum)]
enum Toggle {
    On,
    Off,
}

fn confirmation(command: &Commands) -> String {
    match command {
        Commands::SetLimit { limit } => format!("Charge limit set to {limit}%"),

        Commands::Boost { stop: false } => {
            "Boost enabled, holding 100% for the charge hold or for 24h".to_string()
        }

        Commands::Boost { stop: true } => "Boost cancelled, charge limit restored".to_string(),

        Commands::SetPeriodicCharge { state: Toggle::On } => {
            "Periodic full charge enabled".to_string()
        }

        Commands::SetPeriodicCharge { state: Toggle::Off } => {
            "Periodic full charge disabled".to_string()
        }

        Commands::SetChargePeriod { days } => format!("Charge period set to {days} days"),

        Commands::SetChargeHold { minutes } => format!("Charge hold set to {minutes} minutes"),

        Commands::SetWifiPowerSave { state: Toggle::On } => {
            "Wi-Fi power saving disabled on AC".to_string()
        }

        Commands::SetWifiPowerSave { state: Toggle::Off } => {
            "Wi-Fi power saving left untouched on AC".to_string()
        }

        Commands::Reload => "Configuration reloaded".to_string(),

        Commands::Status | Commands::PixelRefresh { .. } | Commands::Completions => {
            unreachable!("status, pixel-refresh and completions never return Response::Ok")
        }
    }
}

fn report_connect_failure(path: &str, source: &std::io::Error) {
    let hint = connect_hint(source.kind(), path);

    ui::failure(
        "Cannot reach the zenbookd service",
        &source.to_string(),
        Some(&hint),
    );
}

fn pixel_refresh(
    pattern: Pattern,
    size: Option<Resolution>,
    hold: u32,
    minutes: Option<u32>,
) -> ExitCode {
    let size = match size.map_or_else(refresh::detect_resolution, Ok) {
        Ok(size) => size,

        Err(err) => {
            ui::failure(
                "Cannot find the screen size",
                &err.to_string(),
                Some("Pass it explicitly, e.g. --size 2880x1800"),
            );

            return ExitCode::FAILURE;
        }
    };

    ui::success(&format!(
        "Pixel refresh started at {size}, press q or Esc to stop"
    ));

    match refresh::run(pattern, size, hold, minutes) {
        Ok(()) => {
            ui::success("Pixel refresh finished");

            ExitCode::SUCCESS
        }

        Err(err @ RefreshError::FfplayMissing) => {
            ui::failure(
                "Cannot run the pixel refresh",
                &err.to_string(),
                Some("Install ffmpeg, which provides ffplay"),
            );

            ExitCode::FAILURE
        }

        Err(err) => {
            ui::failure("Pixel refresh failed", &err.to_string(), None);

            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let request = match &cli.command {
        Commands::Completions => {
            return match completions::write_fish(&mut io::stdout()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(_) => ExitCode::FAILURE,
            };
        }
        Commands::PixelRefresh {
            pattern,
            minutes,
            hold,
            size,
        } => return pixel_refresh((*pattern).into(), *size, *hold, *minutes),
        Commands::Status => Request::GetStatus,
        Commands::SetLimit { limit } => Request::SetChargeLimit(*limit),
        Commands::Boost { stop } => Request::SetBoost(!stop),
        Commands::SetPeriodicCharge { state } => {
            Request::SetPeriodicFullCharge(*state == Toggle::On)
        }
        Commands::SetChargePeriod { days } => Request::SetFullChargePeriod(*days),
        Commands::SetChargeHold { minutes } => Request::SetFullChargeHold(*minutes),
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

        Err(ClientError::Connect { path, source }) => {
            report_connect_failure(&path, &source);

            ExitCode::FAILURE
        }

        Err(ClientError::Ipc(err)) => {
            ui::failure(
                "Lost contact with the zenbookd service",
                &err.to_string(),
                Some("Try: systemctl status zenbookd.service"),
            );

            ExitCode::FAILURE
        }
    }
}
