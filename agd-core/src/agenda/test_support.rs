//! Fixtures shared by the agenda modules' tests: an index over one temporary
//! org file.

use crate::WorkspaceIndex;
use crate::config::test_support::settings;

pub(super) fn fixture(source: &str) -> (tempfile::TempDir, WorkspaceIndex) {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("agenda.org"), source).unwrap();
    let index = WorkspaceIndex::load(settings(temp.path())).unwrap();
    (temp, index)
}
