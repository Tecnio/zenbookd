use std::os::unix::net::UnixStream;

use clap::{Parser, Subcommand};
use colored::*;

use zenbookd_ipc::{Request, Response, socket_path};

#[derive(Debug, Parser)]
#[command(name = "zenbookd")]
#[command(about = "Zenbook Battery Daemon CLI", long_about = None)]
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
}

fn send_request(request: Request) -> Result<Response, Box<dyn std::error::Error>> {
    let mut stream = UnixStream::connect(socket_path())?;

    zenbookd_ipc::send_message(&mut stream, &request)?;
    let response: Response = zenbookd_ipc::receive_message(&mut stream)?;

    Ok(response)
}

fn main() {
    let cli = Cli::parse();

    let request = match &cli.command {
        Commands::Status => Request::GetStatus,
        Commands::SetLimit { limit } => Request::SetChargeLimit(*limit),
        Commands::Boost { stop } => Request::SetBoost(!stop),
    };

    match send_request(request) {
        Ok(Response::Status(status)) => {
            println!("{}", "── Battery Status ──".bold().cyan());

            if let Some(charge) = status.battery_charge {
                let charge_color = if charge <= 20 {
                    charge.to_string().red()
                } else if charge <= 50 {
                    charge.to_string().yellow()
                } else {
                    charge.to_string().green()
                };

                println!("  {:<22} {}%", "Current Charge:".bold(), charge_color);
            }

            if let Some(health) = status.battery_health {
                let health_color = if health < 80 {
                    health.to_string().red()
                } else {
                    health.to_string().green()
                };

                println!("  {:<22} {}%", "Battery Health:".bold(), health_color);
            }

            println!();

            println!("{}", "── Service Configuration ──".bold().cyan());

            println!(
                "  {:<22} {}%",
                "Charge Limit:".bold(),
                status.charge_limit.to_string().green()
            );

            if let Some(applied) = status.applied_threshold {
                let suffix = if status.boost_until.is_some() {
                    " (boost)"
                } else if status.calibration_active {
                    " (periodic calibration)"
                } else {
                    ""
                };

                let applied_color = if applied == status.charge_limit {
                    applied.to_string().green()
                } else {
                    applied.to_string().yellow()
                };

                println!(
                    "  {:<22} {}%{}",
                    "Applied Threshold:".bold(),
                    applied_color,
                    suffix
                );
            }

            let periodic_info = if status.enable_periodic_full_charge {
                format!(
                    "Every {} days",
                    status.full_charge_period.to_string().cyan()
                )
            } else {
                "Disabled".yellow().to_string()
            };

            println!("  {:<22} {}", "Periodic Full Charge:".bold(), periodic_info);

            let last_full_charge_info = match status.last_full_charge {
                None => "Never".yellow().to_string(),

                Some(ts) => {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);

                    let age_days = (now - ts) / 86_400;

                    if age_days < 1 {
                        "Today".to_string()
                    } else if age_days < 2 {
                        "Yesterday".to_string()
                    } else {
                        format!("{} days ago", age_days.to_string().cyan())
                    }
                }
            };

            println!(
                "  {:<22} {}",
                "Last Full Charge:".bold(),
                last_full_charge_info
            );

            let boost_info = match status.boost_until {
                Some(until) => {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);

                    let remaining = until - now;

                    if remaining > 0 {
                        let hours = remaining / 3600;
                        let minutes = (remaining % 3600) / 60;

                        format!(
                            "Active ({}h {}m left or until full)",
                            hours.to_string().cyan(),
                            minutes.to_string().cyan()
                        )
                    } else {
                        "Active".cyan().to_string()
                    }
                }

                None => "Inactive".yellow().to_string(),
            };

            println!("  {:<22} {}", "Boost:".bold(), boost_info);

            if let Some(err) = &status.threshold_error {
                println!();

                eprintln!(
                    "{} {}",
                    "⚠ Failed to apply the charge threshold:".red().bold(),
                    err
                );

                eprintln!(
                    "{}",
                    "The daemon may lack write access to the battery sysfs attribute; \
                     check that the udev rule from scripts/ is installed."
                        .yellow()
                );
            }
        }

        Ok(Response::Ok) => match &cli.command {
            Commands::SetLimit { limit } => {
                println!(
                    "{} Charge limit set to {}%.",
                    "✔".green().bold(),
                    limit.to_string().green().bold()
                );
            }

            Commands::Boost { stop: false } => {
                println!(
                    "{}",
                    "✔ Boost enabled — charging to 100% for 24h or until full, then restoring the limit."
                        .green()
                        .bold()
                );
            }

            Commands::Boost { stop: true } => {
                println!(
                    "{}",
                    "✔ Boost cancelled — charge limit restored.".green().bold()
                );
            }

            Commands::Status => {
                unreachable!("Status never returns Response::Ok");
            }
        },

        Ok(Response::Error(err)) => {
            eprintln!("{} {}", "✘ Error from service:".red().bold(), err);

            std::process::exit(1);
        }

        Err(err) => {
            eprintln!("{} {}", "✘ Failed to connect to service:".red().bold(), err);
            eprintln!("{}", "Make sure the service is running.".yellow());

            std::process::exit(1);
        }
    }
}
