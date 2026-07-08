use crate::bridle_home;
use crate::cli::ImportTarget;
use crate::harness;
use crate::mcp_config::McpConfig;
use crate::platform;
use crate::profile;
use crate::skills;
use crate::sync;
use std::path::PathBuf;

pub fn run(
    what: ImportTarget,
    harness_id: String,
    all: bool,
    force: bool,
    link: bool,
    update: bool,
    source: Option<PathBuf>,
) {
    let import_mcp = matches!(what, ImportTarget::Mcp | ImportTarget::All);
    let import_skills = matches!(what, ImportTarget::Skills | ImportTarget::All);

    if import_mcp {
        cmd_import_mcp(&harness_id, all, force);
    }

    if import_skills {
        cmd_import_skills(source, force, link, update);
    }

    println!();
    println!("   Run 'bridle sync' to push to all harnesses.");
}

fn cmd_import_mcp(harness_id: &str, all: bool, force: bool) {
    let plat = platform::detect();
    let home = bridle_home();
    let mcp_path = profile::active_mcp_path(&home);

    // Load or create master
    let mut master = if mcp_path.exists() {
        let raw = std::fs::read_to_string(&mcp_path).unwrap_or_default();
        McpConfig::from_json(&raw).unwrap_or_default()
    } else {
        McpConfig::new()
    };

    let mut imported = 0;
    let mut skipped = 0;
    let mut conflicts: Vec<String> = vec![];

    // Determine which harnesses to import from
    let targets: Vec<&harness::HarnessSpec> = if all || harness_id == "all" {
        harness::all().iter().collect()
    } else {
        match harness::all().iter().find(|s| s.id == harness_id) {
            Some(s) => vec![s],
            None => {
                eprintln!(
                    "Unknown harness '{}'. Use 'bridle discover' to see available harnesses.",
                    harness_id
                );
                return;
            }
        }
    };

    for spec in targets {
        // Skip if not installed
        if !spec.base_dir(plat).exists() {
            println!("⚠️  {} — not installed, skipping", spec.id);
            continue;
        }

        let adapter = match sync::adapter_for(spec) {
            Some(a) => a,
            None => {
                println!("❌ {} — no adapter available", spec.id);
                continue;
            }
        };

        let harness_config = match adapter.read_config(plat) {
            Ok(c) => c,
            Err(e) => {
                println!("⚠️  {} — error reading config: {}", spec.id, e);
                continue;
            }
        };

        if harness_config.mcp_servers.is_empty() {
            println!("⏭️  {} — no MCP servers configured", spec.id);
            continue;
        }

        println!("📥 Importing MCP from {}:", spec.id);
        for (name, server) in &harness_config.mcp_servers {
            if master.mcp_servers.contains_key(name) {
                if force {
                    println!("   🔄 {} — overwritten (--force)", name);
                    master.mcp_servers.insert(name.clone(), server.clone());
                    imported += 1;
                } else {
                    println!(
                        "   🔀 {} — already in master, skipped (use --force to overwrite)",
                        name
                    );
                    conflicts.push(format!("{} (from {})", name, spec.id));
                    skipped += 1;
                }
            } else {
                println!("   ✅ {} — imported", name);
                master.mcp_servers.insert(name.clone(), server.clone());
                imported += 1;
            }
        }
    }

    // Save
    std::fs::create_dir_all(&home).ok();
    std::fs::write(&mcp_path, master.to_json_pretty().unwrap()).expect("Failed to write mcp.json");

    println!();
    println!(
        "📊 MCP import summary: {} imported, {} skipped",
        imported, skipped
    );
    if !conflicts.is_empty() && !force {
        println!("💡 Conflicts: {}", conflicts.join(", "));
        println!("   Run 'bridle import mcp --force' to overwrite existing servers.");
    }
}

fn cmd_import_skills(source: Option<PathBuf>, force: bool, link: bool, update: bool) {
    let source = source.unwrap_or_else(|| platform::home_dir().join(".agents").join("skills"));
    let target = profile::active_skills_path(&bridle_home());

    if !source.exists() {
        eprintln!(
            "Source skills directory does not exist: {}",
            source.display()
        );
        std::process::exit(1);
    }

    match skills::import_skills(&source, &target, force, link, update) {
        Ok(report) => {
            for name in &report.imported {
                println!("✅ Imported skill: {}", name);
            }
            for name in &report.skipped {
                println!("⏭️  Skipped (already in bridle): {}", name);
            }
            for (name, err) in &report.errors {
                println!("❌ Error importing {}: {}", name, err);
            }

            println!();
            println!(
                "📊 Skills import summary: {} imported, {} skipped, {} errors",
                report.imported.len(),
                report.skipped.len(),
                report.errors.len()
            );
            if !report.skipped.is_empty() && !force && !update {
                println!("💡 Run 'bridle import skills --update' to refresh changed skills, or '--force' to overwrite all.");
            }
        }
        Err(e) => {
            eprintln!("Failed to import skills: {}", e);
            std::process::exit(1);
        }
    }
}
