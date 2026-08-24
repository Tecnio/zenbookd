use zenbookd_ipc::ServiceStatus;

use crate::ui::{self, GAUGE_CELLS, Line, Panel, Row, Tone};

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

fn charge_tone(percent: u32) -> Tone {
    if percent <= 20 {
        Tone::Danger
    } else if percent <= 50 {
        Tone::Warn
    } else {
        Tone::Ok
    }
}

fn health_tone(percent: u32) -> Tone {
    if percent < 80 {
        Tone::Danger
    } else if percent < 90 {
        Tone::Warn
    } else {
        Tone::Ok
    }
}

fn reading(percent: u32, tone: Tone) -> Line {
    ui::gauge(percent, GAUGE_CELLS, tone)
        .plain("  ")
        .push(format!("{percent:>3}"), tone)
        .plain("%")
}

fn battery_rows(status: &ServiceStatus) -> Vec<Row> {
    let mut rows = Vec::new();

    if let Some(charge) = status.battery_charge {
        rows.push(ui::field("Charge", reading(charge, charge_tone(charge))));
    }

    if let Some(health) = status.battery_health {
        rows.push(ui::field("Health", reading(health, health_tone(health))));
    }

    rows
}

fn applied(status: &ServiceStatus) -> Option<Line> {
    let threshold = status.applied_threshold?;

    let tone = if threshold == status.charge_limit {
        Tone::Ok
    } else {
        Tone::Warn
    };

    let mut line = Line::new().push(threshold.to_string(), tone).plain("%");

    if status.boost_until.is_some() {
        line = line.push("  boost", Tone::Dim);
    } else if status.calibration_active {
        line = line.push("  calibrating", Tone::Dim);
    }

    Some(line)
}

fn periodic(status: &ServiceStatus) -> Line {
    if status.enable_periodic_full_charge {
        Line::new()
            .plain("every ")
            .push(status.full_charge_period.to_string(), Tone::Accent)
            .plain(" days")
    } else {
        Line::new().push("off", Tone::Dim)
    }
}

fn last_full_charge(status: &ServiceStatus) -> Line {
    let Some(timestamp) = status.last_full_charge else {
        return Line::new().push("never", Tone::Warn);
    };

    let days = (now() - timestamp) / 86_400;

    if days < 1 {
        Line::new().plain("today")
    } else if days < 2 {
        Line::new().plain("yesterday")
    } else {
        Line::new()
            .push(days.to_string(), Tone::Accent)
            .plain(" days ago")
    }
}

fn boost(status: &ServiceStatus) -> Line {
    let Some(until) = status.boost_until else {
        return Line::new().push("off", Tone::Dim);
    };

    let remaining = until - now();

    if remaining <= 0 {
        return Line::new().push("ending", Tone::Accent);
    }

    Line::new()
        .push((remaining / 3600).to_string(), Tone::Accent)
        .plain("h ")
        .push((remaining % 3600 / 60).to_string(), Tone::Accent)
        .plain("m left")
}

fn config_rows(status: &ServiceStatus) -> Vec<Row> {
    let mut rows = vec![ui::field(
        "Charge limit",
        Line::new()
            .push(status.charge_limit.to_string(), Tone::Ok)
            .plain("%"),
    )];

    if let Some(line) = applied(status) {
        rows.push(ui::field("Applied threshold", line));
    }

    rows.push(ui::field("Periodic full charge", periodic(status)));
    rows.push(ui::field("Last full charge", last_full_charge(status)));
    rows.push(ui::field("Boost", boost(status)));

    rows
}

pub fn panel(status: &ServiceStatus) -> Panel {
    Panel::new("zenbookd")
        .section(battery_rows(status))
        .section(config_rows(status))
}

pub fn report(status: &ServiceStatus) {
    panel(status).print();

    if let Some(error) = &status.config_error {
        eprintln!();

        ui::notice(
            "Failed to read the configuration file",
            error,
            Some(
                "The charge limit above is what the daemon is enforcing, and it does not \
                 match the file on disk. Fix /etc/zenbookd/config.toml, then: zenbookd reload",
            ),
        );
    }

    if let Some(error) = &status.threshold_error {
        eprintln!();

        ui::notice(
            "Failed to apply the charge threshold",
            error,
            Some(
                "The daemon may lack write access to the battery sysfs attribute. \
                 Check that the udev rule from scripts/ is installed.",
            ),
        );
    }
}
