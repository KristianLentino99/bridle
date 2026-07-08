pub mod adapters;
pub mod harness;
pub mod mcp_config;
pub mod platform;
pub mod skills;
pub mod sync;

/// Bridle home directory — `~/Bridle/` on all platforms.
///
/// Can be overridden with the `BRIDLE_HOME` environment variable for tests
/// or for running bridle against a non-standard config directory.
pub fn bridle_home() -> std::path::PathBuf {
    if let Some(override_path) = std::env::var_os("BRIDLE_HOME") {
        return std::path::PathBuf::from(override_path);
    }

    dirs::home_dir()
        .expect("Could not determine home directory")
        .join("Bridle")
}
