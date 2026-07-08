# Integration Tests for bridle

## Goal

Add reliable, isolated integration tests for the `bridle` CLI that exercise real
command parsing and file I/O without touching the user's actual AI harness
configs.

## Approach

### Env-var overrides

Two environment variables make the CLI testable:

- `BRIDLE_HOME` — overrides `~/Bridle` for Bridle's own config/state.
- `BRIDLE_TEST_HOME` — overrides the home directory used to resolve harness
  base paths (`~/.cursor`, `~/.codex`, `~/.pi/agent`, etc.).

This is more explicit and cross-platform than relying on `$HOME`, which
`dirs::home_dir()` does not consistently honor on Windows.

### Production code changes

1. `src/lib.rs` — `bridle_home()` checks `BRIDLE_HOME` first.
2. `src/main.rs` — removes its duplicated `bridle_home()` and uses
   `bridle::bridle_home()`.
3. `src/platform.rs` — `home_dir()` checks `BRIDLE_TEST_HOME` first.
4. `src/harness.rs` — `HarnessSpec::base_dir()` uses
   `crate::platform::home_dir()` instead of `dirs::home_dir()` directly.

### CLI fix

The `remove` command had an invalid clap definition: a positional with a
default value (`what`) before a required positional (`name`). This triggered a
clap debug assertion in integration tests. It was changed to accept 1–2 raw
arguments and parse them manually, preserving the documented usage:

- `bridle remove plane`
- `bridle remove skills caveman`
- `bridle remove all plane`

### Test structure

Tests live in `tests/cli.rs` and use `std::process::Command` to run the
compiled binary (`env!("CARGO_BIN_EXE_bridle")`). A `CliTest` helper creates a
`tempfile::TempDir`, sets the env overrides, and provides methods to run
commands and inspect files.

### Coverage

- `init` creates default configs.
- `add` creates HTTP and command-based servers with env vars.
- `list` shows configured servers.
- `remove` deletes MCP servers and skills.
- `sync` writes configs to installed harnesses.
- `sync --force` overwrites drift.
- `status` reports differences.
- `import mcp` harvests from one or all harnesses.
- `import skills` copies from a source directory.
- `sync` propagates skills to harnesses that support them.

## Verification

```bash
cargo test
```

Result: 46 unit tests + 15 integration tests pass.
