use std::fs;

use crate::Settings;

use super::file::load_settings;

/// Loads `body` below a `roots` line naming a fresh directory; an
/// error's config path reads CONFIG.
pub(super) fn load_body(body: &str) -> Result<Settings, String> {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("config.toml");
    let root = toml::Value::String(temp.path().to_string_lossy().into_owned());
    fs::write(&config, format!("roots = [{root}]\n{body}")).unwrap();
    load_settings(Some(&config), None)
        .map(|loaded| loaded.settings)
        .map_err(|error| {
            assert_eq!(error.code(), "config");
            error
                .to_string()
                .replace(&config.display().to_string(), "CONFIG")
        })
}

pub(super) fn load_error(body: &str) -> String {
    load_body(body).unwrap_err()
}
