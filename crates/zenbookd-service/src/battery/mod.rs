mod battery;
mod error;

pub use battery::Battery;
pub use error::{BatteryError, BatteryReadError, ThresholdSetError};
