use std::path::PathBuf;
use std::process::Output;

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

    fn harness_dir(&self, name: &str) -> PathBuf {
        self.home.join(name)
    }

    fn write_cursor_config(&self, contents: &str) {
        let dir = self.harness_dir(".cursor");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mcp.json"), contents).unwrap();
    }

    fn write_pi_config(&self, contents: &str) {
        let dir = self.harness_dir(".pi").join("agent");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mcp.json"), contents).unwrap();
    }
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
    std::fs::create_dir_all(t.harness_dir(".cursor")).unwrap();

    let output = t.run_ok(&["sync"]);
    let stdout = t.stdout(&output);
    assert!(stdout.contains("cursor — synced"), "stdout: {stdout}");

    let cursor_config = std::fs::read_to_string(t.harness_dir(".cursor").join("mcp.json")).unwrap();
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

    let cursor_config = std::fs::read_to_string(t.harness_dir(".cursor").join("mcp.json")).unwrap();
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
        .harness_dir(".pi")
        .join("agent")
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
