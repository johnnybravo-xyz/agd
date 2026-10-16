use std::fs;
use std::path::{Path, PathBuf};

use agd_core::{
    AgendaEngine, AgendaQuery, Settings, WorkspaceIndex, agenda_csv, agenda_text, item_row,
    items_csv,
};
use chrono::NaiveDate;
use serde::Deserialize;
use similar::TextDiff;

/// One conformance/agenda/<case>/case.toml. emacs.csv and emacs.txt are
/// written by scripts/emacs-agenda-oracle.py from pinned Emacs, and agd's
/// CSV and text must equal them byte for byte.
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
    /// org-agenda log mode: also list CLOSED entries.
    #[serde(default)]
    log: bool,
    /// Agenda sorting keys, as org-agenda-sorting-strategy's agenda entry.
    sorting_strategy: Option<Vec<String>>,
    /// A tags or tags-todo match run instead of the agenda.
    #[serde(rename = "match")]
    match_expression: Option<String>,
    match_kind: Option<String>,
}

/// agd's output for a case and the Emacs fixture it must equal.
struct Comparison {
    file: &'static str,
    actual: String,
    expected: String,
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

fn fixture(directory: &Path, file: &str) -> String {
    fs::read_to_string(directory.join(file))
        .unwrap_or_else(|error| panic!("read {file} in {}: {error}", directory.display()))
        .replace("\r\n", "\n")
}

fn compare(directory: &Path, case: &Case) -> Vec<Comparison> {
    let mut settings = Settings {
        roots: vec![directory.to_string_lossy().into_owned()],
        agenda_files: vec!["*.org".to_string(), "**/*.org".to_string()],
        ..Settings::default()
    };
    settings.skip_scheduled_if_done = case.skip_scheduled_if_done;
    settings.skip_deadline_if_done = case.skip_deadline_if_done;
    settings.skip_timestamp_if_done = case.skip_timestamp_if_done;
    if let Some(strategy) = &case.sorting_strategy {
        settings.sorting_strategy = strategy.clone();
    }
    let index = WorkspaceIndex::load(settings).expect("index case");
    let engine = AgendaEngine::new(&index);
    let today = date(&case.today);
    let csv = |actual| Comparison {
        file: "emacs.csv",
        actual,
        expected: fixture(directory, "emacs.csv"),
    };
    match &case.match_expression {
        Some(expression) => {
            let kind = case.match_kind.as_deref().unwrap_or("tags");
            let items = if kind == "tags-todo" {
                engine.todo(Some(expression), today)
            } else {
                engine.tags(expression, today)
            }
            .expect("run native match");
            // A match's text starts with its header and a `Press C-u r`
            // hint, which agd does not print; the rows must match.
            let text = fixture(directory, "emacs.txt");
            let rows: String = items.iter().map(|item| item_row(item) + "\n").collect();
            vec![
                csv(items_csv(&items, kind, &index)),
                Comparison {
                    file: "emacs.txt",
                    actual: rows,
                    expected: text
                        .lines()
                        .skip(2)
                        .map(|line| format!("{line}\n"))
                        .collect(),
                },
            ]
        }
        None => {
            let (from, to) = (date(&case.from), date(&case.to));
            let entries = engine
                .agenda(&AgendaQuery {
                    from,
                    to,
                    today,
                    match_expression: None,
                    log_mode: case.log,
                })
                .expect("run native agenda");
            vec![
                csv(agenda_csv(&entries, today, &index)),
                Comparison {
                    file: "emacs.txt",
                    actual: agenda_text(&entries, from, to, None),
                    expected: fixture(directory, "emacs.txt"),
                },
            ]
        }
    }
}

#[test]
fn native_agenda_matches_pinned_emacs_rows_and_text() {
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
        for comparison in compare(&directory, &case) {
            assert!(
                comparison.actual == comparison.expected,
                "agenda drift in {}/{}\n{}",
                directory.display(),
                comparison.file,
                TextDiff::from_lines(&comparison.expected, &comparison.actual).unified_diff()
            );
        }
    }
}
