use std::fs;
use std::path::Path;

use agd_core::{AgendaSpan, ImportResult, ResolvedEditorConfig, Settings, apply_import};

/// Applies a case's resolved.json (the editor reader's output, with its
/// files under /fixture) as a first import into a fresh directory.
fn import_fixture(case: &str) -> (tempfile::TempDir, ImportResult) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../conformance/import")
        .join(case);
    let temp = tempfile::tempdir().expect("create import root");
    fs::write(temp.path().join("tasks.org"), "* TODO Fixture\n").expect("write agenda file");
    let raw = fs::read_to_string(path.join("resolved.json")).expect("read resolved fixture");
    let root = temp.path().to_string_lossy();
    let escaped_root = serde_json::to_string(root.as_ref()).expect("encode fixture root");
    let raw = raw.replace("/fixture", &escaped_root[1..escaped_root.len() - 1]);
    let resolved: ResolvedEditorConfig =
        serde_json::from_str(&raw).expect("parse resolved fixture");
    let result = apply_import(&resolved, &Settings::default(), true).expect("apply fixture");
    (temp, result)
}

fn reported(result: &ImportResult, key: &str, text: &str) -> bool {
    result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.key == key && diagnostic.message.contains(text))
}

#[test]
fn emacs_cases_import_agenda_files_relative_to_derived_roots() {
    for case in ["emacs/vanilla", "emacs/use-package", "emacs/doom"] {
        let (temp, result) = import_fixture(case);
        assert_eq!(
            result.settings.roots,
            vec![temp.path().to_string_lossy().into_owned()],
            "{case}"
        );
        assert_eq!(result.settings.agenda_files, vec!["tasks.org"], "{case}");
        assert!(!result.settings.todo_keywords.is_empty(), "{case}");
    }
}

#[test]
fn emacs_vanilla_keeps_what_agd_runs_and_reports_the_rest() {
    let (_temp, result) = import_fixture("emacs/vanilla");
    assert_eq!(
        result.settings.todo_keywords,
        ["TODO(t) NEXT(n!) WAIT(w@) | DONE(d!) CANCELLED(c@)"]
    );
    assert_eq!(result.settings.deadline_warning_days, 5);
    assert_eq!(result.settings.week_start, "sunday");
    assert!(result.settings.capture_template("t").is_some());
    assert!(result.settings.capture_template("x").is_none());
    assert!(reported(&result, "capture x", "computed capture target"));
    // Templates keep the order they are written in, not key order.
    let keys: Vec<&str> = result
        .settings
        .captures
        .iter()
        .map(|template| template.key.as_str())
        .collect();
    assert_eq!(keys, ["t", "a"]);
    assert_eq!(
        result.settings.view("w").unwrap().blocks[0].kind,
        "tags-todo"
    );
    let blocks = &result.settings.view("b").unwrap().blocks;
    assert_eq!(blocks[0].span, Some(AgendaSpan::Day));
    assert_eq!(blocks[1].header.as_deref(), Some("Home"));
    assert!(result.settings.view("r").is_none());
    assert!(reported(&result, "view r", "unsupported"));
    assert!(reported(&result, "org-todo-keywords", "init.el:"));
}

#[test]
fn emacs_super_agenda_groups_import_as_view_groups() {
    let (_temp, result) = import_fixture("emacs/super-agenda");
    let day = &result.settings.view("d").unwrap().blocks;
    // (org-agenda-span 1) is a day, as Org reads it.
    assert_eq!(day[0].span, Some(AgendaSpan::Day));
    let matches: Vec<Option<&str>> = day[0]
        .groups
        .iter()
        .map(|group| group.match_expression.as_deref())
        .collect();
    assert_eq!(
        matches,
        [
            Some("TODO=\"HOLD\""),
            Some("TODO=\"NEXT\""),
            Some("DEADLINE<\"<today>\""),
            Some("DEADLINE>=\"<today>\"+DEADLINE<\"<tomorrow>\""),
            Some("DEADLINE>=\"<tomorrow>\""),
            Some("errand|shop|PRIORITY=\"A\""),
        ]
    );
    assert_eq!(day[0].groups[0].order, 9);
    let backlog = &day[1].groups;
    assert!(backlog[0].discard);
    assert_eq!(backlog[1].match_expression.as_deref(), Some("-TODO<>\"\""));
    assert_eq!(
        backlog[2].match_expression.as_deref(),
        Some("TODO=\"TODO\"+SCHEDULED=\"\"")
    );
    assert_eq!(backlog[4].effort_max.as_deref(), Some("0:15"));
    assert_eq!(
        backlog[5].match_expression.as_deref(),
        Some("TODO=\"NEXT\"")
    );
    assert_eq!(backlog[5].effort_max.as_deref(), Some("0:30"));
    assert_eq!(backlog[6].effort_min.as_deref(), Some("3:00"));
    assert_eq!(backlog[7].by.as_deref(), Some("category"));
    assert!(reported(&result, "view e", "ORed with other selectors"));
    assert!(reported(&result, "view d", "org-super-agenda ignores case"));
    assert!(
        result.settings.view("w").unwrap().blocks[0]
            .groups
            .is_empty()
    );
    assert_eq!(
        result.settings.view("i").unwrap().blocks[0].groups[1]
            .by
            .as_deref(),
        Some("parent")
    );
    // A selector agd cannot run leaves the block ungrouped, not dropped.
    assert!(
        result.settings.view("n").unwrap().blocks[0]
            .groups
            .is_empty()
    );
    assert!(reported(&result, "view n", ":pred is not supported"));
}

#[test]
fn emacs_deferred_settings_win_over_top_level_ones() {
    // use-package/init.el sets org-agenda-files to nil at top level, then
    // to tasks.org in with-eval-after-load 'org.
    let (_temp, result) = import_fixture("emacs/use-package");
    assert_eq!(result.settings.agenda_files, vec!["tasks.org"]);
    assert_eq!(result.settings.log_done, "note");
}

#[test]
fn neovim_setup_call_imports_options_and_property_matches() {
    let (_temp, result) = import_fixture("nvim/vanilla");
    assert_eq!(result.settings.deadline_warning_days, 6);
    assert_eq!(result.settings.week_start, "sunday");
    assert_eq!(result.settings.lowest_priority, "D");
    assert!(result.settings.log_into_drawer);
    assert!(result.settings.capture_template("x").is_none());
    // A Lua table keeps no written order, so nvim templates come by key.
    let keys: Vec<&str> = result
        .settings
        .captures
        .iter()
        .map(|template| template.key.as_str())
        .collect();
    assert_eq!(keys, ["n", "t"]);
    assert_eq!(
        result.settings.view("n").unwrap().blocks[0]
            .match_expression
            .as_deref(),
        Some("TODO=\"NEXT\"")
    );
    assert_eq!(result.settings.view("r").unwrap().blocks.len(), 2);
}

#[test]
fn neovim_lazy_spec_keeps_its_glob_and_default_notes_file() {
    let (_temp, result) = import_fixture("nvim/lazy");
    assert_eq!(result.settings.agenda_files, vec!["**/*.org"]);
    let note = result.settings.capture_template("n").unwrap();
    assert!(note.file.as_deref().unwrap().ends_with("inbox.org"));
    assert_eq!(note.heading.as_deref(), Some("Notes"));
    assert_eq!(result.settings.todo_keywords, vec!["TODO WAIT | DONE"]);
}
