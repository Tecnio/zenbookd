use std::time::{SystemTime, UNIX_EPOCH};

const DAY: i64 = 86_400;
const HOUR: i64 = 3_600;
const MINUTE: i64 = 60;

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

pub fn boost_remaining(until: i64, now: i64) -> String {
    let remaining = until - now;

    if remaining <= 0 {
        return "ending".to_string();
    }

    let hours = remaining / HOUR;
    let minutes = remaining % HOUR / MINUTE;

    if hours == 0 {
        format!("{minutes}m left")
    } else {
        format!("{hours}h {minutes}m left")
    }
}

pub fn since_full_charge(last: Option<i64>, now: i64) -> String {
    let Some(last) = last else {
        return "never".to_string();
    };

    match (now - last) / DAY {
        ..1 => "today".to_string(),
        1 => "yesterday".to_string(),
        days => format!("{days} days ago"),
    }
}

pub fn held_at_full(held_seconds: u64, hold_minutes: u32) -> String {
    let held = (held_seconds / 60).min(u64::from(hold_minutes));

    format!("{held} of {hold_minutes} min at 100%")
}

pub fn run_length(minutes: u32) -> String {
    if minutes == 0 {
        "until Esc".to_string()
    } else {
        format!("{minutes} min")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Down,
    Up,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub min: u32,
    pub max: u32,
    pub step: u32,
}

impl Range {
    pub fn step(self, value: u32, direction: Direction, fine: bool) -> u32 {
        let step = if fine { 1 } else { self.step };

        let next = match direction {
            Direction::Up => (value / step + 1) * step,
            Direction::Down => value.saturating_sub(1) / step * step,
        };

        next.clamp(self.min, self.max)
    }

    pub fn can_step(self, value: u32, direction: Direction) -> bool {
        match direction {
            Direction::Up => value < self.max,
            Direction::Down => value > self.min,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: Range = Range {
        min: 1,
        max: 100,
        step: 5,
    };

    #[test]
    fn coarse_steps_snap_to_the_grid() {
        assert_eq!(LIMIT.step(80, Direction::Up, false), 85);
        assert_eq!(LIMIT.step(83, Direction::Up, false), 85);
        assert_eq!(LIMIT.step(83, Direction::Down, false), 80);
        assert_eq!(LIMIT.step(80, Direction::Down, false), 75);
    }

    #[test]
    fn steps_stay_in_range() {
        assert_eq!(LIMIT.step(100, Direction::Up, false), 100);
        assert_eq!(LIMIT.step(5, Direction::Down, false), 1);
        assert_eq!(LIMIT.step(1, Direction::Down, true), 1);
        assert!(!LIMIT.can_step(100, Direction::Up));
        assert!(LIMIT.can_step(1, Direction::Up));
    }

    #[test]
    fn fine_steps_move_by_one() {
        assert_eq!(LIMIT.step(80, Direction::Up, true), 81);
        assert_eq!(LIMIT.step(80, Direction::Down, true), 79);
    }

    #[test]
    fn boost_counts_down_in_hours_and_minutes() {
        assert_eq!(
            boost_remaining(1_000 + 23 * HOUR + 9 * MINUTE, 1_000),
            "23h 9m left"
        );
        assert_eq!(boost_remaining(1_000 + 9 * MINUTE + 30, 1_000), "9m left");
        assert_eq!(boost_remaining(1_000, 1_000), "ending");
    }

    #[test]
    fn last_full_charge_reads_as_days() {
        let now = 100 * DAY;

        assert_eq!(since_full_charge(None, now), "never");
        assert_eq!(since_full_charge(Some(now - HOUR), now), "today");
        assert_eq!(since_full_charge(Some(now - DAY - HOUR), now), "yesterday");
        assert_eq!(since_full_charge(Some(now - 12 * DAY), now), "12 days ago");
    }

    #[test]
    fn held_time_caps_at_the_hold() {
        assert_eq!(held_at_full(20 * 60 + 59, 60), "20 of 60 min at 100%");
        assert_eq!(held_at_full(90 * 60, 60), "60 of 60 min at 100%");
    }

    #[test]
    fn zero_minutes_runs_until_esc() {
        assert_eq!(run_length(0), "until Esc");
        assert_eq!(run_length(15), "15 min");
    }
}
