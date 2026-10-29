//! Locating, loading and saving the settings file.
//!
//! `nubmiddle.ini` next to the exe wins if it exists (portable use, e.g. an exe kept in a
//! synced folder); otherwise the file lives in `%APPDATA%\nubmiddle\`, which also works when the
//! exe is installed in a read-only location such as Program Files.

use std::path::{Path, PathBuf};

use crate::config::Settings;

/// File name of the settings file.
pub const FILE_NAME: &str = "nubmiddle.ini";

/// Chooses the settings file from the exe folder and the per-user application data folder.
pub fn choose(exe_dir: Option<&Path>, appdata: Option<&Path>) -> PathBuf {
    if let Some(dir) = exe_dir {
        let portable = dir.join(FILE_NAME);
        if portable.is_file() {
            return portable;
        }
    }
    match appdata {
        Some(dir) => dir.join("nubmiddle").join(FILE_NAME),
        // No APPDATA (unusual): fall back to the exe folder
        None => exe_dir.unwrap_or(Path::new(".")).join(FILE_NAME),
    }
}

/// The settings file for the running exe.
pub fn default_path() -> PathBuf {
    let exe = std::env::current_exe().ok();
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from);
    choose(exe.as_deref().and_then(Path::parent), appdata.as_deref())
}

/// Loads settings from `path` over the defaults. A missing file just means defaults.
/// Returns the settings and warnings about lines that were skipped.
pub fn load(path: &Path) -> (Settings, Vec<String>) {
    let mut settings = Settings::default();
    let warnings = match std::fs::read_to_string(path) {
        Ok(text) => settings.apply_ini(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
        Err(e) => vec![format!("cannot read {}: {e}", path.display())],
    };
    (settings, warnings)
}

/// Saves settings to `path`, creating its folder if needed.
pub fn save(path: &Path, settings: &Settings) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, settings.to_ini())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gesture::MoveMode;

    /// A fresh, empty temporary folder for one test
    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("nubmiddle-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn portable_file_wins_over_appdata() {
        let dir = temp_dir("choose");
        let (exe_dir, appdata) = (dir.join("exe"), dir.join("appdata"));
        std::fs::create_dir_all(&exe_dir).unwrap();
        // Without a file next to the exe: the per-user folder
        assert_eq!(
            choose(Some(&exe_dir), Some(&appdata)),
            appdata.join("nubmiddle").join(FILE_NAME)
        );
        std::fs::write(exe_dir.join(FILE_NAME), "").unwrap();
        assert_eq!(
            choose(Some(&exe_dir), Some(&appdata)),
            exe_dir.join(FILE_NAME)
        );
        assert_eq!(choose(Some(&exe_dir), None), exe_dir.join(FILE_NAME));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_then_load() {
        let dir = temp_dir("save");
        let path = dir.join("sub").join(FILE_NAME);
        // A missing file gives the defaults without warnings
        assert_eq!(load(&path), (Settings::default(), vec![]));
        let mut s = Settings::default();
        s.gesture.mode = MoveMode::Drag;
        s.gesture.sensitivity_h = 5.0;
        save(&path, &s).unwrap();
        assert_eq!(load(&path), (s, vec![]));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
