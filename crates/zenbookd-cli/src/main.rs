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
        /// Charge limit percentage (0-100)
        #[arg(value_parser = clap::value_parser!(u32).range(0..=100))]
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

            let periodic_info = if status.enable_periodic_full_cycle {
                format!("Every {} days", status.full_cycle_period.to_string().cyan())
            } else {
                "Disabled".yellow().to_string()
            };

            println!("  {:<22} {}", "Periodic Full Cycle:".bold(), periodic_info);

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
        }

        Ok(Response::Ok) => {
            println!("{}", "✔ Command executed successfully.".green().bold());
        }

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
