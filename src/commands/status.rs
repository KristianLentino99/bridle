use crate::bridle_home;
use crate::harness;
use crate::mcp_config::McpConfig;
use crate::platform;
use crate::skills::{self, SkillsStatusState};
use crate::sync;

pub fn run() {
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
