use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};

use crate::config::{Config, State};

pub const FULL_CAPACITY: u32 = 100;

#[derive(Debug, PartialEq)]
pub struct Decision {
    pub target_threshold: u32,
    pub state_dirty: bool,
}

#[derive(Debug, Default)]
pub struct FullChargeClock {
    since: Option<Instant>,
}

impl FullChargeClock {
    pub fn observe(&mut self, capacity: u32, at: Instant) -> Duration {
        if capacity < FULL_CAPACITY {
            self.since = None;

            return Duration::ZERO;
        }

        at.saturating_duration_since(*self.since.get_or_insert(at))
    }

    pub fn held(&self, at: Instant) -> Option<Duration> {
        self.since.map(|since| at.saturating_duration_since(since))
    }
}

pub fn needs_full_charge(cfg: &Config, state: &State, now: DateTime<Utc>) -> bool {
    if !cfg.enable_periodic_full_charge {
        return false;
    }

    match state.last_full_charge {
        Some(last) => (now - last).num_days() >= cfg.full_charge_period as i64,

        None => true,
    }
}

pub fn decide(
    cfg: &Config,
    state: &mut State,
    capacity: u32,
    held: Duration,
    now: DateTime<Utc>,
) -> Decision {
    let mut state_dirty = false;

    let hold = cfg.full_charge_hold();
    let calibrated = capacity >= FULL_CAPACITY && held >= hold;

    if calibrated
        && state
            .last_full_charge
            .is_none_or(|last| (now - last).to_std().is_ok_and(|since| since >= hold))
    {
        state.last_full_charge = Some(now);
        state_dirty = true;

        log::info!(
            "Battery held at full for {}m, updated last full charge",
            hold.as_secs() / 60
        );
    }

    let boost_active = match state.boost_until {
        Some(until) if now < until && !calibrated => true,

        Some(_) => {
            state.boost_until = None;
            state_dirty = true;

            log::info!("Boost finished, restoring charge limit");

            false
        }

        None => false,
    };

    let mut target_threshold = cfg.charge_limit;

    if needs_full_charge(cfg, state, now) {
        log::debug!("Periodic full charge needed, setting threshold to {FULL_CAPACITY}");
        target_threshold = FULL_CAPACITY;
    }

    if boost_active {
        log::debug!("Boost active, setting threshold to {FULL_CAPACITY}");
        target_threshold = FULL_CAPACITY;
    }

    Decision {
        target_threshold,
        state_dirty,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: Duration = Duration::from_secs(3600);

    fn now() -> DateTime<Utc> {
        "2026-01-15T12:00:00Z".parse().unwrap()
    }

    fn cfg(charge_limit: u32, periodic: bool, period: u32) -> Config {
        Config {
            charge_limit,
            enable_periodic_full_charge: periodic,
            full_charge_period: period,
            full_charge_hold: 60,
            disable_wifi_power_save_on_ac: true,
        }
    }

    #[test]
    fn records_first_full_charge_once_held() {
        let mut state = State::default();

        let decision = decide(&cfg(80, false, 90), &mut state, 100, HOUR, now());

        assert!(decision.state_dirty);
        assert_eq!(state.last_full_charge, Some(now()));
        assert_eq!(decision.target_threshold, 80);
    }

    #[test]
    fn full_charge_is_not_recorded_before_the_hold_elapses() {
        let mut state = State::default();

        let decision = decide(
            &cfg(80, false, 90),
            &mut state,
            100,
            Duration::from_secs(59 * 60),
            now(),
        );

        assert!(!decision.state_dirty);
        assert!(state.last_full_charge.is_none());
    }

    #[test]
    fn full_charge_is_not_recorded_below_full() {
        let mut state = State::default();

        let decision = decide(&cfg(80, false, 90), &mut state, 99, HOUR * 2, now());

        assert!(!decision.state_dirty);
        assert!(state.last_full_charge.is_none());
    }

    #[test]
    fn does_not_rewrite_full_charge_within_the_hold() {
        let mut state = State {
            last_full_charge: Some(now() - chrono::Duration::minutes(30)),
            ..Default::default()
        };
        let before = state.clone();

        let decision = decide(&cfg(80, false, 90), &mut state, 100, HOUR * 3, now());

        assert!(!decision.state_dirty);
        assert_eq!(state, before);
    }

    #[test]
    fn refreshes_full_charge_after_another_hold() {
        let mut state = State {
            last_full_charge: Some(now() - chrono::Duration::hours(2)),
            ..Default::default()
        };

        let decision = decide(&cfg(80, false, 90), &mut state, 100, HOUR * 3, now());

        assert!(decision.state_dirty);
        assert_eq!(state.last_full_charge, Some(now()));
    }

    #[test]
    fn active_boost_holds_threshold_at_100() {
        let mut state = State {
            last_full_charge: Some(now()),
            boost_until: Some(now() + chrono::Duration::hours(1)),
            ..Default::default()
        };

        let decision = decide(&cfg(80, false, 90), &mut state, 80, Duration::ZERO, now());

        assert_eq!(decision.target_threshold, 100);
        assert!(!decision.state_dirty);
        assert!(state.boost_until.is_some());
    }

    #[test]
    fn expired_boost_is_cleared_and_limit_restored() {
        let mut state = State {
            last_full_charge: Some(now()),
            boost_until: Some(now() - chrono::Duration::hours(1)),
            ..Default::default()
        };

        let decision = decide(&cfg(80, false, 90), &mut state, 80, Duration::ZERO, now());

        assert_eq!(decision.target_threshold, 80);
        assert!(decision.state_dirty);
        assert!(state.boost_until.is_none());
    }

    #[test]
    fn boost_keeps_holding_full_until_the_hold_elapses() {
        let mut state = State {
            last_full_charge: Some(now() - chrono::Duration::minutes(30)),
            boost_until: Some(now() + chrono::Duration::hours(1)),
            ..Default::default()
        };

        let decision = decide(
            &cfg(80, false, 90),
            &mut state,
            100,
            Duration::from_secs(30 * 60),
            now(),
        );

        assert_eq!(decision.target_threshold, 100);
        assert!(!decision.state_dirty);
        assert!(state.boost_until.is_some());
    }

    #[test]
    fn boost_ends_once_held_at_full_for_the_hold() {
        let mut state = State {
            last_full_charge: Some(now() - chrono::Duration::minutes(30)),
            boost_until: Some(now() + chrono::Duration::hours(1)),
            ..Default::default()
        };

        let decision = decide(&cfg(80, false, 90), &mut state, 100, HOUR, now());

        assert_eq!(decision.target_threshold, 80);
        assert!(decision.state_dirty);
        assert!(state.boost_until.is_none());
    }

    #[test]
    fn boost_honours_a_custom_hold() {
        let mut state = State {
            boost_until: Some(now() + chrono::Duration::hours(5)),
            ..Default::default()
        };
        let cfg = Config {
            full_charge_hold: 180,
            ..cfg(80, false, 90)
        };

        let decision = decide(&cfg, &mut state, 100, HOUR * 2, now());

        assert_eq!(decision.target_threshold, 100);
        assert!(state.boost_until.is_some());
    }

    #[test]
    fn periodic_charge_stays_at_full_until_the_hold_elapses() {
        let mut state = State {
            last_full_charge: Some(now() - chrono::Duration::days(91)),
            ..Default::default()
        };

        let decision = decide(
            &cfg(80, true, 90),
            &mut state,
            100,
            Duration::from_secs(10 * 60),
            now(),
        );

        assert_eq!(decision.target_threshold, 100);
        assert!(!decision.state_dirty);
    }

    #[test]
    fn periodic_charge_restores_the_limit_once_held() {
        let mut state = State {
            last_full_charge: Some(now() - chrono::Duration::days(91)),
            ..Default::default()
        };

        let decision = decide(&cfg(80, true, 90), &mut state, 100, HOUR, now());

        assert_eq!(decision.target_threshold, 80);
        assert!(decision.state_dirty);
        assert_eq!(state.last_full_charge, Some(now()));
    }

    #[test]
    fn periodic_charge_triggers_when_never_recorded() {
        let mut state = State::default();

        let decision = decide(&cfg(80, true, 90), &mut state, 50, Duration::ZERO, now());

        assert_eq!(decision.target_threshold, 100);
        assert!(!decision.state_dirty);
    }

    #[test]
    fn periodic_charge_triggers_once_the_period_elapses() {
        let mut state = State {
            last_full_charge: Some(now() - chrono::Duration::days(91)),
            ..Default::default()
        };

        let decision = decide(&cfg(80, true, 90), &mut state, 50, Duration::ZERO, now());

        assert_eq!(decision.target_threshold, 100);
    }

    #[test]
    fn periodic_charge_period_boundary_is_inclusive() {
        let mut state = State {
            last_full_charge: Some(now() - chrono::Duration::days(90)),
            ..Default::default()
        };

        let decision = decide(&cfg(80, true, 90), &mut state, 50, Duration::ZERO, now());

        assert_eq!(decision.target_threshold, 100);
    }

    #[test]
    fn periodic_charge_waits_until_the_period_elapses() {
        let mut state = State {
            last_full_charge: Some(now() - chrono::Duration::days(10)),
            ..Default::default()
        };

        let decision = decide(&cfg(80, true, 90), &mut state, 50, Duration::ZERO, now());

        assert_eq!(decision.target_threshold, 80);
    }

    #[test]
    fn periodic_charge_is_skipped_when_disabled() {
        let mut state = State {
            last_full_charge: Some(now() - chrono::Duration::days(400)),
            ..Default::default()
        };

        let decision = decide(&cfg(80, false, 90), &mut state, 50, Duration::ZERO, now());

        assert_eq!(decision.target_threshold, 80);
    }

    #[test]
    fn needs_full_charge_is_false_when_the_feature_is_disabled() {
        let state = State {
            last_full_charge: Some(now() - chrono::Duration::days(400)),
            ..Default::default()
        };

        assert!(!needs_full_charge(&cfg(80, false, 90), &state, now()));
    }

    #[test]
    fn needs_full_charge_is_true_when_never_recorded() {
        let state = State::default();

        assert!(needs_full_charge(&cfg(80, true, 90), &state, now()));
    }

    #[test]
    fn needs_full_charge_is_true_once_the_period_elapses() {
        let state = State {
            last_full_charge: Some(now() - chrono::Duration::days(91)),
            ..Default::default()
        };

        assert!(needs_full_charge(&cfg(80, true, 90), &state, now()));
    }

    #[test]
    fn needs_full_charge_is_false_before_the_period_elapses() {
        let state = State {
            last_full_charge: Some(now() - chrono::Duration::days(10)),
            ..Default::default()
        };

        assert!(!needs_full_charge(&cfg(80, true, 90), &state, now()));
    }

    #[test]
    fn a_zero_period_keeps_the_threshold_at_full() {
        let mut state = State {
            last_full_charge: Some(now()),
            ..Default::default()
        };

        let decision = decide(&cfg(80, true, 0), &mut state, 50, Duration::ZERO, now());

        assert_eq!(decision.target_threshold, 100);
    }

    #[test]
    fn a_zero_period_still_holds_full_after_recording_a_full_charge() {
        let mut state = State::default();

        let decision = decide(&cfg(80, true, 0), &mut state, 100, HOUR, now());

        assert_eq!(state.last_full_charge, Some(now()));
        assert_eq!(decision.target_threshold, 100);
    }

    #[test]
    fn clock_starts_counting_on_the_first_full_reading() {
        let mut clock = FullChargeClock::default();
        let start = Instant::now();

        assert_eq!(clock.held(start), None);
        assert_eq!(clock.observe(100, start), Duration::ZERO);
        assert_eq!(clock.observe(100, start + HOUR), HOUR);
        assert_eq!(clock.held(start + HOUR), Some(HOUR));
    }

    #[test]
    fn clock_resets_when_the_charge_drops_below_full() {
        let mut clock = FullChargeClock::default();
        let start = Instant::now();

        clock.observe(100, start);

        assert_eq!(clock.observe(99, start + HOUR), Duration::ZERO);
        assert_eq!(clock.held(start + HOUR), None);
        assert_eq!(clock.observe(100, start + HOUR * 2), Duration::ZERO);
        assert_eq!(clock.observe(100, start + HOUR * 3), HOUR);
    }
}
