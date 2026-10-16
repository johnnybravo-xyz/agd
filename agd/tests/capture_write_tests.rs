use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo;

/// A home directory with `org/` and `notes/` roots and a config.toml that
/// lists `templates` (TOML `[[capture]]` tables) after the roots.
struct Home {
    directory: tempfile::TempDir,
}

impl Home {
    fn new(templates: &str) -> Self {
        let directory = tempfile::tempdir().expect("temporary directory");
        let home = directory.path().join("home");
        fs::create_dir_all(home.join("org")).expect("org root");
        fs::create_dir_all(home.join("notes")).expect("notes root");
        fs::create_dir_all(directory.path().join("config/agd")).expect("config directory");
        fs::write(
            directory.path().join("config/agd/config.toml"),
            format!(
                "roots = [{:?}, {:?}]\n\n{templates}",
                home.join("org"),
                home.join("notes"),
            ),
        )
        .expect("config");
        Self { directory }
    }

    fn home(&self) -> PathBuf {
        self.directory.path().join("home")
    }

    fn agd(&self) -> assert_cmd::Command {
        let mut command = cargo::cargo_bin_cmd!("agd");
        command.env("HOME", self.home());
        command.env("XDG_CONFIG_HOME", self.directory.path().join("config"));
        command.env("XDG_DATA_HOME", self.directory.path().join("data"));
        command
    }
}

fn entries(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(directory)
        .expect("read directory")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

#[test]
fn a_tilde_capture_file_expands_like_roots_and_a_dry_run_creates_nothing() {
    let home = Home::new(
        "[[capture]]\nkey = \"t\"\ndesc = \"Todo\"\nfile = \"~/org/inbox.org\"\nheading = \"Inbox\"\ntemplate = \"* TODO %?\"\n",
    );
    home.agd()
        .args(["capture", "t", "Preview", "--dry-run"])
        .assert()
        .success();
    assert!(entries(&home.home().join("org")).is_empty());
    assert!(!home.directory.path().join("data").exists());

    home.agd().args(["capture", "t", "Real"]).assert().success();
    assert_eq!(entries(&home.home().join("org")), ["inbox.org"]);
    assert_eq!(
        fs::read_to_string(home.home().join("org/inbox.org")).expect("inbox"),
        "* Inbox\n** TODO Real\n"
    );
    assert_eq!(entries(&home.home()), ["notes", "org"]);
}

#[test]
fn a_relative_template_file_is_read_under_the_first_root() {
    let home = Home::new(
        "[[capture]]\nkey = \"n\"\ndesc = \"Note\"\nfile = \"~/notes/notes.org\"\ntemplate_file = \"templates/note.txt\"\n",
    );
    fs::create_dir_all(home.home().join("org/templates")).expect("templates");
    fs::write(
        home.home().join("org/templates/note.txt"),
        "* %? :note:\nFrom the template file.",
    )
    .expect("template file");
    home.agd()
        .args(["capture", "n", "Read this"])
        .assert()
        .success();
    let written = fs::read_to_string(home.home().join("notes/notes.org")).expect("notes");
    assert!(
        written.starts_with("* Read this")
            && written.ends_with(":note:\nFrom the template file.\n"),
        "{written:?}"
    );
}
