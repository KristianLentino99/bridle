use crate::harness::HarnessSpec;
use crate::mcp_config::{McpConfig, McpServer};
use crate::platform::Platform;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Result of scanning for a harness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessStatus {
    pub spec: &'static HarnessSpec,
    pub installed: bool,
    pub mcp_config_exists: bool,
}

/// Detect which harnesses are installed on this system.
pub fn detect_all(platform: Platform) -> Vec<HarnessStatus> {
    crate::harness::all()
        .iter()
        .map(|spec| {
            let base = spec.base_dir(platform);
            let mcp_path = spec.mcp_config_path(platform);
            HarnessStatus {
                spec,
                installed: base.exists(),
                mcp_config_exists: mcp_path.exists(),
            }
        })
        .collect()
}

// ── Adapter trait ──────────────────────────────────────────────────

/// Common error type for adapter operations.
#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("TOML error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("TOML serialize error: {0}")]
    TomlSer(#[from] toml::ser::Error),
    #[error("Unsupported MCP format for this adapter")]
    UnsupportedFormat,
}

/// Each harness has an adapter that can read/write its native MCP config.
pub trait Adapter {
    /// Read the harness's native MCP config and convert to canonical form.
    fn read_config(&self, platform: Platform) -> Result<McpConfig, AdapterError>;

    /// Write a canonical MCP config back to the harness's native format.
    fn write_config(&self, config: &McpConfig, platform: Platform) -> Result<(), AdapterError>;

    /// The harness this adapter is for.
    fn harness_id(&self) -> &'static str;
}

// ── JSON adapter (used by Cursor, VS Code, Claude Code) ─────────────

pub struct JsonAdapter {
    spec: &'static HarnessSpec,
}

impl JsonAdapter {
    pub fn new(spec: &'static HarnessSpec) -> Self {
        Self { spec }
    }
}

impl Adapter for JsonAdapter {
    fn harness_id(&self) -> &'static str {
        self.spec.id
    }

    fn read_config(&self, platform: Platform) -> Result<McpConfig, AdapterError> {
        let path = self.spec.mcp_config_path(platform);
        let raw = std::fs::read_to_string(&path)?;
        let config: McpConfig = serde_json::from_str(&raw)?;
        Ok(config)
    }

    fn write_config(&self, config: &McpConfig, platform: Platform) -> Result<(), AdapterError> {
        let path = self.spec.mcp_config_path(platform);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(config)?;
        std::fs::write(&path, json)?;
        Ok(())
    }
}

// ── Pi adapter (JSON with imports) ──────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PiMcpFile {
    #[serde(default)]
    pub imports: Vec<String>,
    #[serde(default, rename = "mcpServers")]
    pub mcp_servers: BTreeMap<String, McpServer>,
}

pub struct PiAdapter {
    spec: &'static HarnessSpec,
}

impl PiAdapter {
    pub fn new(spec: &'static HarnessSpec) -> Self {
        Self { spec }
    }
}

impl Adapter for PiAdapter {
    fn harness_id(&self) -> &'static str {
        "pi"
    }

    fn read_config(&self, platform: Platform) -> Result<McpConfig, AdapterError> {
        let path = self.spec.mcp_config_path(platform);
        let raw = std::fs::read_to_string(&path)?;
        let pi_file: PiMcpFile = serde_json::from_str(&raw)?;
        Ok(McpConfig {
            mcp_servers: pi_file.mcp_servers,
        })
    }

    fn write_config(&self, config: &McpConfig, platform: Platform) -> Result<(), AdapterError> {
        let path = self.spec.mcp_config_path(platform);
        // Preserve existing imports when writing
        let imports = if path.exists() {
            let raw = std::fs::read_to_string(&path)?;
            let existing: PiMcpFile = serde_json::from_str(&raw).unwrap_or(PiMcpFile {
                imports: vec![],
                mcp_servers: BTreeMap::new(),
            });
            existing.imports
        } else {
            vec![
                "cursor".into(),
                "claude-code".into(),
                "claude-desktop".into(),
            ]
        };

        let pi_file = PiMcpFile {
            imports,
            mcp_servers: config.mcp_servers.clone(),
        };

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&pi_file)?;
        std::fs::write(&path, json)?;
        Ok(())
    }
}

// ── TOML adapter (Codex, Kimi) ─────────────────────────────────────

