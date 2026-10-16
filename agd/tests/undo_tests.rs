use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use assert_cmd::cargo;
use serde_json::{Value, json};

/// An org file with an Inbox and a config capturing into it, with the
/// journal under the directory too.
struct Fixture {
    directory: tempfile::TempDir,
    config: std::path::PathBuf,
    inbox: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("temporary directory");
        let inbox = directory.path().join("inbox.org");
        fs::write(&inbox, "* Inbox\n").expect("org fixture");
        let config = directory.path().join("config.toml");
        fs::write(
            &config,
            format!(
                concat!(
                    "roots = [{root:?}]\n\n",
                    "[[capture]]\n",
                    "key = \"t\"\n",
                    "desc = \"Todo\"\n",
                    "file = {inbox:?}\n",
                    "heading = \"Inbox\"\n",
                    "template = \"* TODO %?\"\n",
                ),
                root = directory.path().to_string_lossy(),
                inbox = inbox.to_string_lossy(),
            ),
        )
        .expect("config fixture");
        Self {
            directory,
            config,
            inbox,
        }
    }

    fn command(&self) -> assert_cmd::Command {
        let mut command = cargo::cargo_bin_cmd!("agd");
        command.arg("--config").arg(&self.config);
        command.env("XDG_DATA_HOME", self.directory.path().join("data"));
        command.env("NO_COLOR", "1");
        command
    }

    fn inbox(&self) -> String {
        fs::read_to_string(&self.inbox).expect("inbox")
    }

    /// Captures `title` as an MCP agent does, over `agd mcp`'s stdio.
    fn agent_capture(&self, title: &str) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_agd"))
            .arg("--config")
            .arg(&self.config)
            .arg("mcp")
            .env("XDG_DATA_HOME", self.directory.path().join("data"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("agd mcp");
        let messages = [
            json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
                "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": {"name": "codex-mcp-client", "version": "1.0"}}}),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
                "name": "capture", "arguments": {"template_key": "t", "title": title}}}),
        ];
        let mut stdin = child.stdin.take().expect("stdin");
        for message in messages {
            writeln!(stdin, "{message}").expect("request");
        }
        stdin.flush().expect("flush");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        for line in stdout.lines() {
            let message: Value = serde_json::from_str(&line.expect("line")).expect("JSON-RPC");
            if message["id"] == 1 {
                assert_ne!(message["result"]["isError"], true, "{message}");
                break;
            }
        }
        drop(stdin);
        child.wait().expect("agd mcp exits");
    }
}

#[test]
fn undo_reverts_your_latest_command_line_change_and_never_an_agents() {
    let fixture = Fixture::new();
    fixture.agent_capture("Agent task");
    let after_agent = fixture.inbox();
    for title in ["Buy milk", "Call mum"] {
        fixture
            .command()
            .args(["capture", "t", title])
            .assert()
            .success();
    }

    // The newest first, said as the TUI says a change.
    let output = fixture.command().arg("undo").output().expect("agd undo");
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.ends_with("  undid capture · Call mum\n"), "{stdout}");
    assert!(stdout.starts_with("inbox.org:"), "{stdout}");
    assert!(!fixture.inbox().contains("Call mum"));

    // --json prints the undo result.
    let output = fixture
        .command()
        .args(["undo", "--json"])
        .output()
        .expect("agd undo --json");
    assert!(output.status.success(), "{output:?}");
    let result: Value = serde_json::from_slice(&output.stdout).expect("undo JSON");
    assert_eq!(result["schema"], "agd.undo/v1");
    assert_eq!(result["ok"], true);
    assert_eq!(fixture.inbox(), after_agent, "the agent's capture stays");

    // Nothing of yours is left: refused, and the agent's change is kept.
    let output = fixture.command().arg("undo").output().expect("agd undo");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("error: nothing to undo: no change made from the command line"),
        "{stderr}"
    );
    assert!(!stderr.contains("journal_not_found"), "{stderr}");
    assert_eq!(fixture.inbox(), after_agent);
    let output = fixture
        .command()
        .args(["undo", "--json"])
        .output()
        .expect("agd undo --json");
    let error: Value = serde_json::from_slice(&output.stderr).expect("error JSON");
    assert_eq!(error["code"], "journal_not_found");
}

#[test]
fn undo_refuses_a_change_edited_since_and_writes_nothing() {
    let fixture = Fixture::new();
    fixture
        .command()
        .args(["capture", "t", "Buy milk"])
        .assert()
        .success();
    let edited = fixture.inbox().replace("Buy milk", "Buy oat milk");
    fs::write(&fixture.inbox, &edited).expect("hand edit");
    let output = fixture.command().arg("undo").output().expect("agd undo");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.starts_with("error: can't undo"), "{stderr}");
    assert_eq!(fixture.inbox(), edited);
}
