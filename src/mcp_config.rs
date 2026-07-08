use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Canonical MCP server configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpServer {
    /// URL for HTTP/SSE-based servers. Mutually exclusive with `command`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Command for stdio-based servers. Mutually exclusive with `url`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Arguments for the command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    /// Environment variables for the command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<BTreeMap<String, String>>,
    /// HTTP headers for url-based servers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
}

impl McpServer {
    /// Compare this server against another, returning which top-level fields differ.
    pub fn diff_against(&self, other: &McpServer) -> McpServerDiff {
        McpServerDiff {
            url: self.url != other.url,
            command: self.command != other.command,
            args: self.args != other.args,
            env: self.env != other.env,
            headers: self.headers != other.headers,
        }
    }
}

/// Which fields of an MCP server differ between master and harness.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct McpServerDiff {
    pub url: bool,
    pub command: bool,
    pub args: bool,
    pub env: bool,
    pub headers: bool,
}

impl McpServerDiff {
    pub fn has_changes(&self) -> bool {
        self.url || self.command || self.args || self.env || self.headers
    }

    /// Human-readable list of changed field names.
    pub fn changed_fields(&self) -> Vec<&'static str> {
        let mut fields = vec![];
        if self.url {
            fields.push("url");
        }
        if self.command {
            fields.push("command");
        }
        if self.args {
            fields.push("args");
        }
        if self.env {
            fields.push("env");
        }
        if self.headers {
            fields.push("headers");
        }
        fields
    }
}

/// Difference between two MCP configurations.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct McpConfigDiff {
    /// Servers present in master but missing from harness.
    pub added: Vec<String>,
    /// Servers present in harness but missing from master.
    pub removed: Vec<String>,
    /// Servers present in both but with differing fields.
    pub modified: Vec<(String, McpServerDiff)>,
}

/// Root of the canonical MCP configuration file (`mcp.json`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpConfig {
    #[serde(rename = "mcpServers")]
    pub mcp_servers: BTreeMap<String, McpServer>,
}

impl McpConfig {
    /// Parse a canonical MCP config from JSON string.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Serialize to pretty JSON string.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Create an empty config.
    pub fn new() -> Self {
        McpConfig {
            mcp_servers: BTreeMap::new(),
        }
    }

    /// Add a server to the config.
    pub fn add_server(&mut self, name: &str, server: McpServer) {
        self.mcp_servers.insert(name.to_string(), server);
    }

    /// Remove a server by name.
    pub fn remove_server(&mut self, name: &str) -> Option<McpServer> {
        self.mcp_servers.remove(name)
    }

    /// List all server names.
    pub fn server_names(&self) -> Vec<&str> {
        self.mcp_servers.keys().map(|s| s.as_str()).collect()
    }

    /// Compute the difference between this config and another.
    ///
    /// Perspective: `self` is the master, `other` is the harness.
    /// - `added`: servers present in master but missing from harness
    /// - `removed`: servers present in harness but missing from master
    /// - `modified`: servers present in both but with differing fields
    pub fn diff_against(&self, other: &McpConfig) -> McpConfigDiff {
        let mut diff = McpConfigDiff::default();

        for (name, server) in &self.mcp_servers {
            match other.mcp_servers.get(name) {
                Some(other_server) => {
                    let server_diff = server.diff_against(other_server);
                    if server_diff.has_changes() {
                        diff.modified.push((name.clone(), server_diff));
                    }
                }
                None => diff.added.push(name.clone()),
            }
        }

        for name in other.mcp_servers.keys() {
            if !self.mcp_servers.contains_key(name) {
                diff.removed.push(name.clone());
            }
        }

        diff
    }
}

impl Default for McpConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_JSON: &str = r#"{
  "mcpServers": {
    "posthog": {
      "url": "https://mcp.posthog.com/mcp",
      "headers": {
        "Authorization": "Bearer phx_test"
      }
    },
    "plane": {
      "command": "npx",
      "args": ["plane-mcp-server", "stdio"],
      "env": {
        "PLANE_API_KEY": "plane_api_test"
      }
    }
  }
}"#;

    #[test]
    fn parse_sample_config() {
        let config = McpConfig::from_json(SAMPLE_JSON).unwrap();
        assert_eq!(config.server_names().len(), 2);
        assert!(config.server_names().contains(&"posthog"));
        assert!(config.server_names().contains(&"plane"));
    }

    #[test]
    fn roundtrip_preserves_structure() {
        let config = McpConfig::from_json(SAMPLE_JSON).unwrap();
        let json = config.to_json_pretty().unwrap();
        let roundtripped = McpConfig::from_json(&json).unwrap();
        assert_eq!(config.mcp_servers.len(), roundtripped.mcp_servers.len());
        assert_eq!(
            config.mcp_servers.get("plane").unwrap().command,
            Some("npx".to_string())
        );
    }

    #[test]
    fn empty_config() {
        let config = McpConfig::new();
        assert!(config.server_names().is_empty());
    }

    #[test]
    fn add_and_remove_server() {
        let mut config = McpConfig::new();
        config.add_server(
            "stripe",
            McpServer {
                command: Some("npx".into()),
                args: Some(vec!["-y".into(), "@stripe/mcp".into()]),
                env: None,
                url: None,
                headers: None,
            },
        );
        assert_eq!(config.server_names(), vec!["stripe"]);
        let removed = config.remove_server("stripe");
        assert!(removed.is_some());
    }

    #[test]
    fn diff_detects_added_removed_and_modified() {
        let mut master = McpConfig::new();
        master.add_server(
            "posthog",
            McpServer {
                url: Some("https://mcp.posthog.com/mcp".into()),
                command: None,
                args: None,
                env: None,
                headers: None,
            },
        );
        master.add_server(
            "plane",
            McpServer {
                command: Some("npx".into()),
                args: Some(vec!["plane-mcp-server".into(), "stdio".into()]),
                env: Some(BTreeMap::from([(
                    "PLANE_API_KEY".into(),
                    "master-key".into(),
                )])),
                url: None,
                headers: None,
            },
        );

        let mut harness = McpConfig::new();
        harness.add_server(
            "plane",
            McpServer {
                command: Some("uvx".into()), // changed
                args: Some(vec!["plane-mcp-server".into(), "stdio".into()]),
                env: Some(BTreeMap::from([(
                    "PLANE_API_KEY".into(),
                    "master-key".into(),
                )])),
                url: None,
                headers: None,
            },
        );
        harness.add_server(
            "legacy",
            McpServer {
                command: Some("npx".into()),
                args: None,
                env: None,
                url: None,
                headers: None,
            },
        );

        let diff = master.diff_against(&harness);
        assert_eq!(diff.added, vec!["posthog"]);
        assert_eq!(diff.removed, vec!["legacy"]);
        assert_eq!(diff.modified.len(), 1);
        let (name, server_diff) = &diff.modified[0];
        assert_eq!(name, "plane");
        assert!(server_diff.command);
        assert!(!server_diff.args);
        assert!(!server_diff.env);
        assert_eq!(server_diff.changed_fields(), vec!["command"]);
    }

    #[test]
    fn diff_is_empty_when_configs_match() {
        let mut master = McpConfig::new();
        master.add_server(
            "stripe",
            McpServer {
                command: Some("npx".into()),
                args: Some(vec!["-y".into(), "@stripe/mcp".into()]),
                env: None,
                url: None,
                headers: None,
            },
        );
        let harness = master.clone();
        let diff = master.diff_against(&harness);
        assert!(diff.added.is_empty());
        assert!(diff.removed.is_empty());
        assert!(diff.modified.is_empty());
    }
}
