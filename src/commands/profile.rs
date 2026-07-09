use crate::bridle_home;
use crate::cli::ProfileCommands;
use crate::profile;
use std::io::{self, Write};

pub fn run(command: ProfileCommands) {
    let home = bridle_home();

    // Ensure legacy layout is migrated before any profile operation.
    profile::migrate_legacy_layout(&home).ok();

    match command {
        ProfileCommands::Create { name } => cmd_create(&home, &name),
        ProfileCommands::List => cmd_list(&home),
        ProfileCommands::Switch { name, no_sync } => cmd_switch(&home, &name, no_sync),
        ProfileCommands::Remove { name } => cmd_remove(&home, &name),
        ProfileCommands::Rename { old, new } => cmd_rename(&home, &old, &new),
        ProfileCommands::Clone { from, to } => cmd_clone(&home, &from, &to),
    }
}

fn cmd_create(home: &std::path::Path, name: &str) {
    match profile::create_profile(home, name) {
        Ok(()) => {
            println!("✅ Created profile '{}'", name);
        }
        Err(e) => {
            eprintln!("❌ Failed to create profile '{}': {}", name, e);
            std::process::exit(1);
        }
    }
}

fn cmd_list(home: &std::path::Path) {
    let active = profile::active_profile(home);
    let profiles_dir = profile::profiles_dir(home);

    if !profiles_dir.exists() {
        println!("No profiles found.");
        return;
    }

    println!("Profiles:");
    match std::fs::read_dir(&profiles_dir) {
        Ok(entries) => {
            let mut names: Vec<String> = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect();
            names.sort();

            if names.is_empty() {
                println!("  (none)");
            } else {
                for name in names {
                    let marker = if active.as_ref() == Some(&name) {
                        " *"
                    } else {
                        ""
                    };
                    println!("  {}{}", name, marker);
                }
            }
        }
        Err(e) => {
            eprintln!("❌ Failed to list profiles: {}", e);
            std::process::exit(1);
        }
    }

    if active.is_none() {
        println!();
        println!("No active profile set.");
    }
}

fn cmd_switch(home: &std::path::Path, name: &str, no_sync: bool) {
    // Watch-mode guard.
    if profile::is_watching(home) {
        eprintln!("⚠️  Warning: bridle sync --watch appears to be running.");
        eprintln!("   Switching profiles while watching may cause unexpected syncs.");
        if !confirm("Continue? [y/N] ") {
            println!("Cancelled.");
            return;
        }
    }

    match profile::activate_profile(home, name) {
        Ok(()) => {
            println!("✅ Switched to profile '{}'", name);
        }
        Err(e) => {
            eprintln!("❌ Failed to switch profile: {}", e);
            std::process::exit(1);
        }
    }

    if !no_sync {
        if confirm("Sync now? [Y/n] ") {
            crate::commands::sync::run(false, false, false, false);
        } else {
            println!("   Run 'bridle sync' when ready.");
        }
    }

    if profile::is_watching(home) {
        println!("💡 Remember to restart 'bridle sync --watch' to monitor the new profile.");
    }
}

fn cmd_remove(home: &std::path::Path, name: &str) {
    if name == "default" {
        eprintln!("❌ Cannot remove the 'default' profile.");
        std::process::exit(1);
    }

    if profile::active_profile(home).as_ref() == Some(&name.to_string()) {
        eprintln!("❌ Cannot remove the active profile. Switch to another profile first.");
        std::process::exit(1);
    }

    let dir = profile::profile_dir(home, name);
    if !dir.exists() {
        println!("⚠️  Profile '{}' not found", name);
        return;
    }

    match std::fs::remove_dir_all(&dir) {
        Ok(()) => {
            println!("✅ Removed profile '{}'", name);
        }
        Err(e) => {
            eprintln!("❌ Failed to remove profile '{}': {}", name, e);
            std::process::exit(1);
        }
    }
}

fn cmd_rename(home: &std::path::Path, old: &str, new: &str) {
    if !profile::is_valid_profile_name(new) {
        eprintln!(
            "❌ '{}' is not a valid profile name. Use alphanumeric, '-' or '_'.",
            new
        );
        std::process::exit(1);
    }

    let old_dir = profile::profile_dir(home, old);
    let new_dir = profile::profile_dir(home, new);

    if !old_dir.exists() {
        eprintln!("❌ Profile '{}' does not exist", old);
        std::process::exit(1);
    }

    if new_dir.exists() {
        eprintln!("❌ Profile '{}' already exists", new);
        std::process::exit(1);
    }

    let was_active = profile::active_profile(home).as_ref() == Some(&old.to_string());

    match std::fs::rename(&old_dir, &new_dir) {
        Ok(()) => {
            if was_active {
                profile::set_active_profile(home, new).ok();
                profile::ensure_active_symlinks(home).ok();
            }
            println!("✅ Renamed profile '{}' to '{}'", old, new);
        }
        Err(e) => {
            eprintln!("❌ Failed to rename profile '{}': {}", old, e);
            std::process::exit(1);
        }
    }
}

fn cmd_clone(home: &std::path::Path, from: &str, to: &str) {
    if !profile::is_valid_profile_name(to) {
        eprintln!(
            "❌ '{}' is not a valid profile name. Use alphanumeric, '-' or '_'.",
            to
        );
        std::process::exit(1);
    }

    let from_dir = profile::profile_dir(home, from);
    let to_dir = profile::profile_dir(home, to);

    if !from_dir.exists() {
        eprintln!("❌ Profile '{}' does not exist", from);
        std::process::exit(1);
    }

    if to_dir.exists() {
        eprintln!("❌ Profile '{}' already exists", to);
        std::process::exit(1);
    }

    match copy_profile(home, from, to) {
        Ok(()) => {
            println!("✅ Cloned profile '{}' to '{}'", from, to);
        }
        Err(e) => {
            eprintln!("❌ Failed to clone profile '{}': {}", from, e);
            std::process::exit(1);
        }
    }
}

fn copy_profile(home: &std::path::Path, from: &str, to: &str) -> Result<(), std::io::Error> {
    profile::create_profile(home, to)?;

    let from_mcp = profile::profile_mcp_path(home, from);
    let to_mcp = profile::profile_mcp_path(home, to);
    if from_mcp.exists() {
        std::fs::copy(&from_mcp, &to_mcp)?;
    }

    let from_skills = profile::profile_skills_path(home, from);
    let to_skills = profile::profile_skills_path(home, to);
    if from_skills.exists() {
        copy_dir_all(&from_skills, &to_skills)?;
    }

    Ok(())
}

fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> Result<(), std::io::Error> {
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

fn confirm(prompt: &str) -> bool {
    print!("{} ", prompt);
    io::stdout().flush().unwrap();
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return false;
    }
    let trimmed = input.trim().to_lowercase();
    trimmed.is_empty() || trimmed == "y" || trimmed == "yes"
}
