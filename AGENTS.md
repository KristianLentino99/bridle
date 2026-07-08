# Agent Instructions

## Development Workflow

### Test-Driven Development (TDD)

All feature work and bug fixes on this project follow test-driven development:

1. **RED** — Write a failing test that describes one observable behavior through a public interface.
2. **GREEN** — Write the minimum code needed to make that test pass.
3. **REFACTOR** — Clean up once green, running tests after each change.

Guidelines:
- Work in vertical slices (one behavior at a time), not horizontal slices.
- Prefer integration-style tests that exercise real code paths.
- For CLI behavior, use the existing `BRIDLE_HOME` override pattern in `tests/cli.rs` so tests never touch the user's real config files.
- Do not mock internal collaborators or test private methods.
- Do not add speculative features beyond what the current test requires.

## Documentation & Planning

### Use GitHub Issues to Document Work

- Every feature, refactor, or significant bug fix must be tracked as a GitHub issue.
- Design decisions reached during discovery or interviews are recorded in the issue body (or a dedicated comment) so the project board becomes the source of truth.
- Before implementation starts, the issue should contain:
  - Problem statement
  - Chosen approach and rejected alternatives
  - Acceptance criteria
  - Related files
- Update the issue and the linked GitHub Project board item as work progresses.
- Closing an issue requires passing tests and an up-to-date issue description.

## Project Context

- Rust CLI tool (`cargo build`, `cargo test`).
- Binary entry point: `src/main.rs`.
- Library modules: `src/lib.rs` exposes `adapters`, `cli`, `commands`, `harness`, `mcp_config`, `platform`, `skills`, `sync`.
- Integration tests live in `tests/cli.rs`.
