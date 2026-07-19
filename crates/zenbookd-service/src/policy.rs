use chrono::{DateTime, Utc};

use crate::config::{Config, State};

#[derive(Debug, PartialEq)]
pub struct Decision {
    pub target_threshold: u32,
    pub state_dirty: bool,
}

pub fn decide(cfg: &Config, state: &mut State, capacity: u32, now: DateTime<Utc>) -> Decision {
    let mut state_dirty = false;

    if capacity >= 100 {
        // Avoid constant updates if staying at 100
        if state
            .last_full_charge
            .is_none_or(|last| (now - last).num_minutes() > 60)
        {
            state.last_full_charge = Some(now);
            state_dirty = true;

            log::info!("Updated last full charge timestamp");
        }
    }

    let boost_active = match state.boost_until {
        Some(until) if now < until && capacity < 100 => true,

        Some(_) => {
            state.boost_until = None;
            state_dirty = true;

            log::info!("Boost finished, restoring charge limit");
            false
        }

        None => false,
    };

    let mut target_threshold = cfg.charge_limit;

    if cfg.enable_periodic_full_charge {
        let needs_full_charge = match state.last_full_charge {
            Some(last) => {
                let days_since = (now - last).num_days();

                days_since >= cfg.full_charge_period as i64
            }

            None => true, // Never had a full charge or state lost
        };

        if needs_full_charge {
            log::debug!("Periodic full charge needed, setting threshold to 100");
            target_threshold = 100;
        }
    }

    if boost_active {
        log::debug!("Boost active, setting threshold to 100");
        target_threshold = 100;
    }

    Decision {
        target_threshold,
        state_dirty,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        "2026-01-15T12:00:00Z".parse().unwrap()
    }

    fn cfg(charge_limit: u32, periodic: bool, period: u32) -> Config {
        Config {
            charge_limit,
            enable_periodic_full_charge: periodic,
            full_charge_period: period,
            disable_wifi_power_save_on_ac: true,
        }
    }

    #[test]
    fn records_first_full_charge() {
        let mut state = State::default();

        let decision = decide(&cfg(80, false, 90), &mut state, 100, now());

        assert!(decision.state_dirty);
        assert_eq!(state.last_full_charge, Some(now()));
        assert_eq!(decision.target_threshold, 80);
    }

    #[test]
    fn does_not_rewrite_full_charge_within_the_hour() {
        let mut state = State {
            last_full_charge: Some(now() - chrono::Duration::minutes(30)),
            ..Default::default()
        };
        let before = state.clone();

        let decision = decide(&cfg(80, false, 90), &mut state, 100, now());

        assert!(!decision.state_dirty);
        assert_eq!(state, before);
    }

    #[test]
    fn refreshes_full_charge_after_an_hour() {
        let mut state = State {
            last_full_charge: Some(now() - chrono::Duration::hours(2)),
            ..Default::default()
        };

        let decision = decide(&cfg(80, false, 90), &mut state, 100, now());

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

        let decision = decide(&cfg(80, false, 90), &mut state, 80, now());

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

        let decision = decide(&cfg(80, false, 90), &mut state, 80, now());

        assert_eq!(decision.target_threshold, 80);
        assert!(decision.state_dirty);
        assert!(state.boost_until.is_none());
    }

    #[test]
    fn boost_ends_when_battery_reaches_full() {
        let mut state = State {
            last_full_charge: Some(now() - chrono::Duration::minutes(30)),
            boost_until: Some(now() + chrono::Duration::hours(1)),
            ..Default::default()
        };

        let decision = decide(&cfg(80, false, 90), &mut state, 100, now());

        assert_eq!(decision.target_threshold, 80);
        assert!(decision.state_dirty);
        assert!(state.boost_until.is_none());
    }

    #[test]
    fn periodic_charge_triggers_when_never_recorded() {
        let mut state = State::default();

        let decision = decide(&cfg(80, true, 90), &mut state, 50, now());

        assert_eq!(decision.target_threshold, 100);
        assert!(!decision.state_dirty);
    }

    #[test]
    fn periodic_charge_triggers_once_the_period_elapses() {
        let mut state = State {
            last_full_charge: Some(now() - chrono::Duration::days(91)),
            ..Default::default()
        };

        let decision = decide(&cfg(80, true, 90), &mut state, 50, now());

        assert_eq!(decision.target_threshold, 100);
    }

    #[test]
    fn periodic_charge_period_boundary_is_inclusive() {
        let mut state = State {
            last_full_charge: Some(now() - chrono::Duration::days(90)),
            ..Default::default()
        };

        let decision = decide(&cfg(80, true, 90), &mut state, 50, now());

        assert_eq!(decision.target_threshold, 100);
    }

    #[test]
    fn periodic_charge_waits_until_the_period_elapses() {
        let mut state = State {
            last_full_charge: Some(now() - chrono::Duration::days(10)),
            ..Default::default()
        };

        let decision = decide(&cfg(80, true, 90), &mut state, 50, now());

        assert_eq!(decision.target_threshold, 80);
    }

    #[test]
    fn periodic_charge_is_skipped_when_disabled() {
        let mut state = State {
            last_full_charge: Some(now() - chrono::Duration::days(400)),
            ..Default::default()
        };

        let decision = decide(&cfg(80, false, 90), &mut state, 50, now());

        assert_eq!(decision.target_threshold, 80);
    }
}
