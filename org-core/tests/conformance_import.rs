use std::fs;
use std::path::Path;

use org_core::{
    EMACS_IMPORT_SCHEMA, ImportSnapshot, NVIM_IMPORT_SCHEMA, ResolvedEditorConfig, Settings,
};

fn normalize_fixture(path: &Path, editor: &str) -> ImportSnapshot {
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
    let mut approved = Settings::default();
    approved.files.roots = vec![temp.path().to_string_lossy().into_owned()];
    let snapshot = ImportSnapshot::from_resolved(resolved, &approved, "fixture-host")
        .expect("normalize resolved fixture");
    assert_eq!(snapshot.metadata.editor, editor);
    let encoded = snapshot.to_toml().expect("encode snapshot");
    let decoded: ImportSnapshot = toml::from_str(&encoded).expect("decode snapshot");
    assert_eq!(decoded.metadata.digest, snapshot.metadata.digest);
    snapshot
}

#[test]
fn emacs_fixtures_preserve_resolved_ordering_and_disabled_entries() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../conformance/import/emacs");
    for name in ["vanilla", "use-package", "doom"] {
        let snapshot = normalize_fixture(&root.join(name), "emacs");
        assert_eq!(snapshot.schema, EMACS_IMPORT_SCHEMA);
        assert!(!snapshot.settings.todo.sequences.is_empty());
        assert_eq!(snapshot.settings.files.agenda.len(), 1);
    }
    let vanilla = normalize_fixture(&root.join("vanilla"), "emacs");
    assert!(!vanilla.settings.capture["x"].enabled);
    assert_eq!(vanilla.settings.views["w"].blocks[0].kind, "tags-todo");
    let block = &vanilla.settings.views["b"].blocks;
    assert_eq!(block[0].span.as_deref(), Some("day"));
    assert_eq!(block[1].header.as_deref(), Some("Home"));
    assert!(!vanilla.settings.views.contains_key("r"));
    assert!(vanilla.diagnostics.iter().any(|diagnostic| {
        diagnostic.key == "view b" && diagnostic.message.contains("org-agenda-files-ignored")
    }));
    assert!(
        vanilla
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.key == "view r")
    );
}

#[test]
fn neovim_fixture_accepts_installed_plugin_version() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../conformance/import/nvim/vanilla");
    let snapshot = normalize_fixture(&root, "nvim");
    assert_eq!(snapshot.schema, NVIM_IMPORT_SCHEMA);
    assert_eq!(snapshot.metadata.org_version, "999.0.0");
    assert_eq!(snapshot.settings.agenda.deadline_warning_days, 6);
    assert_eq!(snapshot.settings.agenda.week_start, "sunday");
    assert!(!snapshot.settings.capture["x"].enabled);
    assert_eq!(snapshot.settings.views["r"].blocks.len(), 2);
}
