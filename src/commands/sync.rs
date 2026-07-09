use crate::bridle_home;
use crate::harness;
use crate::mcp_config::McpConfig;
use crate::platform;
use crate::profile;
use crate::skills::{self, SkillsSyncAction};
use crate::sync::{self, SyncAction, SyncState};
use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};

pub fn run(watch: bool, force: bool, no_skills: bool, dry_run: bool) {
    let plat = platform::detect();
    let home = bridle_home();
    let master_path = profile::active_mcp_path(&home);

    // Read master config
    let master = if master_path.exists() {
        let raw = std::fs::read_to_string(&master_path).unwrap_or_default();
        McpConfig::from_json(&raw).unwrap_or_else(|e| {
            eprintln!("Error reading {}: {}", master_path.display(), e);
            std::process::exit(1);
        })
    } else {
        eprintln!(
            "No master config found at {}. Run 'bridle init' first.",
            master_path.display()
        );
        std::process::exit(1);
    };

    let master_skills_dir = profile::active_skills_path(&home);

    if watch {
        run_watch(plat, home, master, master_skills_dir, force, no_skills, dry_run);
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
        dry_run,
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
    dry_run: bool,
) {
    if dry_run {
        println!("🔍 Dry run — no files will be modified");
        println!();
    }

    // ── MCP sync ──────────────────────────────────────────────────────
    let reports = sync::sync_all(master, state, plat, force, dry_run);

    let mut drift_detected = false;
    for report in &reports {
        match &report.action {
            SyncAction::Updated { forced: false } => {
                if dry_run {
                    println!("📝 {} — would sync", report.harness_id);
                    print_mcp_dry_run_details(master, report.harness_id, plat);
                } else {
                    println!("✅ {} — synced", report.harness_id);
                }
            }
            SyncAction::Updated { forced: true } => {
                if dry_run {
                    println!("🔄 {} — would overwrite drift (--force)", report.harness_id);
                    print_mcp_dry_run_details(master, report.harness_id, plat);
                } else {
                    println!("🔄 {} — drift overwritten (--force)", report.harness_id);
                }
            }
            SyncAction::AlreadyInSync => {
                println!("⏭️  {} — already up to date", report.harness_id);
            }
            SyncAction::NotInstalled => {
                println!("⚠️  {} — not installed, skipped", report.harness_id);
            }
            SyncAction::Drift { .. } => {
                drift_detected = true;
                if dry_run {
                    println!(
                        "🔀 {} — would be left drifted (use --force to overwrite)",
                        report.harness_id
                    );
                } else {
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
        let skill_reports = skills::sync_skills_all(master_skills_dir, state, plat, force, dry_run);
        for report in &skill_reports {
            match &report.action {
                SkillsSyncAction::Updated { installed, forced } if installed.is_empty() => {
                    println!("⏭️  {} skills — already up to date", report.harness_id);
                }
                SkillsSyncAction::Updated { installed, forced: false } => {
                    if dry_run {
                        println!(
                            "📝 {} skills — would sync: {}",
                            report.harness_id,
                            installed.join(", ")
                        );
                    } else {
                        println!(
                            "✅ {} skills — synced: {}",
                            report.harness_id,
                            installed.join(", ")
                        );
                    }
                }
                SkillsSyncAction::Updated { installed, forced: true } => {
                    if dry_run {
                        println!(
                            "🔄 {} skills — would overwrite: {}",
                            report.harness_id,
                            installed.join(", ")
                        );
                    } else {
                        println!(
                            "🔄 {} skills — drift overwritten (--force): {}",
                            report.harness_id,
                            installed.join(", ")
                        );
                    }
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
                    if dry_run {
                        println!(
                            "🔀 {} skills — would be left drifted: {}",
                            report.harness_id,
                            skills.join(", ")
                        );
                    } else {
                        println!(
                            "🔀 {} skills — DRIFT on: {} (use --force to overwrite)",
                            report.harness_id,
                            skills.join(", ")
                        );
                    }
                }
                SkillsSyncAction::Error(msg) => {
                    println!("❌ {} skills — error: {}", report.harness_id, msg);
                }
            }
        }
    }

    if !dry_run {
        state.save(home).ok();
    }

    if drift_detected && !dry_run {
        println!();
        println!("💡 Run 'bridle status' to see diffs, or 'bridle sync --force' to overwrite.");
    }
}

/// Print the MCP servers that would be added/removed/modified in a dry run.
fn print_mcp_dry_run_details(master: &McpConfig, harness_id: &str, plat: platform::Platform) {
    let spec = match harness::all().iter().find(|s| s.id == harness_id) {
        Some(s) => s,
        None => return,
    };
    let adapter = match sync::adapter_for(spec) {
        Some(a) => a,
        None => return,
    };
    let effective_master = adapter.effective_config(master, plat);
    let harness_cfg = adapter.read_config(plat).unwrap_or_else(|_| McpConfig::new());
    let diff = effective_master.diff_against(&harness_cfg);

    for name in &diff.added {
        println!("   + {} (would add)", name);
    }
    for name in &diff.removed {
        println!("   - {} (would remove)", name);
    }
    for (name, server_diff) in &diff.modified {
        let fields = server_diff.changed_fields().join(", ");
        println!("   ~ {} (would modify: {})", name, fields);
    }
}

fn run_watch(
    plat: platform::Platform,
    home: PathBuf,
    master: McpConfig,
    master_skills_dir: PathBuf,
    force: bool,
    no_skills: bool,
    dry_run: bool,
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
        dry_run,
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
        let master_path = profile::active_mcp_path(&home);
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
            dry_run,
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
