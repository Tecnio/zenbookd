use std::{path::PathBuf, sync::Mutex};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::{ConfigLoadError, ConfigSaveError, atomic};

const FLUSH_ATTEMPTS: u32 = 8;

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub last_full_charge: Option<DateTime<Utc>>,

    #[serde(default)]
    pub boost_until: Option<DateTime<Utc>>,

    #[serde(default)]
    pub wifi_power_save_restore: Option<bool>,
}

#[derive(Debug)]
pub struct PersistentState {
    data: State,
    dirty: bool,
}

impl PersistentState {
    pub fn new(data: State) -> Self {
        Self { data, dirty: false }
    }

    pub fn data(&self) -> &State {
        &self.data
    }

    pub fn data_mut(&mut self) -> &mut State {
        &mut self.data
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn sync_after_write(&mut self, written: &State) {
        self.dirty = self.data != *written;
    }
}

pub fn load_state() -> Result<State, ConfigLoadError> {
    let path = state_path();

    if !path.is_file() {
        return Err(ConfigLoadError::NotFound);
    }

    let data = std::fs::read_to_string(&path)?;
    let state = toml::from_str::<State>(&data)?;

    Ok(state)
}

pub fn save_state(state: &State) -> Result<(), ConfigSaveError> {
    let data = toml::to_string_pretty(&state)?;

    atomic::write(&state_path(), &data).map_err(Into::into)
}

pub fn flush_state(state: &Mutex<PersistentState>) {
    for _ in 0..FLUSH_ATTEMPTS {
        let snapshot = {
            let guard = state.lock().unwrap();

            if !guard.dirty {
                return;
            }

            guard.data.clone()
        };

        match save_state(&snapshot) {
            Ok(()) => {
                let mut guard = state.lock().unwrap();

                guard.sync_after_write(&snapshot);

                if !guard.dirty {
                    return;
                }
            }

            Err(err) => {
                log::error!("Failed to save state: {err}");
                return;
            }
        }
    }
}

fn state_path() -> PathBuf {
    let directory = std::env::var("STATE_DIR")
        .ok()
        .and_then(|v| v.split(':').next().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/var/lib/zenbookd"));

    directory.join("state.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_document_yields_all_none() {
        assert_eq!(toml::from_str::<State>("").unwrap(), State::default());
    }

    #[test]
    fn missing_fields_deserialize_as_none() {
        let state: State = toml::from_str("wifi_power_save_restore = true").unwrap();

        assert_eq!(state.wifi_power_save_restore, Some(true));
        assert!(state.last_full_charge.is_none());
        assert!(state.boost_until.is_none());
    }

    #[test]
    fn installed_state_seed_loads() {
        let state: State = toml::from_str("last_full_charge = \"2026-01-15T12:00:00Z\"").unwrap();

        assert!(state.last_full_charge.is_some());
    }

    #[test]
    fn roundtrips_through_toml() {
        let state = State {
            last_full_charge: Some("2026-01-15T12:00:00Z".parse().unwrap()),
            boost_until: Some("2026-01-16T12:00:00Z".parse().unwrap()),
            wifi_power_save_restore: Some(false),
        };

        let text = toml::to_string_pretty(&state).unwrap();

        assert_eq!(toml::from_str::<State>(&text).unwrap(), state);
    }

    #[test]
    fn sync_after_write_clears_dirty_when_unchanged() {
        let mut state = PersistentState::new(State::default());
        state.mark_dirty();

        let snapshot = state.data().clone();
        state.sync_after_write(&snapshot);

        assert!(!state.dirty);
    }

    #[test]
    fn sync_after_write_keeps_dirty_when_diverged() {
        let mut state = PersistentState::new(State::default());
        let snapshot = state.data().clone();

        state.data_mut().wifi_power_save_restore = Some(true);
        state.sync_after_write(&snapshot);

        assert!(state.dirty);
    }
}
