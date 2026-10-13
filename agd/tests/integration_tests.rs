use std::fs;

use assert_cmd::cargo;
use predicates::prelude::*;
use serde_json::Value;

struct Fixture {
    directory: tempfile::TempDir,
    config: std::path::PathBuf,
    tasks: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("temporary directory");
        let tasks = directory.path().join("tasks.org");
        fs::write(
            &tasks,
            concat!(
                "#+TODO: TODO NEXT | DONE\n",
                "#+CATEGORY: cli\n",
                "* TODO Renew passport :admin:\n",
                "SCHEDULED: <2026-10-13 Tue 09:30>\n",
                "DEADLINE: <2026-10-20 Tue -3d>\n",
                "* NEXT File paperwork :work:\n",
                "* Inbox\n",
            ),
        )
        .expect("org fixture");
        let config = directory.path().join("config.toml");
        fs::write(
            &config,
            format!(
                concat!(
                    "[files]\n",
                    "roots = [{root:?}]\n",
                    "agenda = [{tasks:?}]\n\n",
                    "[capture.t]\n",
                    "desc = \"Todo\"\n",
                    "file = {tasks:?}\n",
                    "heading = \"Inbox\"\n",
                    "template = \"* TODO %?\\n%i\"\n\n",
                    "[editor]\n",
                    "command = \"true\"\n",
                    "args = [\"+{{line}}\", \"{{file}}\"]\n",
                ),
                root = directory.path().to_string_lossy(),
                tasks = tasks.to_string_lossy(),
            ),
        )
        .expect("config fixture");
        Self {
            directory,
            config,
            tasks,
        }
    }

    fn command(&self) -> assert_cmd::Command {
        let mut command = cargo::cargo_bin_cmd!("agd");
        command.arg("--config").arg(&self.config);
        command.env("XDG_DATA_HOME", self.directory.path().join("data"));
        command
    }

    fn first_ref(&self) -> String {
        let output = self
            .command()
            .args(["todo", "--json"])
            .output()
            .expect("todo command");
        assert!(output.status.success());
        let items: Value = serde_json::from_slice(&output.stdout).expect("todo JSON");
        items[0]["ref"].as_str().expect("item ref").to_string()
    }
}

#[test]
fn agenda_todo_and_query_emit_shared_native_schemas() {
    let fixture = Fixture::new();
    let agenda = fixture
        .command()
        .args(["agenda", "--span", "week", "--from", "2026-10-13", "--json"])
        .output()
        .expect("agenda command");
    assert!(agenda.status.success());
    let rows: Value = serde_json::from_slice(&agenda.stdout).expect("agenda JSON");
    assert_eq!(rows[0]["schema"], "agendum.agenda-entry/v1");
    assert_eq!(rows[0]["item"]["schema"], "agendum.item/v1");
    assert_eq!(rows[0]["occurrence"]["type"], "scheduled");

    fixture
        .command()
        .args(["q", "(and (todo \"NEXT\") (tags \"work\"))"])
        .assert()
        .success()
        .stdout(predicate::str::contains("File paperwork"))
        .stdout(predicate::str::contains("Renew passport").not());
}

#[test]
fn write_preview_is_exact_and_does_not_mutate() {
    let fixture = Fixture::new();
    let before = fs::read(&fixture.tasks).expect("pre-image");
    fixture
        .command()
        .args(["state", &fixture.first_ref(), "DONE", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("-* TODO Renew passport"))
        .stdout(predicate::str::contains("+* DONE Renew passport"));
    assert_eq!(fs::read(&fixture.tasks).expect("post-preview"), before);
}

#[test]
fn stale_write_returns_error_schema() {
    let fixture = Fixture::new();
    let item_ref = fixture.first_ref();
    fixture
        .command()
        .args(["state", &item_ref, "DONE"])
        .assert()
        .success();
    let assertion = fixture
        .command()
        .args(["state", &item_ref, "DONE", "--json"])
        .assert()
        .failure();
    let error: Value =
        serde_json::from_slice(&assertion.get_output().stderr).expect("error schema");
    assert_eq!(error["schema"], "agendum.error/v1");
    assert_eq!(error["code"], "stale_ref");
}

#[test]
fn capture_uses_configured_template_and_keeps_body_literal() {
    let fixture = Fixture::new();
    fixture
        .command()
        .args([
            "capture",
            "t",
            "Call embassy",
            "--body",
            "%(never-evaluate)",
            "--deadline",
            "2026-10-30 -7d",
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("** TODO Call embassy"))
        .stdout(predicate::str::contains("%(never-evaluate)"))
        .stdout(predicate::str::contains("DEADLINE: <2026-10-30 Fri -7d>"));
}

#[test]
fn command_help_exposes_the_spec_surface() {
    cargo::cargo_bin_cmd!("agd")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("agenda"))
        .stdout(predicate::str::contains("todo"))
        .stdout(predicate::str::contains("capture"))
        .stdout(predicate::str::contains("state"))
        .stdout(predicate::str::contains("schedule"))
        .stdout(predicate::str::contains("deadline"))
        .stdout(predicate::str::contains("edit"))
        .stdout(predicate::str::contains("open"))
        .stdout(predicate::str::contains("mcp"));
}
