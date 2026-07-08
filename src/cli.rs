use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "bridle",
    version,
    about = "Sync MCP servers, skills, and agents across AI harnesses"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
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
    /// Manage configuration profiles
    Profile {
        #[command(subcommand)]
        command: ProfileCommands,
    },
}

#[derive(Subcommand)]
pub enum ProfileCommands {
    /// Create a new profile
    Create { name: String },
    /// List all profiles
    List,
    /// Switch to a different profile
    Switch {
        name: String,
        /// Skip the post-switch sync prompt and do not sync
        #[arg(long)]
        no_sync: bool,
    },
    /// Remove a profile
    Remove { name: String },
    /// Rename a profile
    Rename { old: String, new: String },
    /// Clone an existing profile
    Clone { from: String, to: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ImportTarget {
    Mcp,
    Skills,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RemoveTarget {
    Mcp,
    Skills,
    All,
}
