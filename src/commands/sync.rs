use crate::bridle_home;
use crate::harness;
use crate::mcp_config::McpConfig;
use crate::platform;
use crate::profile;
use crate::skills::{self, SkillsSyncAction};
use crate::sync::{self, SyncAction, SyncState};
use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};

pub fn run(watch: bool, force: bool, no_skills: bool) {
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
    profile::start_watching(&home).ok();
    let _guard = WatchGuard(home.clone());

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

struct WatchGuard(PathBuf);

impl Drop for WatchGuard {
    fn drop(&mut self) {
        profile::stop_watching(&self.0).ok();
    }
}
