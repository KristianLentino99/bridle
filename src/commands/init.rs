use crate::bridle_home;
use crate::profile;
use crate::sync::SyncState;

pub fn run() {
    let home = bridle_home();
    std::fs::create_dir_all(&home).expect("Failed to create ~/Bridle/");

    // Initialize profiles layout: default profile + active symlinks.
    profile::create_profile(&home, "default").expect("Failed to create default profile");
    profile::set_active_profile(&home, "default").expect("Failed to set active profile");
    profile::ensure_active_symlinks(&home).expect("Failed to create active symlinks");
    println!("✅ Created default profile");

    let config_path = home.join("config.json");
    if !config_path.exists() {
        let state = SyncState::default();
        state.save(&home).expect("Failed to write config.json");
        println!("✅ Created {}/config.json", home.display());
    }

    println!();
    println!("Use 'bridle add <name>' to add MCP servers, then 'bridle sync' to push.");
}
