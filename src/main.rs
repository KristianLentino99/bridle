use bridle::adapters;
use bridle::bridle_home;
use bridle::harness;
use bridle::mcp_config::{McpConfig, McpServer};
use bridle::platform;
use bridle::skills::{self, SkillsStatusState, SkillsSyncAction};
use bridle::sync::{self, SyncAction, SyncState};
use clap::{Parser, Subcommand, ValueEnum};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "bridle",
    version,
    about = "Sync MCP servers, skills, and agents across AI harnesses"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize ~/Bridle/ with default config
    Init,
    /// Scan system and list detected AI harnesses
    Discover,
    /// Push master config to all installed harnesses
    Sync {
        /// Watch for changes and sync automatically
        #[arg(long)]
        watch: bool,
        /// Force overwrite even if drift detected
        #[arg(long)]
        force: bool,
        /// Skip syncing the skills directory
        #[arg(long)]
        no_skills: bool,
    },
    /// Show diff between master and each harness
    Status,
    /// Add an MCP server to the master config
    Add {
        /// Server name
        name: String,
        /// Command (e.g. npx)
        #[arg(long)]
        command: Option<String>,
        /// Arguments for the command
        #[arg(long, num_args = 1..)]
        args: Vec<String>,
        /// URL (for HTTP-based MCP servers)
        #[arg(long)]
        url: Option<String>,
        /// Environment variables (KEY=VALUE format)
        #[arg(long, num_args = 1..)]
        env: Vec<String>,
    },
    /// Remove an MCP server, skill, or all from the master.
    ///
    /// Usage: bridle remove [mcp|skills|all] <name>
    Remove {
        /// Remove target and name (e.g. "plane" or "skills caveman")
        #[arg(num_args = 1..=2, required = true)]
        args: Vec<String>,
    },
    /// List all servers in the master config
    List,
    /// Import MCP configs, skills, or all into the master
    Import {
        /// What to import: mcp, skills, or all
        #[arg(value_enum, default_value = "mcp")]
        what: ImportTarget,
        /// Harness ID for MCP import (e.g. pi, codex, cursor) or '--all'
        #[arg(default_value = "all")]
        harness: String,
        /// Import MCP from all detected harnesses
        #[arg(long)]
        all: bool,
        /// Force overwrite of existing entries
        #[arg(long)]
        force: bool,
        /// Create symlinks instead of copies so source updates propagate
        #[arg(long)]
        link: bool,
        /// Re-import only skills whose source content has changed
        #[arg(long)]
        update: bool,
        /// Source directory for skills import [default: ~/.agents/skills]
        #[arg(long)]
        source: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ImportTarget {
    Mcp,
    Skills,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum RemoveTarget {
    Mcp,
    Skills,
    All,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init => cmd_init(),
        Commands::Discover => cmd_discover(),
        Commands::Sync {
            watch,
            force,
            no_skills,
        } => cmd_sync(watch, force, no_skills),
        Commands::Status => cmd_status(),
        Commands::Add {
            name,
            command,
            args,
            url,
            env,
        } => cmd_add(&name, command, args, url, env),
        Commands::Remove { args } => cmd_remove(&args),
        Commands::List => cmd_list(),
        Commands::Import {
            what,
            harness,
            all,
            force,
            link,
            update,
            source,
        } => cmd_import(what, &harness, all, force, link, update, source),
    }
}

fn cmd_discover() {
    let plat = platform::detect();
    println!("Platform: {}", plat.name());
    println!();

    let statuses = adapters::detect_all(plat);
    for status in &statuses {
        let icon = if status.installed { "✅" } else { "❌" };
        let mcp_icon = if status.mcp_config_exists {
            "📄"
        } else {
            "  "
        };
        println!(
            "{} {} {} ({})",
            icon, mcp_icon, status.spec.name, status.spec.id
        );
    }
}

fn cmd_sync(watch: bool, force: bool, no_skills: bool) {
    let plat = platform::detect();
    let home = bridle_home();
    let master_path = home.join("mcp.json");

    // Read master config
    let master = if master_path.exists() {
        let raw = std::fs::read_to_string(&master_path).unwrap_or_default();
        McpConfig::from_json(&raw).unwrap_or_else(|e| {
            eprintln!("Error reading {}/mcp.json: {}", home.display(), e);
            std::process::exit(1);
        })
    } else {
        eprintln!(
            "No master config found at {}/mcp.json. Run 'bridle init' first.",
            home.display()
        );
        std::process::exit(1);
    };

    let master_skills_dir = home.join("skills");

    if watch {
        run_watch(plat, home, master, master_skills_dir, force, no_skills);
        return;
    }

    let mut state = SyncState::load_or_default(&home);
    run_sync_pass(
        &master,
        &master_skills_dir,
        &mut state,
        &home,
        plat,
        force,
        no_skills,
    );
}

fn run_sync_pass(
    master: &McpConfig,
    master_skills_dir: &Path,
    state: &mut SyncState,
    home: &Path,
    plat: platform::Platform,
    force: bool,
    no_skills: bool,
) {
    // ── MCP sync ──────────────────────────────────────────────────────
    let reports = sync::sync_all(master, state, plat);

    let mut drift_detected = false;
    for report in &reports {
        match &report.action {
            SyncAction::Updated => {
                println!("✅ {} — synced", report.harness_id);
            }
            SyncAction::AlreadyInSync => {
                println!("⏭️  {} — already up to date", report.harness_id);
            }
            SyncAction::NotInstalled => {
                println!("⚠️  {} — not installed, skipped", report.harness_id);
            }
            SyncAction::Drift { .. } => {
                if force {
                    let spec = harness::all()
                        .iter()
                        .find(|s| s.id == report.harness_id)
                        .unwrap();
                    if let Some(adapter) = sync::adapter_for(spec) {
                        match adapter.write_config(master, plat) {
                            Ok(()) => {
                                state.last_hashes.insert(
                                    report.harness_id.to_string(),
                                    sync::hash_config(master),
                                );
                                println!("🔄 {} — drift overwritten (--force)", report.harness_id);
                            }
                            Err(e) => {
                                println!("❌ {} — error: {}", report.harness_id, e);
                            }
                        }
                    }
                } else {
                    drift_detected = true;
                    println!(
                        "🔀 {} — DRIFT DETECTED (use --force to overwrite, or resolve manually)",
                        report.harness_id
                    );
                }
            }
            SyncAction::Error(msg) => {
                println!("❌ {} — error: {}", report.harness_id, msg);
            }
        }
    }

    // ── Skills sync ───────────────────────────────────────────────────
    if !no_skills {
        let skill_reports = skills::sync_skills_all(master_skills_dir, state, plat, force);
        for report in &skill_reports {
            match &report.action {
                SkillsSyncAction::Updated { installed } if installed.is_empty() => {
                    println!("⏭️  {} skills — already up to date", report.harness_id);
                }
                SkillsSyncAction::Updated { installed } => {
                    println!(
                        "✅ {} skills — synced: {}",
                        report.harness_id,
                        installed.join(", ")
                    );
                }
                SkillsSyncAction::AlreadyInSync => {
                    println!("⏭️  {} skills — already up to date", report.harness_id);
                }
                SkillsSyncAction::NotInstalled => {
                    println!(
                        "⚠️  {} skills — harness not installed, skipped",
                        report.harness_id
                    );
                }
                SkillsSyncAction::NoSkillsDir => {
                    // Harness doesn't support skills; stay quiet to avoid noise.
                }
                SkillsSyncAction::Drift { skills } => {
                    drift_detected = true;
                    println!(
                        "🔀 {} skills — DRIFT on: {} (use --force to overwrite)",
                        report.harness_id,
                        skills.join(", ")
                    );
                }
                SkillsSyncAction::Error(msg) => {
                    println!("❌ {} skills — error: {}", report.harness_id, msg);
                }
            }
        }
    }

    state.save(home).ok();

    if drift_detected {
        println!();
        println!("💡 Run 'bridle status' to see diffs, or 'bridle sync --force' to overwrite.");
    }
}

fn run_watch(
    plat: platform::Platform,
    home: PathBuf,
    master: McpConfig,
    master_skills_dir: PathBuf,
    force: bool,
    no_skills: bool,
) {
    use notify::{Event, EventKind, RecursiveMode, Watcher};

    println!(
        "👁️  Watching {}/ for changes... (Ctrl+C to stop)",
        home.display()
    );

    let mut state = SyncState::load_or_default(&home);

    // Initial sync
    run_sync_pass(
        &master,
        &master_skills_dir,
        &mut state,
        &home,
        plat,
        force,
        no_skills,
    );
    println!();

    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        if let Ok(event) = res {
            if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                tx.send(()).ok();
            }
        }
    })
    .expect("Failed to create file watcher");

    watcher
        .watch(&home, RecursiveMode::NonRecursive)
        .expect("Failed to watch directory");

    // Debounce: wait for changes, then sync
    while let Ok(()) = rx.recv() {
        std::thread::sleep(std::time::Duration::from_millis(500));
        // Drain any pending events
        while rx.try_recv().is_ok() {}

        // Reload master config
        let master_path = home.join("mcp.json");
        let master = if master_path.exists() {
            let raw = std::fs::read_to_string(&master_path).unwrap_or_default();
            McpConfig::from_json(&raw).unwrap_or_default()
        } else {
            continue;
        };

        println!("🔄 Change detected — syncing...");
        let mut state = SyncState::load_or_default(&home);
        run_sync_pass(
            &master,
            &master_skills_dir,
            &mut state,
            &home,
            plat,
            force,
            no_skills,
        );
        println!();
    }
}