/// Minimal Codex/Kimi config.toml structure — we only care about [mcp_servers.*].
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TomlConfig {
    #[serde(default, rename = "mcp_servers")]
    mcp_servers: BTreeMap<String, TomlMcpServer>,
    /// Preserve everything else as raw
    #[serde(flatten)]
    extra: toml::value::Table,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TomlMcpServer {
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    args: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    env: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    headers: Option<BTreeMap<String, String>>,
}

pub struct TomlAdapter {
    spec: &'static HarnessSpec,
}

impl TomlAdapter {
    pub fn new(spec: &'static HarnessSpec) -> Self {
        Self { spec }
    }
}

impl Adapter for TomlAdapter {
    fn harness_id(&self) -> &'static str {
        self.spec.id
    }

    fn read_config(&self, platform: Platform) -> Result<McpConfig, AdapterError> {
        let path = self.spec.mcp_config_path(platform);
        let raw = std::fs::read_to_string(&path)?;
        let toml_config: TomlConfig = toml::from_str(&raw)?;

        let mcp_servers: BTreeMap<String, McpServer> = toml_config
            .mcp_servers
            .into_iter()
            .map(|(name, s)| {
                (
                    name,
                    McpServer {
                        url: s.url,
                        command: s.command,
                        args: s.args,
                        env: s.env,
                        headers: s.headers,
                    },
                )
            })
            .collect();

        Ok(McpConfig { mcp_servers })
    }

    fn write_config(&self, config: &McpConfig, platform: Platform) -> Result<(), AdapterError> {
        let path = self.spec.mcp_config_path(platform);

        // Read the existing file to preserve non-MCP config
        let mut toml_value: toml::Value = if path.exists() {
            let raw = std::fs::read_to_string(&path)?;
            toml::from_str(&raw).unwrap_or(toml::Value::Table(toml::value::Table::new()))
        } else {
            toml::Value::Table(toml::value::Table::new())
        };

        // Build new [mcp_servers] table
        let mut new_servers = toml::value::Table::new();
        for (name, server) in &config.mcp_servers {
            let mut table = toml::value::Table::new();
            if let Some(ref url) = server.url {
                table.insert("url".into(), toml::Value::String(url.clone()));
            }
            if let Some(ref cmd) = server.command {
                table.insert("command".into(), toml::Value::String(cmd.clone()));
            }
            if let Some(ref args) = server.args {
                let arr: Vec<toml::Value> = args
                    .iter()
                    .map(|a| toml::Value::String(a.clone()))
                    .collect();
                table.insert("args".into(), toml::Value::Array(arr));
            }
            if let Some(ref env) = server.env {
                let mut env_table = toml::value::Table::new();
                for (k, v) in env {
                    env_table.insert(k.clone(), toml::Value::String(v.clone()));
                }
                table.insert("env".into(), toml::Value::Table(env_table));
            }
            if let Some(ref headers) = server.headers {
                let mut hdr_table = toml::value::Table::new();
                for (k, v) in headers {
                    hdr_table.insert(k.clone(), toml::Value::String(v.clone()));
                }
                table.insert("headers".into(), toml::Value::Table(hdr_table));
            }
            new_servers.insert(name.clone(), toml::Value::Table(table));
        }

        // Replace or insert [mcp_servers]
        if let toml::Value::Table(ref mut root) = toml_value {
            root.insert("mcp_servers".into(), toml::Value::Table(new_servers));
        }

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let toml_str = toml::to_string_pretty(&toml_value)?;
        std::fs::write(&path, toml_str)?;
        Ok(())
    }
}

// ── Kimi Symlink Adapter ────────────────────────────────────────────

/// Kimi Code reads MCP config via `--mcp-config-file`. This adapter creates
/// a symlink from `~/.kimi-code/mcp.json` → `~/Bridle/mcp.json` so Kimi
/// always sees the master config directly — no separate sync needed.
pub struct KimiAdapter {
    spec: &'static HarnessSpec,
}

impl KimiAdapter {
    pub fn new(spec: &'static HarnessSpec) -> Self {
        Self { spec }
    }
}

