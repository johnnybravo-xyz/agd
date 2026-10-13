use std::path::Path;

use org_core::{IdPolicy, OrgConfig, OrgModeError, load_settings};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliConfig {
    #[serde(default = "default_output_format")]
    pub default_format: String,
}

fn default_output_format() -> String {
    "plain".to_string()
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            default_format: default_output_format(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CliAppConfig {
    pub org: OrgConfig,
    pub cli: CliConfig,
}

impl CliAppConfig {
    pub fn load(
        config_file: Option<String>,
        root_directory: Option<String>,
    ) -> Result<Self, OrgModeError> {
        let loaded = load_settings(
            config_file.as_deref().map(Path::new),
            root_directory.as_deref().map(Path::new),
        )?;
        let settings = loaded.settings;
        let root = settings.files.roots.first().ok_or_else(|| {
            OrgModeError::ConfigError("at least one files.roots entry is required".to_string())
        })?;
        let default_notes_file = settings
            .capture
            .get("t")
            .and_then(|template| template.file.clone())
            .unwrap_or_else(|| "inbox.org".to_string());
        let todo_keywords = settings
            .todo
            .sequences
            .first()
            .cloned()
            .unwrap_or_else(|| OrgConfig::default().org_todo_keywords);
        let org = OrgConfig {
            org_directory: root.clone(),
            org_default_notes_file: default_notes_file,
            org_agenda_files: settings.files.agenda,
            org_agenda_text_search_extra_files: Vec::new(),
            org_todo_keywords: todo_keywords,
            org_auto_created_property: settings.todo.ids == IdPolicy::AddOnWrite,
            org_auto_closed_timestamp: settings.agenda.log_done != "nil",
        };
        Ok(Self {
            org,
            cli: CliConfig::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_read_commands_use_native_file_roots() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("agendum.toml");
        let config = format!(
            "[files]\nroots = [{}]\n",
            toml::Value::String(temp.path().to_string_lossy().into_owned())
        );
        std::fs::write(&config_path, config).unwrap();

        let loaded =
            CliAppConfig::load(Some(config_path.to_string_lossy().into_owned()), None).unwrap();
        assert_eq!(loaded.org.org_directory, temp.path().to_string_lossy());
    }
}