fn cmd_status() {
    let plat = platform::detect();
    let home = bridle_home();
    let master_path = home.join("mcp.json");

    let master = if master_path.exists() {
        let raw = std::fs::read_to_string(&master_path).unwrap_or_default();
        McpConfig::from_json(&raw).unwrap_or_default()
    } else {
        println!("No master config at {}/mcp.json", home.display());
        return;
    };

    let master_hash = sync::hash_config(&master);
    println!("Master config hash: {}", &master_hash[..12]);
    println!();

    for spec in harness::all() {
        let base = spec.base_dir(plat);
        if !base.exists() {
            println!("⚠️  {} — not installed", spec.id);
            continue;
        }

        let adapter = match sync::adapter_for(spec) {
            Some(a) => a,
            None => {
                println!("❌ {} — no adapter", spec.id);
                continue;
            }
        };

        match adapter.read_config(plat) {
            Ok(cfg) => {
                let h = sync::hash_config(&cfg);
                if h == master_hash {
                    println!("✅ {} — in sync", spec.id);
                } else {
                    println!(
                        "🔀 {} — differs (master: {} ≠ harness: {})",
                        spec.id,
                        &master_hash[..12],
                        &h[..12]
                    );
                    let diff = master.diff_against(&cfg);
                    for name in &diff.added {
                        println!("   + {} (missing on harness)", name);
                    }
                    for name in &diff.removed {
                        println!("   - {} (only on harness)", name);
                    }
                    for (name, server_diff) in &diff.modified {
                        let fields = server_diff.changed_fields().join(", ");
                        println!("   ~ {} (modified: {})", name, fields);
                    }
                }
            }
            Err(e) => {
                println!("❌ {} — error reading config: {}", spec.id, e);
            }
        }
    }

    // ── Skills status ─────────────────────────────────────────────────
    println!();
    let master_skills_dir = home.join("skills");
    let skill_statuses = skills::status_skills_all(&master_skills_dir, plat);
    let mut any_skill_support = false;
    for report in &skill_statuses {
        if matches!(report.state, SkillsStatusState::NoSkillsDir) {
            continue;
        }
        any_skill_support = true;
        match &report.state {
            SkillsStatusState::InSync => {
                println!("✅ {} skills — in sync", report.harness_id);
            }
            SkillsStatusState::NotInstalled => {
                println!("⚠️  {} skills — not installed", report.harness_id);
            }
            SkillsStatusState::Missing { skills } => {
                println!(
                    "🔀 {} skills — missing: {}",
                    report.harness_id,
                    skills.join(", ")
                );
            }
            SkillsStatusState::Drifted { skills } => {
                println!(
                    "🔀 {} skills — drifted: {}",
                    report.harness_id,
                    skills.join(", ")
                );
            }
            SkillsStatusState::Mixed { missing, drifted } => {
                println!(
                    "🔀 {} skills — missing: {}, drifted: {}",
                    report.harness_id,
                    missing.join(", "),
                    drifted.join(", ")
                );
            }
            SkillsStatusState::Error(msg) => {
                println!("❌ {} skills — error: {}", report.harness_id, msg);
            }
            SkillsStatusState::NoSkillsDir => {}
        }
    }
    if !any_skill_support {
        println!("No harnesses with skills support detected.");
    }
}

