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
                    "roots = [{root:?}]\n",
                    "agenda_files = [{tasks:?}]\n",
                    "editor = [\"true\", \"+{{line}}\", \"{{file}}\"]\n\n",
                    "[[capture]]\n",
                    "key = \"t\"\n",
                    "desc = \"Todo\"\n",
                    "file = {tasks:?}\n",
                    "heading = \"Inbox\"\n",
                    "template = \"* TODO %?\\n%i\"\n",
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
}

#[test]
fn agenda_prints_org_agenda_layout_and_shared_schemas() {
    let fixture = Fixture::new();
    let agenda = fixture
        .command()
        .args(["agenda", "--span", "week", "--from", "2026-10-13", "--json"])
        .output()
        .expect("agenda command");
    assert!(agenda.status.success());
    let rows: Value = serde_json::from_slice(&agenda.stdout).expect("agenda JSON");
    assert_eq!(rows[0]["schema"], "agd.agenda-entry/v1");
    assert_eq!(rows[0]["item"]["schema"], "agd.item/v1");
    assert_eq!(rows[0]["occurrence"]["type"], "scheduled");

    fixture
        .command()
        .args(["agenda", "--span", "day", "--from", "2026-10-13"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Tuesday    13 October 2026"))
        .stdout(predicate::str::contains(
            "  cli:        09:30...... Scheduled:  TODO Renew passport  :admin:",
        ));

    fixture
        .command()
        .args([
            "agenda",
            "--from",
            "2026-10-13",
            "--match",
            "admin",
            "--csv",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Renew passport"));
}

#[test]
fn status_line_counts_today_for_a_status_bar() {
    let fixture = Fixture::new();
    let today = chrono::Local::now().format("%Y-%m-%d %a").to_string();
    fs::write(
        &fixture.tasks,
        format!("* TODO Late\nSCHEDULED: <2020-01-01 Wed>\n* TODO Now\nSCHEDULED: <{today}>\n* Inbox\n** TODO New idea\n"),
    )
    .unwrap();
    fixture
        .command()
        .args(["agenda", "--status"])
        .assert()
        .success()
        .stdout("1 overdue · 1 today · 1 inbox\n");
}

#[test]
fn views_json_carries_what_a_client_draws_them_with() {
    let fixture = Fixture::new();
    let config = fs::read_to_string(&fixture.config).unwrap();
    fs::write(
        &fixture.config,
        format!(
            concat!(
                "start_view = \"w\"\n",
                "theme = \"light\"\n",
                "nerd_font = true\n",
                "todo_keyword_colors = {{ NEXT = \"teal\" }}\n",
                "{config}",
                "[[view]]\n",
                "key = \"d\"\n",
                "desc = \"Dashboard\"\n",
                "blocks = [{{ kind = \"todo\", match = \"NEXT\" }}]\n",
                "[[view]]\n",
                "key = \"w\"\n",
                "desc = \"Work\"\n",
                "blocks = [{{ kind = \"tags-todo\", match = \"work\" }}]\n",
            ),
            config = config,
        ),
    )
    .unwrap();
    let output = fixture
        .command()
        .args(["agenda", "--views", "--json"])
        .output()
        .expect("views command");
    assert!(output.status.success(), "{output:?}");
    let payload: Value = serde_json::from_slice(&output.stdout).expect("views JSON");
    // start_view first, then config order.
    assert_eq!(payload["views"][0]["key"], "w");
    assert_eq!(payload["views"][0]["start"], true);
    assert_eq!(payload["views"][1]["key"], "d");
    assert_eq!(payload["theme"], "light");
    assert_eq!(payload["nerd_font"], true);
    // The file's #+TODO keywords: a configured color, the sequence's first
    // keyword orange, done green.
    assert_eq!(
        payload["keyword_colors"],
        serde_json::json!({ "DONE": "green", "NEXT": "teal", "TODO": "orange" })
    );
}

#[test]
fn capture_parses_tokens_and_keeps_body_literal() {
    let fixture = Fixture::new();
    let before = fs::read(&fixture.tasks).expect("pre-image");
    fixture
        .command()
        .args([
            "capture",
            "t",
            "Call embassy due:2026-10-30 +admin #A",
            "--body",
            "%(never-evaluate)",
            "--dry-run",
        ])
        .assert()
        .success()
        // Tags align to tags_column (-77), as an edit writes them.
        .stdout(predicate::str::is_match(r"\+\*\* TODO \[#A\] Call embassy {2,}:admin:\n").unwrap())
        .stdout(predicate::str::contains("%(never-evaluate)"))
        .stdout(predicate::str::contains("DEADLINE: <2026-10-30 Fri>"));
    assert_eq!(fs::read(&fixture.tasks).expect("post-preview"), before);
}

#[test]
fn help_lists_only_the_five_surfaces() {
    let assertion = cargo::cargo_bin_cmd!("agd")
        .arg("--help")
        .assert()
        .success();
    let help = String::from_utf8_lossy(&assertion.get_output().stdout).into_owned();
    for command in ["agenda", "capture", "mcp", "setup", "--pane"] {
        assert!(help.contains(command), "missing {command} in\n{help}");
    }
    for removed in ["todo", "state", "schedule", "outline", "search"] {
        assert!(
            !help.contains(&format!("  {removed} ")),
            "{removed} still listed"
        );
    }
}

/// A home with `<config>/agd/config.toml` capturing into `org/inbox.org`.
#[cfg(not(target_os = "windows"))]
fn xdg_home(config: &str) -> tempfile::TempDir {
    let home = tempfile::tempdir().expect("home directory");
    let org = home.path().join("org");
    fs::create_dir(&org).expect("org directory");
    fs::write(org.join("inbox.org"), "* Inbox\n").expect("inbox");
    let config_dir = home.path().join(config).join("agd");
    fs::create_dir_all(&config_dir).expect("config directory");
    fs::write(
        config_dir.join("config.toml"),
        format!(
            concat!(
                "roots = [{org:?}]\n\n",
                "[[capture]]\n",
                "key = \"t\"\n",
                "desc = \"Todo\"\n",
                "file = \"inbox.org\"\n",
                "heading = \"Inbox\"\n",
                "template = \"* TODO %?\"\n",
            ),
            org = org.to_string_lossy(),
        ),
    )
    .expect("config");
    home
}

#[cfg(not(target_os = "windows"))]
#[test]
fn default_paths_follow_xdg_on_every_unix() {
    // Without XDG variables: ~/.config/agd/config.toml and ~/.local/share/agd.
    let home = xdg_home(".config");
    cargo::cargo_bin_cmd!("agd")
        .env("HOME", home.path())
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_DATA_HOME")
        .args(["capture", "t", "Water plants"])
        .assert()
        .success();
    let inbox = fs::read_to_string(home.path().join("org/inbox.org")).expect("inbox");
    assert!(inbox.contains("** TODO Water plants"), "{inbox}");
    assert!(home.path().join(".local/share/agd/journal.jsonl").is_file());

    // XDG_CONFIG_HOME and XDG_DATA_HOME win when they are absolute.
    let home = xdg_home("xdg-config");
    cargo::cargo_bin_cmd!("agd")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("xdg-config"))
        .env("XDG_DATA_HOME", home.path().join("xdg-data"))
        .args(["capture", "t", "Stretch"])
        .assert()
        .success();
    let inbox = fs::read_to_string(home.path().join("org/inbox.org")).expect("inbox");
    assert!(inbox.contains("** TODO Stretch"), "{inbox}");
    assert!(home.path().join("xdg-data/agd/journal.jsonl").is_file());
}

#[test]
fn import_without_a_path_asks_for_the_editor_config() {
    let home = tempfile::tempdir().expect("home directory");
    fs::create_dir_all(home.path().join(".config/nvim")).expect("nvim config");
    cargo::cargo_bin_cmd!("agd")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .args(["setup", "import"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("agd setup import ~/.config/nvim"));
}