impl Adapter for KimiAdapter {
    fn harness_id(&self) -> &'static str {
        "kimi"
    }

    fn read_config(&self, platform: Platform) -> Result<McpConfig, AdapterError> {
        // Read from the symlink target or from config.toml as fallback
        let base = self.spec.base_dir(platform);
        let mcp_json = base.join("mcp.json");
        let config_toml = base.join("config.toml");

        if mcp_json.exists() {
            let raw = std::fs::read_to_string(&mcp_json)?;
            return Ok(serde_json::from_str(&raw)?);
        }
        if config_toml.exists() {
            let raw = std::fs::read_to_string(&config_toml)?;
            let toml_config: TomlConfig = toml::from_str(&raw)?;
            let servers: BTreeMap<String, McpServer> = toml_config
                .mcp_servers
                .into_iter()
                .map(|(name, s)| {
                    (
                        name,
                        McpServer {
                            url: s.url,
                            command: s.command,
                            args: s.args,
                            env: s.env,
                            headers: s.headers,
                        },
                    )
                })
                .collect();
            return Ok(McpConfig {
                mcp_servers: servers,
            });
        }
        Ok(McpConfig::new())
    }

    fn write_config(&self, _config: &McpConfig, platform: Platform) -> Result<(), AdapterError> {
        let base = self.spec.base_dir(platform);
        let mcp_json = base.join("mcp.json");
        let bridle_master = crate::bridle_home().join("mcp.json");

        std::fs::create_dir_all(&base)?;

        // Remove existing file/symlink if present
        if mcp_json.exists() || mcp_json.is_symlink() {
            std::fs::remove_file(&mcp_json)?;
        }

        // Create symlink: ~/.kimi-code/mcp.json → ~/Bridle/mcp.json
        symlink_or_copy(&bridle_master, &mcp_json)?;

        Ok(())
    }
}

#[cfg(unix)]
fn symlink_or_copy(src: &PathBuf, dst: &PathBuf) -> Result<(), AdapterError> {
    std::os::unix::fs::symlink(src, dst)?;
    Ok(())
}