fn cmd_init() {
    let home = bridle_home();
    std::fs::create_dir_all(&home).expect("Failed to create ~/Bridle/");

    let mcp_path = home.join("mcp.json");
    if !mcp_path.exists() {
        let default = McpConfig::new();
        std::fs::write(&mcp_path, default.to_json_pretty().unwrap())
            .expect("Failed to write mcp.json");
        println!("✅ Created {}/mcp.json", home.display());
    } else {
        println!("⏭️  {}/mcp.json already exists", home.display());
    }

    let config_path = home.join("config.json");
    if !config_path.exists() {
        let state = SyncState::default();
        state.save(&home).expect("Failed to write config.json");
        println!("✅ Created {}/config.json", home.display());
    }

    println!();
    println!("Use 'bridle add <name>' to add MCP servers, then 'bridle sync' to push.");
}

fn cmd_add(
    name: &str,
    command: Option<String>,
    args: Vec<String>,
    url: Option<String>,
    env_pairs: Vec<String>,
) {
    let home = bridle_home();
    let mcp_path = home.join("mcp.json");

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

    config.add_server(name, server);

    std::fs::create_dir_all(&home).ok();
    std::fs::write(&mcp_path, config.to_json_pretty().unwrap()).expect("Failed to write mcp.json");
    println!("✅ Added '{}' to master config", name);
    println!("   Run 'bridle sync' to push to all harnesses.");
}

