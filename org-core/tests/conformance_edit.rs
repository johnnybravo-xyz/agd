use std::fs;
use std::path::Path;

use chrono::NaiveDateTime;
use org_core::{Change, EditIntent, PlanningTimestamp, Settings, WriteContext, WriteEngine};
use serde::Deserialize;

/// conformance/edit/<case>/case.toml; expected.org is written by
/// scripts/emacs-edit-oracle.py from the same input, settings and clock.
#[derive(Deserialize)]
struct Case {
    operation: String,
    value: String,
    now: String,
    #[serde(default = "nil")]
    log_done: String,
    #[serde(default = "nil")]
    log_repeat: String,
    #[serde(default = "nil")]
    log_reschedule: String,
    #[serde(default = "nil")]
    log_redeadline: String,
    #[serde(default)]
    log_into_drawer: bool,
}

fn nil() -> String {
    "nil".to_string()
}

fn date(value: &str) -> PlanningTimestamp {
    PlanningTimestamp {
        date: value.to_string(),
        time: None,
        end_time: None,
        repeater: None,
        warning: None,
        raw: None,
    }
}

fn intent(case: &Case) -> EditIntent {
    let value = case.value.clone();
    match case.operation.as_str() {
        "state" => EditIntent {
            state: Change::Set(value),
            ..EditIntent::default()
        },
        "schedule" => EditIntent {
            scheduled: Change::Set(date(&value)),
            ..EditIntent::default()
        },
        "deadline" => EditIntent {
            deadline: Change::Set(date(&value)),
            ..EditIntent::default()
        },
        "priority" => EditIntent {
            priority: Change::Set(value),
            ..EditIntent::default()
        },
        operation => panic!("unsupported operation {operation}"),
    }
}

#[test]
fn native_edits_match_pinned_emacs_results_byte_for_byte() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../conformance/edit");
    let mut cases: Vec<_> = fs::read_dir(&root)
        .expect("read edit cases")
        .map(|entry| entry.expect("edit case").path())
        .filter(|path| path.join("case.toml").is_file())
        .collect();
    cases.sort();
    assert!(
        !cases.is_empty(),
        "edit conformance cases must not be empty"
    );

    for directory in cases {
        let name = directory
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let case: Case =
            toml::from_str(&fs::read_to_string(directory.join("case.toml")).unwrap()).unwrap();
        let temp = tempfile::tempdir().expect("create edit case");
        let path = temp.path().join("input.org");
        fs::copy(directory.join("input.org"), &path).expect("copy edit source");
        let mut settings = Settings::default();
        settings.files.roots = vec![temp.path().to_string_lossy().into_owned()];
        settings.files.agenda = vec!["*.org".to_string()];
        settings.agenda.log_done = case.log_done.clone();
        settings.agenda.log_repeat = case.log_repeat.clone();
        settings.agenda.log_reschedule = case.log_reschedule.clone();
        settings.agenda.log_redeadline = case.log_redeadline.clone();
        settings.agenda.log_into_drawer = case.log_into_drawer;
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl"))
            .expect("create write engine");
        let context = WriteContext {
            actor: "conformance".to_string(),
            project: "conformance".to_string(),
            now: Some(NaiveDateTime::parse_from_str(&case.now, "%Y-%m-%d %H:%M").unwrap()),
        };
        engine
            .edit("task-id", &intent(&case), false, &context)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let actual = fs::read_to_string(&path).expect("read native edit");
        let expected = fs::read_to_string(directory.join("expected.org")).expect("read Emacs edit");
        assert!(
            actual == expected,
            "edit drift for {name}\n{}",
            similar::TextDiff::from_lines(&expected, &actual).unified_diff()
        );
    }
}
