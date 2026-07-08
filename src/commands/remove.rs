use crate::bridle_home;
use crate::cli::RemoveTarget;
use crate::mcp_config::McpConfig;
use crate::profile;
use crate::skills;
use clap::ValueEnum;

pub fn run(args: Vec<String>) {
    let (what, name) = match args.len() {
        1 => (RemoveTarget::Mcp, args[0].clone()),
        2 => {
            let what = RemoveTarget::from_str(&args[0], true).unwrap_or_else(|_| {
                eprintln!(
                    "Error: '{}' is not a valid remove target. Use mcp, skills, or all.",
                    args[0]
                );
                std::process::exit(1);
            });
            (what, args[1].clone())
        }
        _ => {
            eprintln!("Error: invalid arguments. Usage: bridle remove [mcp|skills|all] <name>");
            std::process::exit(1);
        }
    };

    let home = bridle_home();
    let remove_mcp = matches!(what, RemoveTarget::Mcp | RemoveTarget::All);
    let remove_skills = matches!(what, RemoveTarget::Skills | RemoveTarget::All);

    if remove_mcp {
        let mcp_path = profile::active_mcp_path(&home);
        let mut config = if mcp_path.exists() {
            let raw = std::fs::read_to_string(&mcp_path).unwrap_or_default();
            McpConfig::from_json(&raw).unwrap_or_default()
        } else {
            eprintln!("No master config at {}", mcp_path.display());
            return;
        };

        if config.remove_server(&name).is_some() {
            std::fs::write(&mcp_path, config.to_json_pretty().unwrap())
                .expect("Failed to write mcp.json");
            println!("✅ Removed MCP server '{}' from master config", name);
        } else {
            println!("⚠️  MCP server '{}' not found in master config", name);
        }
    }

    if remove_skills {
        let skills_dir = profile::active_skills_path(&home);
        match skills::remove_skill(&skills_dir, &name) {
            Ok(true) => println!("✅ Removed skill '{}' from ~/Bridle/skills/", name),
            Ok(false) => println!("⚠️  Skill '{}' not found in ~/Bridle/skills/", name),
            Err(e) => eprintln!("❌ Error removing skill '{}': {}", name, e),
        }
    }
}
