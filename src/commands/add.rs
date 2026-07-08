use crate::bridle_home;
use crate::mcp_config::{McpConfig, McpServer};
use crate::profile;
use std::collections::BTreeMap;

pub fn run(
    name: String,
    command: Option<String>,
    args: Vec<String>,
    url: Option<String>,
    env_pairs: Vec<String>,
) {
    let home = bridle_home();
    let mcp_path = profile::active_mcp_path(&home);

    let mut config = if mcp_path.exists() {
        let raw = std::fs::read_to_string(&mcp_path).unwrap_or_default();
        McpConfig::from_json(&raw).unwrap_or_default()
    } else {
        McpConfig::new()
    };

    let env = if env_pairs.is_empty() {
        None
    } else {
        let mut map = BTreeMap::new();
        for pair in &env_pairs {
            if let Some((k, v)) = pair.split_once('=') {
                map.insert(k.to_string(), v.to_string());
            }
        }
        if map.is_empty() {
            None
        } else {
            Some(map)
        }
    };

    let server = McpServer {
        url,
        command,
        args: if args.is_empty() { None } else { Some(args) },
        env,
        headers: None,
    };

    config.add_server(&name, server);

    std::fs::create_dir_all(&home).ok();
    std::fs::write(&mcp_path, config.to_json_pretty().unwrap()).expect("Failed to write mcp.json");
    println!("✅ Added '{}' to master config", name);
    println!("   Run 'bridle sync' to push to all harnesses.");
}
