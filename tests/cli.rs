use std::path::PathBuf;
use std::process::Output;

use bridle::harness::{HarnessSpec, CLAUDE_CODE, CLAUDE_DESKTOP, CURSOR, PI};
use bridle::platform::{self, Platform};

/// Integration tests for the `bridle` CLI.
///
/// Each test runs the compiled binary in a temporary home directory using
/// `BRIDLE_HOME` and `BRIDLE_TEST_HOME` overrides so we never touch the
/// user's real config files.
fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_bridle"))
}

struct CliTest {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    bridle_home: PathBuf,
}

impl CliTest {
    fn new() -> Self {
        let tmp = tempfile::TempDir::new().unwrap();
        let home = tmp.path().to_path_buf();
        let bridle_home = home.join("Bridle");
        Self {
            _tmp: tmp,
            home,
            bridle_home,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        std::process::Command::new(binary())
            .args(args)
            .env("BRIDLE_HOME", &self.bridle_home)
            .env("BRIDLE_TEST_HOME", &self.home)
            .output()
            .expect("failed to run bridle binary")
    }

    fn run_ok(&self, args: &[&str]) -> Output {
        let output = self.run(args);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "bridle {:?} failed: {stderr}",
            args
        );
        output
    }

    fn stdout(&self, output: &Output) -> String {
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn mcp_json(&self) -> PathBuf {
        // Resolve the active profile path so tests work on Windows where the
        // legacy ~/Bridle/mcp.json path may be a copy instead of a symlink.
        self.active_mcp_json()
    }

    fn active_mcp_json(&self) -> PathBuf {
        let config_path = self.bridle_home.join("config.json");
        let active = if config_path.exists() {
            let raw = std::fs::read_to_string(&config_path).unwrap();
            let config: serde_json::Value = serde_json::from_str(&raw).unwrap();
            config["active_profile"]
                .as_str()
                .unwrap_or("default")
                .to_string()
        } else {
            "default".to_string()
        };
        self.profile_mcp_json(&active)
    }

    fn read_mcp_json(&self) -> serde_json::Value {
        let raw = std::fs::read_to_string(self.mcp_json()).unwrap();
        serde_json::from_str(&raw).unwrap()
    }

    fn skills_dir(&self) -> PathBuf {
        let config_path = self.bridle_home.join("config.json");
        let active = if config_path.exists() {
            let raw = std::fs::read_to_string(&config_path).unwrap();
            let config: serde_json::Value = serde_json::from_str(&raw).unwrap();
            config["active_profile"]
                .as_str()
                .unwrap_or("default")
                .to_string()
        } else {
            "default".to_string()
        };
        self.profile_dir(&active).join("skills")
    }

    fn harness_base(&self, spec: &'static HarnessSpec) -> PathBuf {
        spec.base_dir_in_home(platform::detect(), &self.home)
    }

    fn write_cursor_config(&self, contents: &str) {
        let dir = self.harness_base(&CURSOR);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mcp.json"), contents).unwrap();
    }

    fn write_pi_config(&self, contents: &str) {
        let dir = self.harness_base(&PI);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mcp.json"), contents).unwrap();
    }

    fn write_claude_code_config(&self, contents: &str) {
        // Claude Code stores config in ~/.claude.json (a dotfile at HOME level).
        std::fs::write(self.home.join(".claude.json"), contents).unwrap();
    }

    fn profile_dir(&self, name: &str) -> PathBuf {
        self.bridle_home.join("profiles").join(name)
    }

    fn profile_mcp_json(&self, name: &str) -> PathBuf {
        self.profile_dir(name).join("mcp.json")
    }

    fn read_profile_mcp_json(&self, name: &str) -> serde_json::Value {
        let raw = std::fs::read_to_string(self.profile_mcp_json(name)).unwrap();
        serde_json::from_str(&raw).unwrap()
    }
}

#[test]
fn test_harness_base_uses_platform_specific_layout() {
    let t = CliTest::new();

    assert_eq!(
        PI.base_dir_in_home(Platform::Windows, &t.home),
        t.home
            .join("AppData")
            .join("Roaming")
            .join("pi")
            .join("agent")
    );
}

#[test]
fn init_creates_default_configs() {
    let t = CliTest::new();
    t.run_ok(&["init"]);

    assert!(t.mcp_json().exists());
    assert!(t.bridle_home.join("config.json").exists());

    let config = t.read_mcp_json();
    assert!(config["mcpServers"].as_object().unwrap().is_empty());
}

#[test]
fn add_creates_http_server() {
    let t = CliTest::new();
    t.run_ok(&["init"]);
    t.run_ok(&["add", "posthog", "--url", "https://mcp.posthog.com/mcp"]);

    let config = t.read_mcp_json();
    let server = config["mcpServers"]["posthog"].as_object().unwrap();
    assert_eq!(server["url"], "https://mcp.posthog.com/mcp");
}

#[test]
fn add_creates_command_server_with_env() {
    let t = CliTest::new();
    t.run_ok(&["init"]);
    t.run_ok(&[
        "add",
        "plane",
        "--command",
        "npx",
        "--args",
        "plane-mcp-server",
        "--args",
        "stdio",
        "--env",
        "PLANE_API_KEY=secret",
    ]);

    let config = t.read_mcp_json();
    let server = config["mcpServers"]["plane"].as_object().unwrap();
    assert_eq!(server["command"], "npx");
    assert_eq!(
        server["args"],
        serde_json::json!(["plane-mcp-server", "stdio"])
    );
    assert_eq!(server["env"]["PLANE_API_KEY"], "secret");
}

#[test]
fn list_shows_servers() {
    let t = CliTest::new();
    t.run_ok(&["init"]);
    t.run_ok(&["add", "posthog", "--url", "https://mcp.posthog.com/mcp"]);
    t.run_ok(&["add", "stripe", "--command", "npx"]);

    let output = t.run_ok(&["list"]);
    let stdout = t.stdout(&output);

    assert!(stdout.contains("posthog"));
    assert!(stdout.contains("stripe"));
}

#[test]
fn remove_deletes_server() {
    let t = CliTest::new();
    t.run_ok(&["init"]);
    t.run_ok(&["add", "posthog", "--url", "https://mcp.posthog.com/mcp"]);
    t.run_ok(&["remove", "posthog"]);

    let config = t.read_mcp_json();
    assert!(!config["mcpServers"]
        .as_object()
        .unwrap()
        .contains_key("posthog"));
}

#[test]
fn sync_updates_installed_harness() {
    let t = CliTest::new();
    t.run_ok(&["init"]);
    t.run_ok(&["add", "posthog", "--url", "https://mcp.posthog.com/mcp"]);

    // Fake a Cursor install with no existing MCP config.
    let cursor_base = t.harness_base(&CURSOR);
    std::fs::create_dir_all(&cursor_base).unwrap();

    let output = t.run_ok(&["sync"]);
    let stdout = t.stdout(&output);
    assert!(stdout.contains("cursor — synced"), "stdout: {stdout}");

    let cursor_config = std::fs::read_to_string(cursor_base.join("mcp.json")).unwrap();
    assert!(cursor_config.contains("posthog"));
}

#[test]
fn sync_to_claude_desktop_skips_url_only_servers() {
    let t = CliTest::new();
    t.run_ok(&["init"]);
    t.run_ok(&["add", "posthog", "--url", "https://mcp.posthog.com/mcp"]);
    t.run_ok(&[
        "add",
        "plane",
        "--command",
        "npx",
        "--args",
        "plane-mcp-server",
        "--args",
        "stdio",
    ]);

    // Fake a Claude Desktop install with no existing MCP config.
    let claude_base = t.harness_base(&CLAUDE_DESKTOP);
    std::fs::create_dir_all(&claude_base).unwrap();

    let output = t.run_ok(&["sync"]);
    let stdout = t.stdout(&output);
    assert!(
        stdout.contains("claude-desktop — synced"),
        "stdout: {stdout}"
    );

    let claude_config =
        std::fs::read_to_string(claude_base.join("claude_desktop_config.json")).unwrap();
    let value: serde_json::Value = serde_json::from_str(&claude_config).unwrap();
    let servers = value["mcpServers"].as_object().unwrap();
    assert!(servers.contains_key("plane"), "command server should sync");
    assert!(
        !servers.contains_key("posthog"),
        "URL-only server should not sync to Claude Desktop"
    );
}

#[test]
fn sync_detects_drift() {
    let t = CliTest::new();
    t.run_ok(&["init"]);
    t.run_ok(&["add", "posthog", "--url", "https://mcp.posthog.com/mcp"]);

    // Fake a Cursor install with a config that differs from master.
    t.write_cursor_config(r#"{"mcpServers":{"other":{"command":"npx"}}}"#);

    // First sync writes the master hash.
    t.run_ok(&["sync"]);
    // Mutate the harness outside of bridle.
    t.write_cursor_config(r#"{"mcpServers":{"modified":{"command":"uvx"}}}"#);

    let output = t.run_ok(&["sync"]);
    let stdout = t.stdout(&output);
    assert!(
        stdout.contains("cursor — DRIFT DETECTED"),
        "stdout: {stdout}"
    );
}

#[test]
fn sync_force_overwrites_drift() {
    let t = CliTest::new();
    t.run_ok(&["init"]);
    t.run_ok(&["add", "posthog", "--url", "https://mcp.posthog.com/mcp"]);

    t.write_cursor_config(r#"{"mcpServers":{"other":{"command":"npx"}}}"#);
    t.run_ok(&["sync"]);
    t.write_cursor_config(r#"{"mcpServers":{"modified":{"command":"uvx"}}}"#);

    let output = t.run_ok(&["sync", "--force"]);
    let stdout = t.stdout(&output);
    assert!(
        stdout.contains("cursor — drift overwritten"),
        "stdout: {stdout}"
    );

    let cursor_config = std::fs::read_to_string(t.harness_base(&CURSOR).join("mcp.json")).unwrap();
    assert!(cursor_config.contains("posthog"));
}

#[test]
fn status_shows_drift() {
    let t = CliTest::new();
    t.run_ok(&["init"]);
    t.run_ok(&["add", "posthog", "--url", "https://mcp.posthog.com/mcp"]);

    t.write_cursor_config(r#"{"mcpServers":{"other":{"command":"npx"}}}"#);

    let output = t.run_ok(&["status"]);
    let stdout = t.stdout(&output);
    assert!(stdout.contains("cursor — differs"), "stdout: {stdout}");
    assert!(stdout.contains("+ posthog"), "stdout: {stdout}");
    assert!(stdout.contains("- other"), "stdout: {stdout}");
}

#[test]
fn import_mcp_from_single_harness() {
    let t = CliTest::new();
    t.run_ok(&["init"]);
    t.write_cursor_config(r#"{"mcpServers":{"stripe":{"command":"npx"}}}"#);

    let output = t.run_ok(&["import", "mcp", "cursor"]);
    let stdout = t.stdout(&output);
    assert!(stdout.contains("stripe — imported"), "stdout: {stdout}");

    let config = t.read_mcp_json();
    assert!(config["mcpServers"]
        .as_object()
        .unwrap()
        .contains_key("stripe"));
}

#[test]
fn import_mcp_skips_existing_without_force() {
    let t = CliTest::new();
    t.run_ok(&["init"]);
    t.run_ok(&["add", "stripe", "--command", "npx"]);
    t.write_cursor_config(r#"{"mcpServers":{"stripe":{"command":"uvx"}}}"#);

    let output = t.run_ok(&["import", "mcp", "cursor"]);
    let stdout = t.stdout(&output);
    assert!(
        stdout.contains("stripe — already in master, skipped"),
        "stdout: {stdout}"
    );

    let config = t.read_mcp_json();
    // Existing master value should be preserved.
    assert_eq!(config["mcpServers"]["stripe"]["command"], "npx");
}

#[test]
fn import_mcp_all_harnesses() {
    let t = CliTest::new();
    t.run_ok(&["init"]);
    t.write_cursor_config(r#"{"mcpServers":{"cursor-server":{"command":"npx"}}}"#);
    t.write_pi_config(r#"{"imports":[],"mcpServers":{"pi-server":{"url":"https://example.com"}}}"#);

    let output = t.run_ok(&["import", "mcp", "--all"]);
    let stdout = t.stdout(&output);
    assert!(
        stdout.contains("cursor-server — imported"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("pi-server — imported"), "stdout: {stdout}");

    let config = t.read_mcp_json();
    assert!(config["mcpServers"]
        .as_object()
        .unwrap()
        .contains_key("cursor-server"));
    assert!(config["mcpServers"]
        .as_object()
        .unwrap()
        .contains_key("pi-server"));
}

#[test]
fn import_skills_copies_from_source() {
    let t = CliTest::new();
    t.run_ok(&["init"]);

    let source = t.home.join("source-skills");
    let skill_dir = source.join("caveman");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(skill_dir.join("SKILL.md"), "# caveman").unwrap();

    let output = t.run_ok(&["import", "skills", "--source", source.to_str().unwrap()]);
    let stdout = t.stdout(&output);
    assert!(stdout.contains("caveman"), "stdout: {stdout}");

    assert!(t.skills_dir().join("caveman").join("SKILL.md").exists());
}

#[test]
fn sync_skills_to_installed_harness() {
    let t = CliTest::new();
    t.run_ok(&["init"]);

    // Add a skill to the master skills dir.
    let skill_dir = t.skills_dir().join("caveman");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(skill_dir.join("SKILL.md"), "# caveman").unwrap();

    // Fake a Pi install (Pi supports skills).
    t.write_pi_config(r#"{"imports":[],"mcpServers":{}}"#);

    let output = t.run_ok(&["sync"]);
    let stdout = t.stdout(&output);
    assert!(
        stdout.contains("pi skills — synced: caveman"),
        "stdout: {stdout}"
    );

    assert!(t
        .harness_base(&PI)
        .join("skills")
        .join("caveman")
        .join("SKILL.md")
        .exists());
}

#[test]
fn remove_skill_deletes_from_master_skills() {
    let t = CliTest::new();
    t.run_ok(&["init"]);

    let skill_dir = t.skills_dir().join("caveman");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(skill_dir.join("SKILL.md"), "# caveman").unwrap();

    t.run_ok(&["remove", "skills", "caveman"]);
    assert!(!skill_dir.exists());
}

#[test]
fn init_creates_default_profile() {
    let t = CliTest::new();
    t.run_ok(&["init"]);

    assert!(t.profile_dir("default").exists());
    assert!(t.profile_mcp_json("default").exists());
    assert!(t.profile_dir("default").join("skills").exists());

    let config = t.read_profile_mcp_json("default");
    assert!(config["mcpServers"].as_object().unwrap().is_empty());
}

#[test]
fn profile_create_makes_named_profile() {
    let t = CliTest::new();
    t.run_ok(&["init"]);
    t.run_ok(&["profile", "create", "work"]);

    assert!(t.profile_dir("work").exists());
    assert!(t.profile_mcp_json("work").exists());
    assert!(t.profile_dir("work").join("skills").exists());

    let config = t.read_profile_mcp_json("work");
    assert!(config["mcpServers"].as_object().unwrap().is_empty());
}

#[test]
fn profile_list_shows_profiles_and_active_marker() {
    let t = CliTest::new();
    t.run_ok(&["init"]);
    t.run_ok(&["profile", "create", "work"]);

    let output = t.run_ok(&["profile", "list"]);
    let stdout = t.stdout(&output);

    assert!(stdout.contains("default *"), "stdout: {stdout}");
    assert!(stdout.contains("work"), "stdout: {stdout}");
}

#[test]
fn profile_switch_changes_active_profile() {
    let t = CliTest::new();
    t.run_ok(&["init"]);
    t.run_ok(&["profile", "create", "work"]);

    // Add a server to the active default profile so we can tell them apart.
    t.run_ok(&["add", "posthog", "--url", "https://mcp.posthog.com/mcp"]);

    // Switch without sync.
    let output = t.run_ok(&["profile", "switch", "work", "--no-sync"]);
    let stdout = t.stdout(&output);
    assert!(
        stdout.contains("Switched to profile 'work'"),
        "stdout: {stdout}"
    );

    // work profile should be empty; default should still contain posthog.
    let config = t.read_profile_mcp_json("work");
    assert!(config["mcpServers"].as_object().unwrap().is_empty());

    let default_config = t.read_profile_mcp_json("default");
    assert!(default_config["mcpServers"]
        .as_object()
        .unwrap()
        .contains_key("posthog"));
}

#[test]
fn profile_clone_copies_profile_contents() {
    let t = CliTest::new();
    t.run_ok(&["init"]);
    t.run_ok(&["add", "posthog", "--url", "https://mcp.posthog.com/mcp"]);

    let output = t.run_ok(&["profile", "clone", "default", "work"]);
    let stdout = t.stdout(&output);
    assert!(
        stdout.contains("Cloned profile 'default' to 'work'"),
        "stdout: {stdout}"
    );

    let cloned = t.read_profile_mcp_json("work");
    assert!(cloned["mcpServers"]
        .as_object()
        .unwrap()
        .contains_key("posthog"));
}

#[test]
fn profile_remove_deletes_non_active_profile() {
    let t = CliTest::new();
    t.run_ok(&["init"]);
    t.run_ok(&["profile", "create", "work"]);

    t.run_ok(&["profile", "remove", "work"]);
    assert!(!t.profile_dir("work").exists());
}

#[test]
fn profile_rename_updates_directory_and_active_state() {
    let t = CliTest::new();
    t.run_ok(&["init"]);
    t.run_ok(&["profile", "create", "work"]);
    t.run_ok(&["profile", "switch", "work", "--no-sync"]);

    t.run_ok(&["profile", "rename", "work", "job"]);

    assert!(!t.profile_dir("work").exists());
    assert!(t.profile_dir("job").exists());

    // Active profile should still resolve to the renamed profile.
    let list_output = t.run_ok(&["profile", "list"]);
    let stdout = t.stdout(&list_output);
    assert!(stdout.contains("job *"), "stdout: {stdout}");
}

#[test]
fn profile_auto_migrates_legacy_layout() {
    let t = CliTest::new();
    std::fs::create_dir_all(&t.bridle_home).unwrap();
    // Write to the legacy path before migration has happened.
    std::fs::write(
        t.bridle_home.join("mcp.json"),
        r#"{"mcpServers":{"legacy":{"command":"npx"}}}"#,
    )
    .unwrap();

    // Running any profile command triggers migration.
    t.run_ok(&["profile", "list"]);

    assert!(t.profile_dir("default").exists());
    let migrated = t.read_profile_mcp_json("default");
    assert!(migrated["mcpServers"]
        .as_object()
        .unwrap()
        .contains_key("legacy"));
}

#[test]
fn profile_switch_warns_when_watch_marker_present() {
    let t = CliTest::new();
    t.run_ok(&["init"]);
    t.run_ok(&["profile", "create", "work"]);

    // Simulate a running watch process by creating the marker.
    std::fs::File::create(t.bridle_home.join(".watch")).unwrap();

    // Provide 'n' to the confirmation prompts.
    let mut child = std::process::Command::new(binary())
        .args(["profile", "switch", "work"])
        .env("BRIDLE_HOME", &t.bridle_home)
        .env("BRIDLE_TEST_HOME", &t.home)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    use std::io::Write;
    {
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(b"n\n").unwrap();
    }
    let output = child.wait_with_output().unwrap();

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("bridle sync --watch appears to be running"),
        "combined: {combined}"
    );

    // Because we answered 'n', the active profile should still be default.
    let list_output = t.run_ok(&["profile", "list"]);
    let stdout = t.stdout(&list_output);
    assert!(stdout.contains("default *"), "stdout: {stdout}");
}

#[test]
fn sync_dry_run_does_not_write_files() {
    let t = CliTest::new();
    t.run_ok(&["init"]);
    t.run_ok(&["add", "posthog", "--url", "https://mcp.posthog.com/mcp"]);

    // Fake a Cursor install with no existing MCP config.
    let cursor_base = t.harness_base(&CURSOR);
    std::fs::create_dir_all(&cursor_base).unwrap();

    let state_before = std::fs::read_to_string(t.bridle_home.join("config.json")).unwrap();

    let output = t.run_ok(&["sync", "--dry-run"]);
    let stdout = t.stdout(&output);

    assert!(stdout.contains("Dry run"), "stdout: {stdout}");
    assert!(stdout.contains("cursor — would sync"), "stdout: {stdout}");
    assert!(stdout.contains("+ posthog (would add)"), "stdout: {stdout}");

    // No harness config should have been written.
    assert!(!cursor_base.join("mcp.json").exists());

    // State file should be unchanged.
    let state_after = std::fs::read_to_string(t.bridle_home.join("config.json")).unwrap();
    assert_eq!(state_before, state_after);
}

#[test]
fn sync_dry_run_detects_drift() {
    let t = CliTest::new();
    t.run_ok(&["init"]);
    t.run_ok(&["add", "posthog", "--url", "https://mcp.posthog.com/mcp"]);

    t.write_cursor_config(r#"{"mcpServers":{"other":{"command":"npx"}}}"#);
    t.run_ok(&["sync"]); // establish baseline hash
    t.write_cursor_config(r#"{"mcpServers":{"modified":{"command":"uvx"}}}"#);

    let output = t.run_ok(&["sync", "--dry-run"]);
    let stdout = t.stdout(&output);

    assert!(
        stdout.contains("cursor — would be left drifted"),
        "stdout: {stdout}"
    );
}

#[test]
fn sync_dry_run_force_reports_overwrite() {
    let t = CliTest::new();
    t.run_ok(&["init"]);
    t.run_ok(&["add", "posthog", "--url", "https://mcp.posthog.com/mcp"]);

    t.write_cursor_config(r#"{"mcpServers":{"other":{"command":"npx"}}}"#);
    t.run_ok(&["sync"]); // establish baseline hash
    t.write_cursor_config(r#"{"mcpServers":{"modified":{"command":"uvx"}}}"#);

    let output = t.run_ok(&["sync", "--dry-run", "--force"]);
    let stdout = t.stdout(&output);

    assert!(
        stdout.contains("cursor — would overwrite drift"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("- modified (would remove)"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("+ posthog (would add)"), "stdout: {stdout}");

    // Verify no actual write happened.
    let cursor_config = std::fs::read_to_string(t.harness_base(&CURSOR).join("mcp.json")).unwrap();
    assert!(cursor_config.contains("modified"));
    assert!(!cursor_config.contains("posthog"));
}

#[test]
fn sync_dry_run_skills_no_write() {
    let t = CliTest::new();
    t.run_ok(&["init"]);

    let skill_dir = t.skills_dir().join("caveman");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(skill_dir.join("SKILL.md"), "# caveman").unwrap();

    // Fake a Pi install (Pi supports skills).
    t.write_pi_config(r#"{"imports":[],"mcpServers":{}}"#);

    let output = t.run_ok(&["sync", "--dry-run"]);
    let stdout = t.stdout(&output);

    assert!(
        stdout.contains("pi skills — would sync: caveman"),
        "stdout: {stdout}"
    );

    // No skill should be installed.
    assert!(!t.harness_base(&PI).join("skills").join("caveman").exists());
}

#[test]
fn status_accepts_dry_run_flag() {
    let t = CliTest::new();
    t.run_ok(&["init"]);
    t.run_ok(&["add", "posthog", "--url", "https://mcp.posthog.com/mcp"]);

    let output = t.run_ok(&["status", "--dry-run"]);
    let stdout = t.stdout(&output);

    assert!(stdout.contains("Dry run"), "stdout: {stdout}");
}

#[test]
fn claude_code_has_skills_dir_configured() {
    // Claude Code supports ~/.claude/skills/ — verify the harness spec reflects this.
    assert!(
        CLAUDE_CODE.skills_dir.is_some(),
        "CLAUDE_CODE must have skills_dir set to 'skills' so Bridle can sync skills to ~/.claude/skills/"
    );
}

#[test]
fn sync_skills_to_claude_code() {
    let t = CliTest::new();
    t.run_ok(&["init"]);

    // Add a skill to the master skills dir.
    let skill_dir = t.skills_dir().join("koomy-business-metrics");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(skill_dir.join("SKILL.md"), "# koomy-business-metrics").unwrap();

    // Fake a Claude Code install: ~/.claude/ directory (for harness detection)
    // and ~/.claude.json (the actual config file).
    let claude_dir = t.harness_base(&CLAUDE_CODE);
    std::fs::create_dir_all(&claude_dir).unwrap();
    t.write_claude_code_config(r#"{"mcpServers":{}}"#);

    let output = t.run_ok(&["sync"]);
    let stdout = t.stdout(&output);
    assert!(
        stdout.contains("claude-code skills — synced: koomy-business-metrics"),
        "stdout: {stdout}"
    );

    assert!(claude_dir
        .join("skills")
        .join("koomy-business-metrics")
        .join("SKILL.md")
        .exists());
}
