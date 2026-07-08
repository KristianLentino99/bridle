use std::path::PathBuf;
use std::process::Output;

use bridle::harness::{HarnessSpec, CURSOR, PI};
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
        self.bridle_home.join("mcp.json")
    }

    fn read_mcp_json(&self) -> serde_json::Value {
        let raw = std::fs::read_to_string(self.mcp_json()).unwrap();
        serde_json::from_str(&raw).unwrap()
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

    assert!(t
        .bridle_home
        .join("skills")
        .join("caveman")
        .join("SKILL.md")
        .exists());
}

#[test]
fn sync_skills_to_installed_harness() {
    let t = CliTest::new();
    t.run_ok(&["init"]);

    // Add a skill to the master skills dir.
    let skill_dir = t.bridle_home.join("skills").join("caveman");
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

    let skill_dir = t.bridle_home.join("skills").join("caveman");
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
    std::fs::write(
        &t.mcp_json(),
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
