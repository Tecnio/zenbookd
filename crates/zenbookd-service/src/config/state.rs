use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::{ConfigLoadError, ConfigSaveError};

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub last_full_cycle: Option<DateTime<Utc>>,

    #[serde(default)]
    pub boost_until: Option<DateTime<Utc>>,
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
    let path = state_path();

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let data = toml::to_string_pretty(&state)?;

    std::fs::write(path, &data).map_err(Into::into)
}

fn state_path() -> PathBuf {
    let directory = std::env::var("STATE_DIR")
        .ok()
        .and_then(|v| v.split(':').next().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/var/lib/zenbookd"));

    directory.join("state.toml")
}
