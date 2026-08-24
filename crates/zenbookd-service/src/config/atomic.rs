use std::{
    fs::{self, File},
    io::{self, Write},
    path::Path,
};

pub fn write(path: &Path, data: &str) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;

    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config");

    let temp = parent.join(format!(".{name}.tmp"));

    let result = replace(&temp, path, data);

    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }

    result
}

fn replace(temp: &Path, path: &Path, data: &str) -> io::Result<()> {
    let mut file = File::create(temp)?;

    file.write_all(data.as_bytes())?;
    file.sync_all()?;

    drop(file);

    fs::rename(temp, path)?;

    if let Some(parent) = path.parent() {
        File::open(parent)?.sync_all()?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_the_previous_contents() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");

        write(&path, "first").unwrap();
        write(&path, "second").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "second");
    }

    #[test]
    fn leaves_no_temporary_behind() {
        let tmp = tempfile::tempdir().unwrap();

        write(&tmp.path().join("config.toml"), "value").unwrap();

        let leftovers: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .filter(|name| name != "config.toml")
            .collect();

        assert!(leftovers.is_empty(), "left behind {leftovers:?}");
    }

    #[test]
    fn creates_missing_parent_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nested").join("config.toml");

        write(&path, "value").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "value");
    }
}
