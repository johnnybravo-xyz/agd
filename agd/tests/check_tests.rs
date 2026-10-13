use std::fs;

use assert_cmd::cargo;
use serde_json::Value;

#[test]
fn check_uses_configured_root_and_emits_json() {
    let directory = tempfile::tempdir().expect("temporary directory");
    fs::write(
        directory.path().join("tasks.org"),
        b"#+TODO: OPEN(o) | CLOSED(c)\r\n* OPEN Task\r\n",
    )
    .expect("org fixture");
    fs::write(directory.path().join("ignored.txt"), "* Ignored\n").expect("ignored fixture");

    let assertion = cargo::cargo_bin_cmd!("agd")
        .arg("--root-directory")
        .arg(directory.path())
        .arg("check")
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

    let assertion = cargo::cargo_bin_cmd!("agd")
        .arg("--root-directory")
        .arg(directory.path())
        .arg("check")
        .arg("--json")
        .assert()
        .code(1);
    let report: Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("JSON check report");

    assert_eq!(report["summary"]["errors"], 1);
    assert_eq!(report["problems"][0]["code"], "duplicate_id");
    assert_eq!(report["problems"][0]["line"], 1);
}
