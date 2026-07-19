use std::{
    fs,
    path::{Path, PathBuf},
};

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
        Self::find_in(Path::new(POWER_SUPPLY))
    }

    fn find_in(root: &Path) -> Result<Adapter, AdapterError> {
        let mut paths: Vec<PathBuf> = fs::read_dir(root)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();

        paths.sort();

        for path in paths {
            if !path.is_dir() {
                continue;
            }

            // Not every power supply node exposes `type`, and one that doesn't
            // must not end the search for the ones that do.
            let Ok(kind) = fs::read_to_string(path.join(TYPE_KEY)) else {
                continue;
            };

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

#[cfg(test)]
mod tests {
    use super::*;

    fn supply(root: &Path, name: &str, attrs: &[(&str, &str)]) {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();

        for (key, value) in attrs {
            fs::write(dir.join(key), value).unwrap();
        }
    }

    #[test]
    fn finds_the_mains_supply() {
        let tmp = tempfile::tempdir().unwrap();

        supply(tmp.path(), "BAT0", &[("type", "Battery\n")]);
        supply(
            tmp.path(),
            "ADP1",
            &[("type", "Mains\n"), ("online", "1\n")],
        );

        let adapter = Adapter::find_in(tmp.path()).unwrap();

        assert!(adapter.online().unwrap());
    }

    #[test]
    fn skips_entries_without_a_type_attribute() {
        let tmp = tempfile::tempdir().unwrap();

        supply(tmp.path(), "aaa_no_type", &[]);
        supply(
            tmp.path(),
            "zzz_mains",
            &[("type", "Mains\n"), ("online", "0\n")],
        );

        let adapter = Adapter::find_in(tmp.path()).unwrap();

        assert!(!adapter.online().unwrap());
    }

    #[test]
    fn reports_not_found_without_a_mains_supply() {
        let tmp = tempfile::tempdir().unwrap();

        supply(tmp.path(), "BAT0", &[("type", "Battery\n")]);

        assert!(matches!(
            Adapter::find_in(tmp.path()),
            Err(AdapterError::NotFound)
        ));
    }
}
