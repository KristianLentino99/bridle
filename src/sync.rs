use std::collections::BTreeMap;
use std::path::Path;

use crate::adapters::{Adapter, JsonAdapter, KimiAdapter, PiAdapter, TomlAdapter};
use crate::harness::{HarnessSpec, McpFormat};
use crate::mcp_config::McpConfig;
use crate::platform::Platform;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ── Sync state ──────────────────────────────────────────────────────

/// Persistent state stored in `~/Bridle/config.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncState {
    /// Per-harness last-known MCP config hash (harness_id → sha256 hex).
    #[serde(default)]
    pub last_hashes: BTreeMap<String, String>,
    /// Per-harness last-known canonical skills directory hash.
    #[serde(default)]
    pub last_skill_hashes: BTreeMap<String, String>,
}

impl SyncState {
    pub fn load_or_default(bridle_home: &Path) -> Self {
        let path = bridle_home.join("config.json");
        if path.exists() {
            let raw = std::fs::read_to_string(&path).unwrap_or_default();
            serde_json::from_str(&raw).unwrap_or_default()
        } else {
            SyncState::default()
        }
    }

    pub fn save(&self, bridle_home: &Path) -> Result<(), std::io::Error> {
        let path = bridle_home.join("config.json");
        std::fs::create_dir_all(bridle_home)?;
        let json = serde_json::to_string_pretty(self).unwrap();
        std::fs::write(&path, json)
    }
}

// ── Sync result types ───────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncAction {
    /// Harness updated with master config.
    Updated,
    /// Harness already in sync (hash matches).
    AlreadyInSync,
    /// Drift detected — manual modification on harness side.
    Drift {
        harness_hash: String,
        master_hash: String,
    },
    /// Harness not installed.
    NotInstalled,
    /// Error during sync.
    Error(String),
}

#[derive(Debug, Clone)]
pub struct SyncReport {
    pub harness_id: &'static str,
    pub action: SyncAction,
}

// ── Sync engine ─────────────────────────────────────────────────────

/// Hash a canonical McpConfig for drift detection.
pub fn hash_config(config: &McpConfig) -> String {
    // Serialize deterministically with sorted keys
    let json = serde_json::to_string(config).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(json.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Create the appropriate adapter for a harness spec.
pub fn adapter_for(spec: &'static HarnessSpec) -> Option<Box<dyn Adapter>> {
    match spec.id {
        "kimi" => Some(Box::new(KimiAdapter::new(spec))),
        _ => match spec.mcp_format {
            McpFormat::Json => Some(Box::new(JsonAdapter::new(spec))),
            McpFormat::JsonWithImports => Some(Box::new(PiAdapter::new(spec))),
            McpFormat::Toml => Some(Box::new(TomlAdapter::new(spec))),
        },
    }
}

/// Sync master config to a single harness.
pub fn sync_one(
    spec: &'static HarnessSpec,
    master: &McpConfig,
    state: &mut SyncState,
    platform: Platform,
) -> SyncReport {
    let base = spec.base_dir(platform);
    if !base.exists() {
        return SyncReport {
            harness_id: spec.id,
            action: SyncAction::NotInstalled,
        };
    }

    let adapter = match adapter_for(spec) {
        Some(a) => a,
        None => {
            return SyncReport {
                harness_id: spec.id,
                action: SyncAction::Error("No adapter available".into()),
            }
        }
    };

    let master_hash = hash_config(master);
    let current_hash = match adapter.read_config(platform) {
        Ok(cfg) => hash_config(&cfg),
        Err(_) => String::new(), // No existing config
    };

    let last_known = state.last_hashes.get(spec.id).cloned();

    if current_hash.is_empty() || Some(current_hash.clone()) == last_known {
        // No existing config, or it matches what we last wrote → safe to overwrite
        match adapter.write_config(master, platform) {
            Ok(()) => {
                state.last_hashes.insert(spec.id.to_string(), master_hash);
                SyncReport {
                    harness_id: spec.id,
                    action: SyncAction::Updated,
                }
            }
            Err(e) => SyncReport {
                harness_id: spec.id,
                action: SyncAction::Error(e.to_string()),
            },
        }
    } else if current_hash == master_hash {
        // Already matches master → skip
        state.last_hashes.insert(spec.id.to_string(), master_hash);
        SyncReport {
            harness_id: spec.id,
            action: SyncAction::AlreadyInSync,
        }
    } else {
        // Drift: harness has been modified externally
        SyncReport {
            harness_id: spec.id,
            action: SyncAction::Drift {
                harness_hash: current_hash,
                master_hash,
            },
        }
    }
}

/// Sync master config to all installed harnesses.
pub fn sync_all(master: &McpConfig, state: &mut SyncState, platform: Platform) -> Vec<SyncReport> {
    crate::harness::all()
        .iter()
        .map(|spec| sync_one(spec, master, state, platform))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp_config::McpServer;

    #[test]
    fn hash_is_stable() {
        let mut config = McpConfig::new();
        config.add_server(
            "test",
            McpServer {
                url: Some("https://example.com".into()),
                command: None,
                args: None,
                env: None,
                headers: None,
            },
        );
        let h1 = hash_config(&config);
        let h2 = hash_config(&config);
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_differs_when_config_differs() {
        let mut c1 = McpConfig::new();
        c1.add_server(
            "test",
            McpServer {
                url: Some("https://a.com".into()),
                command: None,
                args: None,
                env: None,
                headers: None,
            },
        );
        let mut c2 = McpConfig::new();
        c2.add_server(
            "test",
            McpServer {
                url: Some("https://b.com".into()),
                command: None,
                args: None,
                env: None,
                headers: None,
            },
        );
        assert_ne!(hash_config(&c1), hash_config(&c2));
    }

    #[test]
    fn sync_state_roundtrip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let home = tmp.path().to_path_buf();

        let mut state = SyncState::default();
        state.last_hashes.insert("pi".into(), "abc123".into());
        state.save(&home).unwrap();

        let loaded = SyncState::load_or_default(&home);
        assert_eq!(loaded.last_hashes.get("pi").unwrap(), "abc123");
    }
}
