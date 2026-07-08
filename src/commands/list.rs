use crate::bridle_home;
use crate::mcp_config::McpConfig;
use crate::profile;

pub fn run() {
    let home = bridle_home();
    let mcp_path = profile::active_mcp_path(&home);

    let config = if mcp_path.exists() {
        let raw = std::fs::read_to_string(&mcp_path).unwrap_or_default();
        McpConfig::from_json(&raw).unwrap_or_default()
    } else {
        println!("No master config at {}", mcp_path.display());
        return;
    };

    let names = config.server_names();
    if names.is_empty() {
        println!("No MCP servers configured. Use 'bridle add <name>' to add one.");
    } else {
        println!("MCP servers in master config:");
        for name in names {
            if let Some(server) = config.mcp_servers.get(name) {
                let kind = if server.url.is_some() { "http" } else { "cmd" };
                println!("  📡 {} ({})", name, kind);
            }
        }
    }
}
