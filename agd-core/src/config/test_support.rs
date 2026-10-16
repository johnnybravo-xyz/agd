//! Helpers shared by the crate's tests: settings over a temporary root, and
//! loading a config body from a temporary file.

use std::fs;
use std::path::Path;

use crate::Settings;

use super::file::load_settings;

/// Settings whose one root is `root`, every `.org` file under it an agenda
/// file.
pub(crate) fn settings(root: &Path) -> Settings {
    Settings {
        roots: vec![root.to_string_lossy().into_owned()],
        agenda_files: vec!["**/*.org".to_string()],
        ..Settings::default()
    }
}

/// Loads `body` below a `roots` line naming a fresh directory; an
/// error's config path reads CONFIG.
pub(super) fn load_body(body: &str) -> Result<Settings, String> {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("config.toml");
    let root = toml::Value::String(temp.path().to_string_lossy().into_owned());
    fs::write(&config, format!("roots = [{root}]\n{body}")).unwrap();
    load_settings(Some(&config), None).map_err(|error| {
        assert_eq!(error.code().as_str(), "invalid_config");
        error
            .to_string()
            .replace(&config.display().to_string(), "CONFIG")
    })
}

pub(super) fn load_error(body: &str) -> String {
    load_body(body).unwrap_err()
}
