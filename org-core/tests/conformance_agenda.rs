use std::fs;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use org_core::{AgendaEngine, AgendaQuery, Settings, WorkspaceIndex, agenda_csv};
use serde::Deserialize;
use similar::TextDiff;

#[derive(Debug, Deserialize)]
struct Case {
    today: String,
    from: String,
    to: String,
    #[serde(default)]
    skip_scheduled_if_done: bool,
    #[serde(default)]
    skip_deadline_if_done: bool,
    #[serde(default)]
    skip_timestamp_if_done: bool,
    /// Agenda sorting keys; Org 9.6 in the oracle has no urgency-* keys.
    sorting_strategy: Option<Vec<String>>,
}

fn date(value: &str) -> NaiveDate {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").expect("valid case date")
}

fn case_directories(root: &Path) -> Vec<PathBuf> {
    let mut cases: Vec<PathBuf> = fs::read_dir(root)
        .expect("read agenda cases")
        .map(|entry| entry.expect("read agenda case").path())
        .filter(|path| path.join("case.toml").is_file())
        .collect();
    cases.sort();
    cases
}

#[test]
fn native_agenda_matches_pinned_emacs_rows() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../conformance/agenda");
    let cases = case_directories(&root);
    assert!(
        !cases.is_empty(),
        "agenda conformance cases must not be empty"
    );

    for directory in cases {
        let case: Case = toml::from_str(
            &fs::read_to_string(directory.join("case.toml")).expect("read case.toml"),
        )
        .expect("parse case.toml");
        let mut settings = Settings::default();
        settings.files.roots = vec![directory.to_string_lossy().into_owned()];
        settings.files.agenda = vec!["*.org".to_string(), "**/*.org".to_string()];
        settings.agenda.skip_scheduled_if_done = case.skip_scheduled_if_done;
        settings.agenda.skip_deadline_if_done = case.skip_deadline_if_done;
        settings.agenda.skip_timestamp_if_done = case.skip_timestamp_if_done;
        if let Some(strategy) = case.sorting_strategy {
            settings.agenda.sorting_strategy = strategy;
        }
        let index = WorkspaceIndex::load(settings).expect("index case");
        let entries = AgendaEngine::new(&index)
            .agenda(&AgendaQuery {
                from: date(&case.from),
                to: date(&case.to),
                today: date(&case.today),
                match_expression: None,
                log_mode: false,
            })
            .expect("run native agenda");
        let actual = agenda_csv(&entries);
        let expected = fs::read_to_string(directory.join("emacs.csv"))
            .expect("read Emacs rows")
            .replace("\r\n", "\n");
        assert!(
            actual == expected,
            "agenda drift in {}\n{}",
            directory.display(),
            TextDiff::from_lines(&expected, &actual).unified_diff()
        );
    }
}
