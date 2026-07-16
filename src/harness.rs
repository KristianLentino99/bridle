//! Harness registry — defines all supported AI coding harnesses and their
//! platform-specific config paths.

use crate::platform::Platform;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub mcp_format: McpFormat,
    /// Base directory (platform-relative; resolved on demand).
    pub macos_base: &'static str,
    pub linux_base: &'static str,
    pub windows_base: &'static str,
    /// MCP config filename relative to base_dir, unless `mcp_config_absolute`
    /// is set (in which case that absolute path under HOME is used directly).
    pub mcp_config_file: &'static str,
    /// Optional absolute path under HOME for the MCP config file.
    /// When set, `mcp_config_path` returns this path joined to HOME
    /// instead of base_dir.join(mcp_config_file).
    pub mcp_config_absolute: Option<&'static str>,
    /// Skills directory relative to base_dir, if any.
    pub skills_dir: Option<&'static str>,
    /// Agents directory relative to base_dir, if any.
    pub agents_dir: Option<&'static str>,
    /// Detection marker relative to base_dir.
    pub detection_marker: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpFormat {
    Json,
    JsonWithImports,
    Toml,
}

impl HarnessSpec {
    /// Resolve the base directory for the current platform.
    pub fn base_dir(&self, platform: Platform) -> PathBuf {
        let home = crate::platform::home_dir();
        self.base_dir_in_home(platform, &home)
    }

    /// Resolve the base directory for a platform using an explicit home directory.
    pub fn base_dir_in_home(&self, platform: Platform, home: &Path) -> PathBuf {
        let raw = match platform {
            Platform::MacOS => self.macos_base,
            Platform::Linux => self.linux_base,
            Platform::Windows => self.windows_base,
        };
        // Replace `~/` or `~\` with home directory
        if let Some(stripped) = raw.strip_prefix("~/") {
            join_home_relative(home, stripped)
        } else if let Some(stripped) = raw.strip_prefix(r"~\") {
            join_home_relative(home, stripped)
        } else {
            PathBuf::from(raw)
        }
    }

    /// Full path to the MCP config file.
    pub fn mcp_config_path(&self, platform: Platform) -> PathBuf {
        if let Some(absolute) = self.mcp_config_absolute {
            let home = crate::platform::home_dir();
            return join_home_relative(&home, absolute.strip_prefix("~/").unwrap_or(absolute));
        }
        self.base_dir(platform).join(self.mcp_config_file)
    }
}

fn join_home_relative(home: &Path, relative: &str) -> PathBuf {
    let mut path = home.to_path_buf();
    for component in relative.split(['/', '\\']).filter(|s| !s.is_empty()) {
        path.push(component);
    }
    path
}

/// All supported harnesses, in registry order.
pub fn all() -> &'static [HarnessSpec] {
    &[PI, CLAUDE_DESKTOP, CLAUDE_CODE, CURSOR, VSCODE, CODEX, KIMI]
}

pub const PI: HarnessSpec = HarnessSpec {
    id: "pi",
    name: "Pi Coding Agent",
    mcp_format: McpFormat::JsonWithImports,
    macos_base: "~/.pi/agent",
    linux_base: "~/.pi/agent",
    windows_base: r"~\AppData\Roaming\pi\agent", // approximate, adjust after checking
    mcp_config_file: "mcp.json",
    mcp_config_absolute: None,
    skills_dir: Some("skills"),
    agents_dir: None,
    detection_marker: "mcp.json",
};

pub const CLAUDE_DESKTOP: HarnessSpec = HarnessSpec {
    id: "claude-desktop",
    name: "Claude Desktop",
    mcp_format: McpFormat::Json,
    macos_base: "~/Library/Application Support/Claude",
    linux_base: "~/.config/Claude",
    windows_base: r"~\AppData\Roaming\Claude",
    mcp_config_file: "claude_desktop_config.json",
    mcp_config_absolute: None,
    skills_dir: None,
    agents_dir: None,
    detection_marker: "claude_desktop_config.json",
};

pub const CLAUDE_CODE: HarnessSpec = HarnessSpec {
    id: "claude-code",
    name: "Claude Code",
    mcp_format: McpFormat::Json,
    macos_base: "~/.claude",
    linux_base: "~/.claude",
    windows_base: r"~\.claude",
    mcp_config_file: "mcp_servers.json",
    mcp_config_absolute: Some("~/.claude.json"),
    skills_dir: Some("skills"),
    agents_dir: None,
    detection_marker: ".claude.json",
};

pub const CURSOR: HarnessSpec = HarnessSpec {
    id: "cursor",
    name: "Cursor",
    mcp_format: McpFormat::Json,
    macos_base: "~/.cursor",
    linux_base: "~/.cursor",
    windows_base: r"~\.cursor",
    mcp_config_file: "mcp.json",
    mcp_config_absolute: None,
    skills_dir: None,
    agents_dir: None,
    detection_marker: "mcp.json",
};

pub const VSCODE: HarnessSpec = HarnessSpec {
    id: "vscode",
    name: "VS Code",
    mcp_format: McpFormat::Json,
    macos_base: "~/Library/Application Support/Code/User",
    linux_base: "~/.config/Code/User",
    windows_base: r"~\AppData\Roaming\Code\User",
    mcp_config_file: "mcp.json",
    mcp_config_absolute: None,
    skills_dir: None,
    agents_dir: None,
    detection_marker: "mcp.json",
};

pub const CODEX: HarnessSpec = HarnessSpec {
    id: "codex",
    name: "Codex CLI",
    mcp_format: McpFormat::Toml,
    macos_base: "~/.codex",
    linux_base: "~/.codex",
    windows_base: r"~\.codex",
    mcp_config_file: "config.toml",
    mcp_config_absolute: None,
    skills_dir: Some("skills"),
    agents_dir: Some("agents"),
    detection_marker: "config.toml",
};

pub const KIMI: HarnessSpec = HarnessSpec {
    id: "kimi",
    name: "Kimi Code",
    mcp_format: McpFormat::Json, // Uses symlink to bridle master
    macos_base: "~/.kimi-code",
    linux_base: "~/.kimi-code",
    windows_base: r"~\.kimi-code",
    mcp_config_file: "mcp.json",
    mcp_config_absolute: None,
    skills_dir: None,
    agents_dir: None,
    detection_marker: "config.toml",
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Platform;

    #[test]
    fn all_harnesses_have_unique_ids() {
        let ids: Vec<&str> = all().iter().map(|h| h.id).collect();
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(ids.len(), unique.len(), "Duplicate harness IDs found");
    }

    #[test]
    fn pi_base_dir_resolves_home() {
        let base = PI.base_dir(Platform::MacOS);
        let home = dirs::home_dir().unwrap();
        assert!(base.starts_with(home));
        assert!(base.ends_with(".pi/agent"));
    }

    #[test]
    fn codex_is_toml_format() {
        assert_eq!(CODEX.mcp_format, McpFormat::Toml);
    }

    #[test]
    fn codex_has_skills_and_agents() {
        assert!(CODEX.skills_dir.is_some());
        assert!(CODEX.agents_dir.is_some());
    }
}
