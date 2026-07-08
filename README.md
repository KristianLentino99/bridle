# bridle

**Sync MCP servers, skills, and agents across all your AI coding harnesses.**

One config file. Every tool in sync.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.96%2B-orange.svg)](https://www.rust-lang.org)

---

## Why bridle?

You use multiple AI coding tools — Pi, Claude, Codex, Cursor, Kimi, VS Code. Each stores MCP server configs in different places with different formats. Adding a new MCP server means updating 5+ files by hand.

**bridle** gives you a single source of truth (`~/Bridle/mcp.json`) and syncs it to every harness automatically.

```
Before:                           After:
~/.pi/agent/mcp.json              ~/Bridle/mcp.json  ← edit once
~/.claude/mcp_servers.json              │
~/.cursor/mcp.json                bridle sync  ← push everywhere
~/.codex/config.toml              bridle sync --watch  ← daemon mode
~/.kimi-code/config.toml
~/Library/.../Claude/...json
~/Library/.../Code/User/mcp.json
```

## Supported harnesses

| Harness | MCP sync target | Skills sync | Agent sync | Notes |
|---------|-----------------|-------------|------------|-------|
| Pi Coding Agent | `mcp.json` (JSON + imports) | ✅ | — | Preserves imports |
| Claude Desktop | `claude_desktop_config.json` (JSON) | — | — | — |
| Claude Code | `mcp_servers.json` (JSON) | — | — | — |
| Cursor | `mcp.json` (JSON) | — | — | — |
| VS Code | `mcp.json` (JSON) | — | — | — |
| Codex CLI | `config.toml` (TOML) | ✅ | Planned | — |
| Kimi Code | `mcp.json` (JSON symlink/copy) | — | — | MCP sync works; some third-party OpenAI-compatible providers can fail in Kimi Code |

Skills sync means Bridle mirrors `~/Bridle/skills/` into the harness's configured skills directory. Kimi also imports MCP servers from `config.toml` as a fallback, but sync writes the supported `mcp.json` target. Known Kimi Code caveat: with some third-party OpenAI-compatible providers, Kimi Code can send `max_tokens` above the provider output limit and fail the first turn; set `KIMI_MODEL_MAX_COMPLETION_TOKENS` to the provider's limit as a workaround.

Cross-platform: **macOS**, **Linux**, **Windows**.

## Quick start

## Install

### Homebrew (macOS/Linux)

```bash
brew install KristianLentino99/tap/bridle
```

### Cargo (any platform)

```bash
cargo install bridle-harness
```

> The crate is published as `bridle-harness` because the `bridle` name was already taken on crates.io. The installed binary is still named `bridle`.

### Build from source

```bash
git clone https://github.com/KristianLentino99/bridle.git
cd bridle
cargo build --release
sudo cp target/release/bridle /usr/local/bin/
```

> Requires Rust 1.96+.

### Verify

```bash
bridle --help
```

### Initialize

```bash
bridle init
```

Creates `~/Bridle/` with an empty `mcp.json` and `config.json`.

### Add MCP servers

```bash
# HTTP-based server
bridle add posthog --url https://mcp.posthog.com/mcp

# Command-based server with env vars
bridle add plane --command npx --args plane-mcp-server --args stdio \
  --env PLANE_API_KEY=your_key --env PLANE_WORKSPACE_SLUG=koomy

# npm package
bridle add stripe --command npx --args=-y --args=@stripe/mcp --args=--tools --args=all
```

### Remove

```bash
bridle remove plane           # Remove an MCP server (default)
bridle remove skills caveman  # Remove a skill from ~/Bridle/skills/
bridle remove all plane       # Remove both an MCP server and a skill with the same name
```

### Sync

```bash
bridle sync           # Push to all installed harnesses
bridle sync --force   # Overwrite even if drift detected
bridle sync --watch   # Watch ~/Bridle/ and sync on changes
```

### Inspect

```bash
bridle discover       # List all detected AI harnesses
bridle status         # Show diff between master and each harness
bridle list           # List all MCP servers in master config
```

### Import into master

```bash
# MCP servers (default)
bridle import              # Import MCP servers from all harnesses
bridle import mcp pi       # Import MCP servers from Pi only
bridle import mcp --all --force

# Skills
bridle import skills                        # Copy ~/.agents/skills/* → ~/Bridle/skills/
bridle import skills --force                # Overwrite existing skills
bridle import skills --update               # Re-import only skills that changed at the source
bridle import skills --link                 # Symlink instead of copy; source updates propagate automatically
bridle import skills --source ~/my-skills   # Import from a custom directory

# All (MCP + skills)
bridle import all --all --force
```

By default, skills are copied so `~/Bridle/skills/` becomes the canonical source. Use `--link` if you prefer `~/.agents/skills/` (or `--source`) to remain the source of truth — updates there are instantly visible to bridle and all harnesses. Use `--update` to refresh copied skills when new versions arrive. Run `bridle sync` afterwards to push everything to every harness.

### Profiles

Keep separate sets of MCP servers and skills for different contexts (work, personal, a startup, etc.).

```bash
bridle profile create work
bridle profile create personal
bridle profile clone work work-backup
bridle profile list
bridle profile switch work
# bridle profile switch warns if bridle sync --watch is running
bridle profile rename personal my-startup
bridle profile remove my-startup
```

- `bridle init` creates a `default` profile.
- Existing single-file setups are auto-migrated to a `default` profile on the first profile command.
- `~/Bridle/mcp.json` and `~/Bridle/skills/` are symlinks to the active profile.
- `bridle sync`, `status`, `add`, `remove`, `list`, and `import` always operate on the active profile.

## How it works

```
~/Bridle/                          ← Single source of truth
├── mcp.json                        → symlink to active profile mcp.json
├── skills/                         → symlink to active profile skills/
├── profiles/                       Named profile directories
│   ├── default/
│   │   ├── mcp.json
│   │   └── skills/
│   ├── work/
│   │   ├── mcp.json
│   │   └── skills/
│   └── my-startup/
│       ├── mcp.json
│       └── skills/
├── agents/                         Shared agent definitions
└── config.json                     Sync state, drift hashes & active profile

bridle sync
  ├── Read ~/Bridle/mcp.json
  ├── Push MCP config to each installed harness
  ├── Sync ~/Bridle/skills/ to each harness's skills directory
  ├── Compare hashes against last-known state
  ├── Drift detected? → warn user (or overwrite with --force)
  └── Save sync state
```

**Drift detection:** bridle stores a SHA-256 hash of each harness config after every sync. If you manually edit a harness outside of bridle, the next sync detects the drift and warns instead of silently overwriting.

## Canonical config format

`~/Bridle/mcp.json`:

```json
{
  "mcpServers": {
    "deepwiki": {
      "url": "https://mcp.deepwiki.com/mcp"
    },
    "plane": {
      "command": "npx",
      "args": ["plane-mcp-server", "stdio"],
      "env": {
        "PLANE_API_KEY": "plane_api_...",
        "PLANE_WORKSPACE_SLUG": "koomy"
      }
    },
    "stripe": {
      "command": "npx",
      "args": ["-y", "@stripe/mcp", "--tools", "all"]
    }
  }
}
```

> **Rule:** canonical format always prefers `npx` commands. Adapters translate to `uvx` or native commands per harness as needed.

## Project structure

```
src/
├── main.rs           CLI (clap): discover, sync, status, init, add, remove, list
├── lib.rs            Crate root, bridle_home()
├── platform.rs       OS detection (macOS/Linux/Windows), path resolution
├── harness.rs        Harness registry (7 harnesses, cross-platform paths)
├── mcp_config.rs     Canonical MCP config parser/writer (JSON)
├── adapters.rs       Format adapters: JsonAdapter, PiAdapter, TomlAdapter
└── sync.rs           Sync engine: drift detection (SHA-256), state persistence
```

## Roadmap

- [x] MCP sync across 7 harnesses (3 format adapters)
- [x] Drift detection with SHA-256 hashing
- [x] `--watch` daemon mode
- [x] Cross-platform path resolution
- [x] Skills sync (`skills/` directory → harness skills dirs)
- [ ] Agent sync (Codex TOML agents)
- [x] `bridle import` — harvest config from harness → master
- [x] Homebrew distribution (`brew install KristianLentino99/tap/bridle`)
- [ ] Nix flake

## Contributing

### Publishing a new release

1. **Tag and push the release**

```bash
VERSION=v0.1.0
cargo test
git tag -a "$VERSION" -m "bridle $VERSION"
git push origin "$VERSION"
```

2. **Update the Homebrew formula SHA**

```bash
VERSION=v0.1.0
curl -L "https://github.com/KristianLentino99/bridle/archive/refs/tags/$VERSION.tar.gz" \
  | shasum -a 256
```

Put that SHA in `homebrew/bridle.rb`.

3. **Create the Homebrew tap repository**

Create a public GitHub repo: `KristianLentino99/homebrew-tap`.

4. **Push the formula to the tap**

```bash
git clone git@github.com:KristianLentino99/homebrew-tap.git ../homebrew-tap
mkdir -p ../homebrew-tap/Formula
cp homebrew/bridle.rb ../homebrew-tap/Formula/bridle.rb
cd ../homebrew-tap
git add Formula/bridle.rb
git commit -m "Add bridle 0.1.0"
git push
```

5. **Validate**

```bash
brew audit --formula --strict KristianLentino99/tap/bridle
brew install --build-from-source KristianLentino99/tap/bridle
brew test KristianLentino99/tap/bridle
```

6. **Install**

```bash
brew install KristianLentino99/tap/bridle
```

### Cargo publish (alternative)

```bash
cargo publish
# Users then do:
# cargo install bridle
```

### Development

## License

MIT © 2026 Kristian Lentino

---

```
   🐴 bridle — keep your harness in sync
```
