mod config;
mod error;
mod state;

pub use config::{Config, load_config, save_config};
pub use error::{ConfigLoadError, ConfigSaveError};

// State is implicitly used via load_state
// but the structure is not directly used.
#[allow(unused_imports)]
pub use state::{State, load_state, save_state};