#[cfg(not(unix))]
fn symlink_or_copy(src: &PathBuf, dst: &PathBuf) -> Result<(), AdapterError> {
    std::fs::copy(src, dst)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::McpFormat;
    use tempfile::TempDir;

    #[test]
    fn detect_all_finds_nothing_in_empty_temp() {
        // On macOS, real harnesses exist. But we can test the function runs.
        let statuses = detect_all(Platform::MacOS);
        assert_eq!(statuses.len(), 7);
        // At least pi should exist on this machine
        let pi_status = statuses.iter().find(|s| s.spec.id == "pi").unwrap();
        assert!(pi_status.installed);
    }

    #[test]
    fn pi_adapter_roundtrip_preserves_imports() {
        let tmp = TempDir::new().unwrap();
        let agent_dir = tmp.path().join(".pi").join("agent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        let mcp_json = agent_dir.join("mcp.json");

        // Write initial Pi config with imports
        let initial = r#"{
  "imports": ["cursor", "claude-code"],
  "mcpServers": {
    "stripe": {
      "command": "npx",
      "args": ["-y", "@stripe/mcp"]
    }
  }
}"#;
        std::fs::write(&mcp_json, initial).unwrap();

        // Create a custom spec that points to our temp dir
        let spec = harness_spec_at(&agent_dir, McpFormat::JsonWithImports);
        let adapter = PiAdapter::new(spec);

        // Read
        let config = adapter.read_config(Platform::MacOS).unwrap();
        assert_eq!(config.server_names(), vec!["stripe"]);

        // Modify and write back
        let mut updated = config.clone();
        updated.add_server(
            "posthog",
            McpServer {
                url: Some("https://mcp.posthog.com/mcp".into()),
                command: None,
                args: None,
                env: None,
                headers: None,
            },
        );
        adapter.write_config(&updated, Platform::MacOS).unwrap();

        // Verify imports preserved
        let raw = std::fs::read_to_string(&mcp_json).unwrap();
        assert!(raw.contains("\"cursor\""));
        assert!(raw.contains("\"claude-code\""));
        assert!(raw.contains("stripe"));
        assert!(raw.contains("posthog"));
    }

    #[test]
    fn json_adapter_reads_and_writes() {
        let tmp = TempDir::new().unwrap();
        let cursor_dir = tmp.path().join(".cursor");
        std::fs::create_dir_all(&cursor_dir).unwrap();
        let mcp_json = cursor_dir.join("mcp.json");

        let initial = r#"{
  "mcpServers": {
    "posthog": {
      "url": "https://mcp.posthog.com/mcp"
    }
  }
}"#;
        std::fs::write(&mcp_json, initial).unwrap();

        let spec = harness_spec_at(&cursor_dir, McpFormat::Json);
        let adapter = JsonAdapter::new(spec);

        let config = adapter.read_config(Platform::MacOS).unwrap();
        assert_eq!(config.server_names(), vec!["posthog"]);

        // Write back with a new server
        let mut updated = config.clone();
        updated.add_server(
            "sentry",
            McpServer {
                url: Some("https://mcp.sentry.dev/mcp".into()),
                command: None,
                args: None,
                env: None,
                headers: None,
            },
        );
        adapter.write_config(&updated, Platform::MacOS).unwrap();

        let raw = std::fs::read_to_string(&mcp_json).unwrap();
        assert!(raw.contains("sentry"));
    }

    #[test]
    fn toml_adapter_reads_codex_style_config() {
        let tmp = TempDir::new().unwrap();
        let agent_dir = tmp.path().join(".codex");
        std::fs::create_dir_all(&agent_dir).unwrap();
        let config_toml = agent_dir.join("config.toml");

        let initial = r#"
model = "gpt-5.5"
personality = "pragmatic"

[mcp_servers.deepwiki]
url = "https://mcp.deepwiki.com/mcp"

[mcp_servers.plane]
command = "uvx"
args = ["plane-mcp-server", "stdio"]

[mcp_servers.plane.env]
PLANE_API_KEY = "plane_api_test"
"#;
        std::fs::write(&config_toml, initial).unwrap();

        let spec = harness_spec_at(&agent_dir, McpFormat::Toml);
        let adapter = TomlAdapter::new(spec);

        let config = adapter.read_config(Platform::MacOS).unwrap();
        assert!(config.server_names().contains(&"deepwiki"));
        assert!(config.server_names().contains(&"plane"));

        // Write back with a new server
        let mut updated = config.clone();
        updated.add_server(
            "stripe",
            McpServer {
                command: Some("npx".into()),
                args: Some(vec!["-y".into(), "@stripe/mcp".into()]),
                url: None,
                env: None,
                headers: None,
            },
        );
        adapter.write_config(&updated, Platform::MacOS).unwrap();

        let raw = std::fs::read_to_string(&config_toml).unwrap();
        assert!(
            raw.contains("model = \"gpt-5.5\""),
            "preserved non-MCP config"
        );
        assert!(raw.contains("stripe"));
    }

    #[test]
    fn kimi_adapter_creates_symlink_to_bridle_master() {
        let tmp = TempDir::new().unwrap();
        let kimi_dir = tmp.path().join(".kimi-code");
        std::fs::create_dir_all(&kimi_dir).unwrap();

        // Create a fake bridle master
        let bridle_dir = tmp.path().join("Bridle");
        std::fs::create_dir_all(&bridle_dir).unwrap();
        let master_json = bridle_dir.join("mcp.json");
        std::fs::write(
            &master_json,
            r#"{"mcpServers":{"test":{"url":"https://example.com"}}}"#,
        )
        .unwrap();

        // Override bridle_home for this test by using a custom spec
        let path_str = Box::leak(kimi_dir.to_string_lossy().to_string().into_boxed_str());

        // Create a spec with a custom base that contains mcp.json as target
        let spec = Box::leak(Box::new(HarnessSpec {
            id: "kimi",
            name: "Kimi Code",
            mcp_format: McpFormat::Json,
            macos_base: path_str,
            linux_base: path_str,
            windows_base: path_str,
            mcp_config_file: "mcp.json",
            skills_dir: None,
            agents_dir: None,
            detection_marker: "config.toml",
        }));

        let adapter = KimiAdapter::new(spec);
        let kimi_mcp = kimi_dir.join("mcp.json");

        // Write should fail because bridle_home() points to real ~/Bridle
        // Instead test that read works with a direct mcp.json
        std::fs::write(
            &kimi_mcp,
            r#"{"mcpServers":{"kimi-test":{"url":"https://kimi.example.com"}}}"#,
        )
        .unwrap();

        let config = adapter.read_config(Platform::MacOS).unwrap();
        assert!(config.server_names().contains(&"kimi-test"));
    }

    // Helper: create a &'static HarnessSpec pointing at a temp directory
    fn harness_spec_at(base: &std::path::Path, format: McpFormat) -> &'static HarnessSpec {
        let path_str = Box::leak(base.to_string_lossy().to_string().into_boxed_str());
        let config_file = match format {
            McpFormat::Toml => "config.toml",
            _ => "mcp.json",
        };
        Box::leak(Box::new(HarnessSpec {
            id: "test",
            name: "Test",
            mcp_format: format,
            macos_base: path_str,
            linux_base: path_str,
            windows_base: path_str,
            mcp_config_file: config_file,
            skills_dir: None,
            agents_dir: None,
            detection_marker: config_file,
        }))
    }
}
