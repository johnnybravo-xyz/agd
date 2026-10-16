use std::fs;

use assert_cmd::cargo;
use serde_json::Value;

/// agd with no user config or journal: HOME and the XDG directories point
/// into the test directory, so the developer's own files are never read.
fn agd(directory: &tempfile::TempDir) -> assert_cmd::Command {
    let mut command = cargo::cargo_bin_cmd!("agd");
    command.env("HOME", directory.path().join("home"));
    command.env("XDG_CONFIG_HOME", directory.path().join("config"));
    command.env("XDG_DATA_HOME", directory.path().join("data"));
    command
}

#[test]
fn check_uses_configured_root_and_emits_json() {
    let directory = tempfile::tempdir().expect("temporary directory");
    fs::write(
        directory.path().join("tasks.org"),
        b"#+TODO: OPEN(o) | CLOSED(c)\r\n* OPEN Task\r\n",
    )
    .expect("org fixture");
    fs::write(directory.path().join("ignored.txt"), "* Ignored\n").expect("ignored fixture");

    let assertion = agd(&directory)
        .arg("--root-directory")
        .arg(directory.path())
        .args(["setup", "check"])
        .arg("--json")
        .assert()
        .success();
    let report: Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("JSON check report");

    assert_eq!(report["schema"], "agd.check/v1");
    assert_eq!(report["summary"]["files"], 1);
    assert_eq!(report["summary"]["headlines"], 1);
    assert_eq!(report["summary"]["errors"], 0);
}

#[test]
fn an_interrupted_write_is_reported_resolved_and_never_blocks_reads() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let tasks = directory.path().join("tasks.org");
    let bytes = "* TODO Changed by hand\nSCHEDULED: <2026-10-16 Fri>\n";
    fs::write(&tasks, bytes).expect("org fixture");
    let journal = directory.path().join("data/agd/journal.jsonl");
    fs::create_dir_all(journal.parent().unwrap()).expect("data directory");
    let record = serde_json::json!({
        "schema": "agd.journal/v1",
        "id": "20261016T090000.000000000-1",
        "status": "pending",
        "timestamp": "2026-10-16T09:00:00+00:00",
        "actor": "cli",
        "project": "fixture",
        "file": fs::canonicalize(&tasks).unwrap(),
        "item_ref": null,
        "pre_hash": "0".repeat(64),
        "post_hash": "1".repeat(64),
        "patches": [],
        "diff": "",
    });
    fs::write(&journal, format!("{record}\n")).expect("journal fixture");

    agd(&directory)
        .arg("--root-directory")
        .arg(directory.path())
        .args(["agenda", "--status"])
        .assert()
        .success();
    let assertion = agd(&directory)
        .arg("--root-directory")
        .arg(directory.path())
        .args(["setup", "check", "--json"])
        .assert()
        .code(1);
    let report: Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("JSON check report");
    assert_eq!(report["problems"][0]["code"], "journal_conflict");

    let assertion = agd(&directory)
        .arg("--root-directory")
        .arg(directory.path())
        .args(["setup", "check", "--resolve"])
        .assert()
        .success();
    let output = String::from_utf8_lossy(&assertion.get_output().stdout).into_owned();
    assert!(
        output.contains("resolved journal entry 20261016T090000.000000000-1"),
        "{output}"
    );
    assert_eq!(fs::read_to_string(&tasks).expect("tasks"), bytes);
}

#[test]
fn check_reports_duplicate_ids_and_exits_one() {
    let directory = tempfile::tempdir().expect("temporary directory");
    fs::write(
        directory.path().join("first.org"),
        "* First\n:PROPERTIES:\n:ID: same-id\n:END:\n",
    )
    .expect("first fixture");
    fs::write(
        directory.path().join("second.org"),
        "* Second\n:PROPERTIES:\n:ID: same-id\n:END:\n",
    )
    .expect("second fixture");

    let assertion = agd(&directory)
        .arg("--root-directory")
        .arg(directory.path())
        .args(["setup", "check"])
        .arg("--json")
        .assert()
        .code(1);
    let report: Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("JSON check report");

    assert_eq!(report["summary"]["errors"], 1);
    assert_eq!(report["problems"][0]["code"], "duplicate_id");
    assert_eq!(report["problems"][0]["line"], 1);
}
