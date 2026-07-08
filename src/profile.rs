//! Profile management — named sets of MCP servers, skills, and agents.
//!
//! Each profile lives under `~/Bridle/profiles/<name>/` and contains its own
//! `mcp.json` and `skills/` directory. The active profile is exposed through
//! symlinks at `~/Bridle/mcp.json` and `~/Bridle/skills/` so the rest of the
//! codebase can keep using `bridle_home()` unchanged.

use crate::mcp_config::McpConfig;
use crate::sync::SyncState;
use std::io;
use std::path::{Path, PathBuf};

/// Returns the directory that holds all profiles.
pub fn profiles_dir(bridle_home: &Path) -> PathBuf {
    bridle_home.join("profiles")
}

/// Returns the directory for a specific profile.
pub fn profile_dir(bridle_home: &Path, name: &str) -> PathBuf {
    profiles_dir(bridle_home).join(name)
}

/// Returns the canonical MCP config path for a profile.
pub fn profile_mcp_path(bridle_home: &Path, name: &str) -> PathBuf {
    profile_dir(bridle_home, name).join("mcp.json")
}

/// Returns the canonical skills directory for a profile.
pub fn profile_skills_path(bridle_home: &Path, name: &str) -> PathBuf {
    profile_dir(bridle_home, name).join("skills")
}

/// Returns the active profile symlink paths.
pub fn active_mcp_symlink(bridle_home: &Path) -> PathBuf {
    bridle_home.join("mcp.json")
}

pub fn active_skills_symlink(bridle_home: &Path) -> PathBuf {
    bridle_home.join("skills")
}

/// Returns the path to the watch-mode marker file.
pub fn watch_marker_path(bridle_home: &Path) -> PathBuf {
    bridle_home.join(".watch")
}

/// Returns true if the watch-mode marker file exists.
pub fn is_watching(bridle_home: &Path) -> bool {
    watch_marker_path(bridle_home).exists()
}

/// Create the watch-mode marker file.
pub fn start_watching(bridle_home: &Path) -> io::Result<()> {
    std::fs::File::create(watch_marker_path(bridle_home))?;
    Ok(())
}

/// Remove the watch-mode marker file.
pub fn stop_watching(bridle_home: &Path) -> io::Result<()> {
    let path = watch_marker_path(bridle_home);
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

/// Validates a profile name.
pub fn is_valid_profile_name(name: &str) -> bool {
    let max_len = 32;
    if name.is_empty() || name.len() > max_len {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Returns the active profile name from `config.json`, if any.
pub fn active_profile(bridle_home: &Path) -> Option<String> {
    let state = SyncState::load_or_default(bridle_home);
    state.active_profile
}

/// Set the active profile name in `config.json`.
pub fn set_active_profile(bridle_home: &Path, name: &str) -> io::Result<()> {
    let mut state = SyncState::load_or_default(bridle_home);
    state.active_profile = Some(name.to_string());
    state.save(bridle_home)
}

/// Create a new profile with empty `mcp.json` and `skills/`.
pub fn create_profile(bridle_home: &Path, name: &str) -> io::Result<()> {
    if !is_valid_profile_name(name) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("'{}' is not a valid profile name", name),
        ));
    }

    let dir = profile_dir(bridle_home, name);
    std::fs::create_dir_all(&dir)?;

    let mcp_path = profile_mcp_path(bridle_home, name);
    if !mcp_path.exists() {
        let default = McpConfig::new();
        let json = default.to_json_pretty().unwrap();
        std::fs::write(&mcp_path, json)?;
    }

    let skills_path = profile_skills_path(bridle_home, name);
    std::fs::create_dir_all(&skills_path)?;

    Ok(())
}

/// Ensure the legacy single-file layout is migrated to a `default` profile.
/// If `profiles/` already exists, this is a no-op.
pub fn migrate_legacy_layout(bridle_home: &Path) -> io::Result<Option<String>> {
    let profiles = profiles_dir(bridle_home);
    if profiles.exists() {
        return Ok(None);
    }

    std::fs::create_dir_all(&profiles)?;

    let default_dir = profile_dir(bridle_home, "default");
    std::fs::create_dir_all(&default_dir)?;

    let legacy_mcp = bridle_home.join("mcp.json");
    let profile_mcp = profile_mcp_path(bridle_home, "default");
    if legacy_mcp.exists() && !legacy_mcp.is_symlink() {
        std::fs::rename(&legacy_mcp, &profile_mcp)?;
    } else if !profile_mcp.exists() {
        let default = McpConfig::new();
        std::fs::write(&profile_mcp, default.to_json_pretty().unwrap())?;
    }

    let legacy_skills = bridle_home.join("skills");
    let profile_skills = profile_skills_path(bridle_home, "default");
    if legacy_skills.exists() && !legacy_skills.is_symlink() {
        std::fs::rename(&legacy_skills, &profile_skills)?;
    } else if !profile_skills.exists() {
        std::fs::create_dir_all(&profile_skills)?;
    }

    // Update state to mark default as active and create active symlinks.
    let mut state = SyncState::load_or_default(bridle_home);
    state.active_profile = Some("default".to_string());
    state.save(bridle_home)?;
    ensure_active_symlinks(bridle_home)?;

    Ok(Some("default".to_string()))
}

/// Repoint the active symlinks to the given profile.
pub fn activate_profile(bridle_home: &Path, name: &str) -> io::Result<()> {
    if !profile_dir(bridle_home, name).exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Profile '{}' does not exist", name),
        ));
    }

    let mcp_target = profile_mcp_path(bridle_home, name);
    let mcp_link = active_mcp_symlink(bridle_home);
    replace_with_symlink_or_copy(&mcp_target, &mcp_link)?;

    let skills_target = profile_skills_path(bridle_home, name);
    let skills_link = active_skills_symlink(bridle_home);
    replace_with_symlink_or_copy(&skills_target, &skills_link)?;

    set_active_profile(bridle_home, name)?;

    Ok(())
}

/// Ensure the active symlinks exist and point to the active profile.
/// Called by `init` after creating the default profile.
pub fn ensure_active_symlinks(bridle_home: &Path) -> io::Result<()> {
    let active = active_profile(bridle_home).unwrap_or_else(|| "default".to_string());
    activate_profile(bridle_home, &active)
}

/// Remove an existing path (file, dir, or symlink) and replace it with a
/// symlink to `target`. Falls back to copying on platforms where symlinks are
/// not available.
fn replace_with_symlink_or_copy(target: &Path, link: &Path) -> io::Result<()> {
    // Remove whatever is currently at the link path.
    if link.exists() || link.is_symlink() {
        let meta = std::fs::symlink_metadata(link)?;
        if meta.is_dir() {
            std::fs::remove_dir_all(link)?;
        } else {
            std::fs::remove_file(link)?;
        }
    }

    create_symlink_or_copy(target, link)
}

#[cfg(unix)]
fn create_symlink_or_copy(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(not(unix))]
fn create_symlink_or_copy(target: &Path, link: &Path) -> io::Result<()> {
    let meta = std::fs::metadata(target)?;
    if meta.is_dir() {
        copy_dir_all(target, link)
    } else {
        std::fs::copy(target, link).map(|_| ())
    }
}

#[cfg(not(unix))]
fn copy_dir_all(src: &Path, dst: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let meta = entry.metadata()?;
        if meta.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
