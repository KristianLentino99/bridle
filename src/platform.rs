use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOS,
    Linux,
    Windows,
}

impl Platform {
    /// Lowercase platform name.
    pub fn name(&self) -> &'static str {
        match self {
            Platform::MacOS => "macos",
            Platform::Linux => "linux",
            Platform::Windows => "windows",
        }
    }
}

/// Detect the current platform at runtime.
pub fn detect() -> Platform {
    if cfg!(target_os = "macos") {
        Platform::MacOS
    } else if cfg!(target_os = "linux") {
        Platform::Linux
    } else if cfg!(target_os = "windows") {
        Platform::Windows
    } else {
        Platform::MacOS // fallback for dev/test on unknown platforms
    }
}

/// Platform-appropriate config/data directory.
///
/// - macOS: `~/Library/Application Support/`
/// - Linux: `$XDG_CONFIG_HOME` or `~/.config/`
/// - Windows: `%APPDATA%`
pub fn config_dir() -> PathBuf {
    match detect() {
        Platform::MacOS => dirs::home_dir()
            .expect("Could not determine home directory")
            .join("Library")
            .join("Application Support"),
        Platform::Linux => {
            dirs::config_dir().unwrap_or_else(|| dirs::home_dir().unwrap().join(".config"))
        }
        Platform::Windows => dirs::config_dir().expect("Could not determine AppData directory"),
    }
}

/// Home directory (`~` on unix, `%USERPROFILE%` on Windows).
///
/// The `BRIDLE_TEST_HOME` environment variable can override the real home
/// directory so integration tests can create fake harness directories without
/// touching the user's actual config files.
pub fn home_dir() -> PathBuf {
    if let Some(override_path) = std::env::var_os("BRIDLE_TEST_HOME") {
        return PathBuf::from(override_path);
    }

    dirs::home_dir().expect("Could not determine home directory")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_name_is_lowercase() {
        assert_eq!(Platform::MacOS.name(), "macos");
        assert_eq!(Platform::Linux.name(), "linux");
        assert_eq!(Platform::Windows.name(), "windows");
    }

    #[test]
    fn detect_returns_current_platform() {
        let platform = detect();
        assert_eq!(platform, Platform::MacOS);
    }

    #[test]
    fn config_dir_is_absolute() {
        let dir = config_dir();
        assert!(dir.is_absolute());
        assert!(dir.starts_with("/"));
    }

    #[test]
    fn home_dir_is_absolute() {
        let dir = home_dir();
        assert!(dir.is_absolute());
        assert!(dir.starts_with("/"));
    }
}
