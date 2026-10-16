//! Fixtures shared by the agenda modules' tests: an index over one temporary
//! org file.

use crate::{Settings, WorkspaceIndex};

pub(super) fn fixture(source: &str) -> (tempfile::TempDir, WorkspaceIndex) {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("agenda.org"), source).unwrap();
    let settings = Settings {
        roots: vec![temp.path().to_string_lossy().into_owned()],
        agenda_files: vec!["**/*.org".to_string()],
        ..Settings::default()
    };
    let index = WorkspaceIndex::load(settings).unwrap();
    (temp, index)
}
