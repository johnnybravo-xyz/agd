use std::fs;
use std::path::Path;

use org_core::{ImportResult, ResolvedEditorConfig, Settings, apply_import};

fn import_fixture(path: &Path, editor: &str) -> ImportResult {
    let temp = tempfile::tempdir().expect("create import root");
    let source_task = path.join("tasks.org");
    let target_task = temp.path().join("tasks.org");
    if source_task.is_file() {
        fs::copy(source_task, &target_task).expect("copy imported agenda file");
    } else {
        fs::write(&target_task, "* TODO Fixture\n").expect("write imported agenda file");
    }
    let raw = fs::read_to_string(path.join("resolved.json")).expect("read resolved fixture");
    let root = temp.path().to_string_lossy();
    let escaped_root = serde_json::to_string(root.as_ref()).expect("encode fixture root");
    let raw = raw.replace("/fixture", &escaped_root[1..escaped_root.len() - 1]);
    let resolved: ResolvedEditorConfig =
        serde_json::from_str(&raw).expect("parse resolved fixture");
    assert_eq!(resolved.editor, editor);
    let base = Settings {
        roots: vec![temp.path().to_string_lossy().into_owned()],
        ..Settings::default()
    };
    apply_import(&resolved, &base).expect("apply resolved fixture")
}

fn reported(result: &ImportResult, key: &str) -> bool {
    result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.key == key)
}

#[test]
fn emacs_fixtures_keep_resolved_ordering_and_report_what_is_left_out() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../conformance/import/emacs");
    for name in ["vanilla", "use-package", "doom"] {
        let result = import_fixture(&root.join(name), "emacs");
        assert!(!result.settings.todo_keywords.is_empty());
        assert_eq!(result.settings.agenda_files.len(), 1);
    }
    let vanilla = import_fixture(&root.join("vanilla"), "emacs");
    assert!(vanilla.settings.capture_template("x").is_none());
    assert!(reported(&vanilla, "capture x"));
    assert_eq!(
        vanilla.settings.view("w").unwrap().blocks[0].kind,
        "tags-todo"
    );
    let block = &vanilla.settings.view("b").unwrap().blocks;
    assert_eq!(block[0].span.as_deref(), Some("day"));
    assert_eq!(block[1].header.as_deref(), Some("Home"));
    assert!(vanilla.settings.view("r").is_none());
    assert!(vanilla.diagnostics.iter().any(|diagnostic| {
        diagnostic.key == "view b" && diagnostic.message.contains("org-agenda-files-ignored")
    }));
    assert!(reported(&vanilla, "view r"));
}

#[test]
fn neovim_fixture_imports_week_start_warnings_and_views() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../conformance/import/nvim/vanilla");
    let result = import_fixture(&root, "nvim");
    assert_eq!(result.settings.deadline_warning_days, 6);
    assert_eq!(result.settings.week_start, "sunday");
    assert!(result.settings.capture_template("x").is_none());
    assert!(reported(&result, "capture x"));
    assert_eq!(result.settings.view("r").unwrap().blocks.len(), 2);
}
