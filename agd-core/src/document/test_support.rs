//! Helpers shared by the document modules' tests.

use std::path::Path;

use crate::Settings;

pub(super) fn settings(root: &Path) -> Settings {
    Settings {
        roots: vec![root.to_string_lossy().into_owned()],
        ..Settings::default()
    }
}
