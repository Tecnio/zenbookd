use std::{fs, path::PathBuf};

use crate::adapter::{AdapterError, AdapterReadError};

const POWER_SUPPLY: &str = "/sys/class/power_supply/";

const TYPE_KEY: &str = "type";
const TYPE_MAINS: &str = "Mains";

const ONLINE_KEY: &str = "online";

#[derive(Debug)]
pub struct Adapter {
    online: PathBuf,
}

impl Adapter {
    pub fn find() -> Result<Adapter, AdapterError> {
        let read_dir = fs::read_dir(POWER_SUPPLY)?;

        for entry in read_dir.filter_map(Result::ok) {
            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            path.file_name()
                .and_then(|s| s.to_str())
                .ok_or(AdapterError::NameParseError)?;

            let kind = fs::read_to_string(path.join(TYPE_KEY))?;

            if kind.trim() != TYPE_MAINS {
                continue;
            }

            let adapter = Adapter {
                online: path.join(ONLINE_KEY),
            };

            return Ok(adapter);
        }

        Err(AdapterError::NotFound)
    }

    pub fn online(&self) -> Result<bool, AdapterReadError> {
        let str = fs::read_to_string(&self.online)?;
        let online = str.trim().parse::<u32>()?;

        Ok(online == 1)
    }
}
