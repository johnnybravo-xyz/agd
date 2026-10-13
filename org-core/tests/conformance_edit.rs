use std::fs;
use std::path::Path;

use org_core::{Change, EditIntent, PlanningTimestamp, Settings, WriteContext, WriteEngine};

fn timestamp() -> PlanningTimestamp {
    PlanningTimestamp {
        date: "2026-10-30".to_string(),
        time: None,
        end_time: None,
        repeater: None,
        warning: None,
        raw: None,
    }
}

fn context() -> WriteContext {
    WriteContext {
        actor: "conformance".to_string(),
        project: "conformance".to_string(),
    }
}

#[test]
fn native_edits_match_pinned_emacs_results_byte_for_byte() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../conformance/edit");
    let operations = [
        (
            "state",
            EditIntent {
                state: Change::Set("DONE".to_string()),
                ..EditIntent::default()
            },
        ),
        (
            "schedule",
            EditIntent {
                scheduled: Change::Set(timestamp()),
                ..EditIntent::default()
            },
        ),
        (
            "deadline",
            EditIntent {
                deadline: Change::Set(timestamp()),
                ..EditIntent::default()
            },
        ),
        (
            "priority",
            EditIntent {
                priority: Change::Set("A".to_string()),
                ..EditIntent::default()
            },
        ),
    ];

    for (operation, intent) in operations {
        let temp = tempfile::tempdir().expect("create edit case");
        let path = temp.path().join("input.org");
        fs::copy(root.join("base.org"), &path).expect("copy edit source");
        let mut settings = Settings::default();
        settings.files.roots = vec![temp.path().to_string_lossy().into_owned()];
        settings.files.agenda = vec!["*.org".to_string()];
        settings.agenda.log_done = "nil".to_string();
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl"))
            .expect("create write engine");
        engine
            .edit("task-id", &intent, false, &context())
            .unwrap_or_else(|error| panic!("{operation}: {error}"));
        let actual = fs::read(&path).expect("read native edit");
        let expected = fs::read(root.join("expected").join(format!("{operation}.org")))
            .expect("read Emacs edit");
        assert_eq!(actual, expected, "edit drift for {operation}");
    }
}
