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

    // As org-batch-agenda prints it: the span header, ` 9:30` without a
    // leading zero, and tags ending at column 79.
    fixture
        .command()
        .args(["agenda", "--span", "day", "--from", "2026-10-13"])
        .assert()
        .success()
        .stdout(concat!(
            "Day-agenda (W42):\n",
            "Tuesday    13 October 2026\n",
            "  cli:         9:30...... Scheduled:  TODO Renew passport               :admin:\n",
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
fn agenda_span_takes_a_number_of_days_from_config_or_flag() {
    let fixture = Fixture::new();
    let config = fs::read_to_string(&fixture.config).unwrap();
    fs::write(&fixture.config, format!("agenda_span = 3\n{config}")).unwrap();
    let output = fixture
        .command()
        .args(["agenda", "--from", "2026-10-12"])
        .output()
        .expect("agenda command");
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.starts_with("3 days-agenda (W42):\nMonday     12 October 2026"),
        "{text}"
    );
    assert!(text.contains("Wednesday  14 October 2026"), "{text}");
    assert!(!text.contains("Thursday"), "{text}");

    fixture
        .command()
        .args(["agenda", "--span", "2", "--from", "2026-10-13"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Wednesday  14 October 2026"))
        .stdout(predicate::str::contains("Thursday").not());

    fs::write(&fixture.config, format!("agenda_span = \"wek\"\n{config}")).unwrap();
    fixture
        .command()
        .args(["agenda"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "expected day, week, month or a number of days, not \"wek\"",
        ));
}

#[test]
fn builtin_n_view_runs_beside_configured_views() {
    let fixture = Fixture::new();
    let config = fs::read_to_string(&fixture.config).unwrap();
    fs::write(
        &fixture.config,
        format!(
            "{config}[[view]]\nkey = \"w\"\ndesc = \"Work\"\nblocks = [{{ kind = \"tags-todo\", match = \"work\" }}]\n"
        ),
    )
    .unwrap();
    fixture
        .command()
        .args(["agenda", "--view", "n", "--from", "2026-10-13"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Week-agenda (W42):"))
        .stdout(predicate::str::contains("NEXT File paperwork"));
    fixture
        .command()
        .args(["agenda", "--views"])
        .assert()
        .success()
        .stdout("w   Work\nn   Agenda and all TODOs  (built-in)\n");
    fixture
        .command()
        .args(["agenda", "--view", "x"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown view 'x'; views: w, n"));
}

#[test]
fn from_takes_a_negative_offset_as_its_value() {
    let fixture = Fixture::new();
    let week_ago = (chrono::Local::now().date_naive() - chrono::Duration::weeks(1))
        .format("%Y-%m-%d")
        .to_string();
    fs::write(
        &fixture.tasks,
        format!("* DONE Last week\nSCHEDULED: <{week_ago}>\n"),
    )
    .unwrap();
    for offset in ["-1w", "-7"] {
        fixture
            .command()
            .args(["agenda", "--span", "day", "--brief", "--from", offset])
            .assert()
            .success()
            .stdout(format!("{week_ago} Last week\n"));
    }
}

#[test]
fn json_mode_reports_every_error_as_agd_error() {
    let fixture = Fixture::new();
    let failures: [(&[&str], &str); 7] = [
        (&["agenda", "--view", "zz", "--json"], "unknown_view"),
        (&["agenda", "--view", "--json"], "no_start_view"),
        (
            &["agenda", "--profile", "nope", "--json"],
            "unknown_profile",
        ),
        (&["agenda", "--from", "someday", "--json"], "invalid_date"),
        (&["capture", "--json"], "usage"),
        (
            &["capture", "t", "Hi", "--prompt", "bad", "--json"],
            "invalid_prompt",
        ),
        // Clap refuses this before any command runs.
        (&["agenda", "--bogus", "--json"], "usage"),
    ];
    for (arguments, code) in failures {
        let output = fixture.command().args(arguments).output().expect("agd");
        assert!(!output.status.success(), "{arguments:?}");
        let error: Value = serde_json::from_slice(&output.stderr)
            .unwrap_or_else(|_| panic!("{arguments:?}: {output:?}"));
        assert_eq!(error["schema"], "agd.error/v1", "{arguments:?}");
        assert_eq!(error["code"], code, "{arguments:?}: {error}");
    }
    // Without --json the same error is a plain line.
    fixture
        .command()
        .args(["agenda", "--view", "zz"])
        .assert()
        .failure()
        .stderr(predicate::str::starts_with("Error: unknown view 'zz'"));
}

#[test]
fn errors_name_the_valid_choices() {
    let fixture = Fixture::new();
    let config = fs::read_to_string(&fixture.config).unwrap();
    fs::write(
        &fixture.config,
        format!(
            concat!(
                "{config}",
                "[[capture]]\n",
                "key = \"m\"\n",
                "desc = \"Meeting\"\n",
                "file = \"tasks.org\"\n",
                "template = \"* Meet %^{{Who}} about %?\"\n",
                "[[agent_profile]]\n",
                "name = \"coding\"\n",
            ),
            config = config,
        ),
    )
    .unwrap();
    let failures: [(&[&str], &str); 3] = [
        (&["capture", "x", "Buy milk"], "templates: t, m"),
        (&["agenda", "--profile", "nope"], "profiles: coding"),
        (&["capture", "m", "Budget"], "--prompt 'Who=...'"),
    ];
    for (arguments, hint) in failures {
        fixture
            .command()
            .args(arguments)
            .assert()
            .failure()
            .stderr(predicate::str::contains(hint));
    }
}

#[test]
fn flags_a_command_would_ignore_are_usage_errors() {
    let fixture = Fixture::new();
    let refused: [&[&str]; 11] = [
        &["agenda", "--view", "n", "--brief"],
        &["agenda", "--view", "n", "--profile", "work"],
        &["agenda", "--status", "--span", "week"],
        &["agenda", "--status", "--from", "fri"],
        &["agenda", "--status", "--match", "work"],
        &["agenda", "--views", "--span", "week"],
        &["agenda", "--views", "--log"],
        &["capture", "-i", "--dry-run"],
        &["capture", "-i", "--prompt", "Who=me"],
        &["mcp", "--log-level", "bogus"],
        &["--pane", "agenda"],
    ];
    for arguments in refused {
        let output = fixture.command().args(arguments).output().expect("agd");
        assert_eq!(output.status.code(), Some(2), "{arguments:?}: {output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("cannot be used with")
                || stderr.contains("invalid value 'bogus'")
                || stderr.contains("takes no command"),
            "{arguments:?}: {stderr}"
        );
    }
}

#[test]
fn status_line_counts_today_for_a_status_bar() {
    let fixture = Fixture::new();
    let now = chrono::Local::now().date_naive();
    let today = now.format("%Y-%m-%d %a").to_string();
    // A deadline in four days shows today as a warning, but is not today's.
    let soon = (now + chrono::Duration::days(4))
        .format("%Y-%m-%d %a")
        .to_string();
    fs::write(
        &fixture.tasks,
        format!("* TODO Late\nSCHEDULED: <2020-01-01 Wed>\n* TODO Now\nSCHEDULED: <{today}>\n* TODO Soon\nDEADLINE: <{soon}>\n* Inbox\n** TODO New idea\n"),
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
    // start_view first, then config order, then Org's built-in n.
    assert_eq!(payload["views"][0]["key"], "w");
    assert_eq!(payload["views"][0]["start"], true);
    assert_eq!(payload["views"][0]["builtin"], false);
    assert_eq!(payload["views"][1]["key"], "d");
    assert_eq!(payload["views"][2]["key"], "n");
    assert_eq!(payload["views"][2]["builtin"], true);
    assert_eq!(payload["views"].as_array().unwrap().len(), 3);
    assert_eq!(payload["theme"], "light");
    assert_eq!(payload["nerd_font"], true);
    assert_eq!(payload["schema"], "agd.views/v1");

    // Every section names its schema and carries groups, empty or not.
    let output = fixture
        .command()
        .args(["agenda", "--view", "w", "--json"])
        .output()
        .expect("view command");
    assert!(output.status.success(), "{output:?}");
    let sections: Value = serde_json::from_slice(&output.stdout).expect("view JSON");
    assert_eq!(sections[0]["schema"], "agd.view-section/v1");
    assert_eq!(sections[0]["kind"], "tags-todo");
    assert_eq!(sections[0]["groups"], serde_json::json!([]));
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
        .stdout(predicate::str::contains(
            "** TODO [#A] Call embassy :admin:",
        ))
        .stdout(predicate::str::contains("%(never-evaluate)"))
        .stdout(predicate::str::contains("DEADLINE: <2026-10-30 Fri>"));
    assert_eq!(fs::read(&fixture.tasks).expect("post-preview"), before);
}

#[test]
fn capture_takes_a_key_and_unquoted_words_or_the_text_alone() {
    let fixture = Fixture::new();
    // Unquoted words after the key are the text.
    fixture
        .command()
        .args([
            "capture",
            "t",
            "Water",
            "the",
            "plants",
            "due:fri",
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("** TODO Water the plants\n"));
    // One argument is the text for the first template; a pipe gets the ref.
    let output = fixture
        .command()
        .args(["capture", "Renew visa due:fri"])
        .output()
        .expect("capture");
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.starts_with(
            &fixture
                .tasks
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        ),
        "{stdout}"
    );
    assert_eq!(stdout.lines().count(), 1, "{stdout}");
    assert!(
        fs::read_to_string(&fixture.tasks)
            .unwrap()
            .contains("** TODO Renew visa\nDEADLINE: <")
    );
    // Unquoted words without a key would file the first word as a key.
    fixture
        .command()
        .args(["capture", "Renew", "visa"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "quote the text: agd capture \"Renew visa\"",
        ));
    fixture
        .command()
        .args(["capture", "t"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("needs text"));
}

#[cfg(unix)]
#[test]
fn a_closed_stdout_ends_the_command_quietly() {
    let fixture = Fixture::new();
    for arguments in [
        &["agenda", "--span", "month"][..],
        &["agenda", "--csv"],
        &["capture", "t", "Quiet", "--dry-run"],
        &["setup", "check"],
    ] {
        let mut command = std::process::Command::new(assert_cmd::cargo::cargo_bin!("agd"));
        command
            .arg("--config")
            .arg(&fixture.config)
            .env("XDG_DATA_HOME", fixture.directory.path().join("data"))
            .args(arguments)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = command.spawn().expect("agd");
        // Close the reading end before agd writes, like `| head` exiting.
        drop(child.stdout.take());
        let output = child.wait_with_output().expect("agd exit");
        assert!(output.status.success(), "{arguments:?}: {output:?}");
        assert_eq!(String::from_utf8_lossy(&output.stderr), "", "{arguments:?}");
    }
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
    assert!(help.contains("start_view"), "{help}");

    let assertion = cargo::cargo_bin_cmd!("agd")
        .args(["agenda", "--help"])
        .assert()
        .success();
    let help = String::from_utf8_lossy(&assertion.get_output().stdout).into_owned();
    for hint in ["built-in n", "C-c a m", "[[agent_profile]]"] {
        assert!(help.contains(hint), "missing {hint} in\n{help}");
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

#[cfg(not(target_os = "windows"))]
#[test]
fn first_run_without_config_or_org_says_how_to_start() {
    let home = tempfile::tempdir().expect("home directory");
    let config = home.path().join(".config/agd/config.toml");
    cargo::cargo_bin_cmd!("agd")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_DATA_HOME", home.path().join(".local/share"))
        .args(["agenda"])
        .assert()
        .failure()
        .stderr(format!(
            concat!(
                "Error: no config at {} and no ~/org to read. Start with one of:\n",
                "  mkdir ~/org    agd then reads it with its defaults\n",
                "  agd setup import <your Emacs or Neovim config directory>\n",
                "  agd -r DIR    read another directory for this run\n",
            ),
            config.display()
        ));
    let output = cargo::cargo_bin_cmd!("agd")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .args(["agenda", "--json"])
        .output()
        .expect("agenda command");
    let error: Value = serde_json::from_slice(&output.stderr).expect("error JSON");
    assert_eq!(error["code"], "config");

    // `mkdir ~/org` is enough.
    fs::create_dir(home.path().join("org")).expect("org directory");
    cargo::cargo_bin_cmd!("agd")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_DATA_HOME", home.path().join(".local/share"))
        .args(["agenda", "--views"])
        .assert()
        .success()
        .stdout("n   Agenda and all TODOs  (built-in)\n");
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