fn cmd_remove(args: &[String]) {
    let (what, name) = match args.len() {
        1 => (RemoveTarget::Mcp, args[0].as_str()),
        2 => {
            let what = RemoveTarget::from_str(&args[0], true).unwrap_or_else(|_| {
                eprintln!(
                    "Error: '{}' is not a valid remove target. Use mcp, skills, or all.",
                    args[0]
                );
                std::process::exit(1);
            });
            (what, args[1].as_str())
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
        let mcp_path = home.join("mcp.json");
        let mut config = if mcp_path.exists() {
            let raw = std::fs::read_to_string(&mcp_path).unwrap_or_default();
            McpConfig::from_json(&raw).unwrap_or_default()
        } else {
            eprintln!("No master config at {}/mcp.json", home.display());
            return;
        };

        if config.remove_server(name).is_some() {
            std::fs::write(&mcp_path, config.to_json_pretty().unwrap())
                .expect("Failed to write mcp.json");
            println!("✅ Removed MCP server '{}' from master config", name);
        } else {
            println!("⚠️  MCP server '{}' not found in master config", name);
        }
    }

    if remove_skills {
        let skills_dir = home.join("skills");
        match skills::remove_skill(&skills_dir, name) {
            Ok(true) => println!("✅ Removed skill '{}' from ~/Bridle/skills/", name),
            Ok(false) => println!("⚠️  Skill '{}' not found in ~/Bridle/skills/", name),
            Err(e) => eprintln!("❌ Error removing skill '{}': {}", name, e),
        }
    }
}

fn cmd_list() {
    let home = bridle_home();
    let mcp_path = home.join("mcp.json");

    let config = if mcp_path.exists() {
        let raw = std::fs::read_to_string(&mcp_path).unwrap_or_default();
        McpConfig::from_json(&raw).unwrap_or_default()
    } else {
        println!("No master config at {}/mcp.json", home.display());
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

fn cmd_import(
    what: ImportTarget,
    harness_id: &str,
    all: bool,
    force: bool,
    link: bool,
    update: bool,
    source: Option<PathBuf>,
) {
    let import_mcp = matches!(what, ImportTarget::Mcp | ImportTarget::All);
    let import_skills = matches!(what, ImportTarget::Skills | ImportTarget::All);

    if import_mcp {
        cmd_import_mcp(harness_id, all, force);
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
    let mcp_path = home.join("mcp.json");

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
    let target = bridle_home().join("skills");

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
