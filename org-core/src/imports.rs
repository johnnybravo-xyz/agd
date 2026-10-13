use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::Local;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    AgdError, CaptureTemplate, EMACS_IMPORT_SCHEMA, NVIM_IMPORT_SCHEMA, RootSet, Settings,
    ViewBlock, ViewSettings,
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportStatus {
    Exact,
    Difference,
    Unsupported,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImportDiagnostic {
    pub status: ImportStatus,
    pub key: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PortableEntry {
    pub key: String,
    pub description: String,
    pub kind: String,
    pub match_expression: Option<String>,
    pub enabled: bool,
    #[serde(default)]
    pub diagnostics: Vec<String>,
}

/// One custom agenda command as the editor probe reports it. Enabled
/// commands become `[views.KEY]`; `diagnostics` name what was left out.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImportedCommand {
    pub key: String,
    pub description: String,
    pub enabled: bool,
    #[serde(default)]
    pub blocks: Vec<ViewBlock>,
    #[serde(default)]
    pub diagnostics: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedEditorConfig {
    pub editor: String,
    pub editor_version: String,
    pub org_version: String,
    pub source: String,
    pub agenda_files: Vec<String>,
    pub todo_sequences: Vec<Vec<String>>,
    pub tag_alist: Vec<String>,
    pub persistent_tags: Vec<String>,
    pub tag_inheritance_exclusions: Vec<String>,
    pub highest_priority: String,
    pub lowest_priority: String,
    pub default_priority: String,
    pub deadline_warning_days: i64,
    pub agenda_span: String,
    pub agenda_start_day: Option<String>,
    pub week_start: String,
    pub skip_scheduled_if_done: bool,
    pub skip_deadline_if_done: bool,
    pub skip_timestamp_if_done: bool,
    pub sorting_strategy: Vec<String>,
    pub time_grid: Vec<String>,
    pub log_done: String,
    pub log_repeat: String,
    pub log_into_drawer: bool,
    #[serde(default)]
    pub capture: BTreeMap<String, CaptureTemplate>,
    #[serde(default)]
    pub groups: Vec<PortableEntry>,
    #[serde(default)]
    pub commands: Vec<ImportedCommand>,
    #[serde(default)]
    pub diagnostics: Vec<ImportDiagnostic>,
    #[serde(default)]
    pub verification: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SnapshotMetadata {
    pub editor: String,
    pub editor_version: String,
    pub org_version: String,
    pub source: String,
    pub host: String,
    pub extracted_at: String,
    pub digest: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ImportSnapshot {
    pub schema: String,
    pub metadata: SnapshotMetadata,
    pub settings: Settings,
    pub groups: Vec<PortableEntry>,
    pub diagnostics: Vec<ImportDiagnostic>,
}

impl ImportSnapshot {
    pub fn from_resolved(
        resolved: ResolvedEditorConfig,
        approved: &Settings,
        host: impl Into<String>,
    ) -> Result<Self, AgdError> {
        let schema = match resolved.editor.as_str() {
            "emacs" => EMACS_IMPORT_SCHEMA,
            "nvim" => NVIM_IMPORT_SCHEMA,
            value => {
                return Err(AgdError::new(
                    "unsupported_editor",
                    format!("unsupported editor snapshot: {value}"),
                ));
            }
        };
        validate_resolved(&resolved)?;
        let roots = RootSet::new(&approved.files)?;
        let mut diagnostics = resolved.diagnostics.clone();
        let mut agenda_files = Vec::new();
        for value in &resolved.agenda_files {
            let path = PathBuf::from(value);
            match roots.resolve_read(&path) {
                Ok(path) => agenda_files.push(path.to_string_lossy().into_owned()),
                Err(error) => diagnostics.push(ImportDiagnostic {
                    status: ImportStatus::Unsupported,
                    key: value.clone(),
                    message: format!("agenda file disabled by configured roots: {error}"),
                }),
            }
        }
        agenda_files.sort();
        agenda_files.dedup();

        let mut settings = Settings::default();
        settings.files.roots.clear();
        settings.files.agenda = agenda_files;
        settings.todo.sequences = resolved.todo_sequences.clone();
        settings.tags.alist = resolved.tag_alist.clone();
        settings.tags.persistent = resolved.persistent_tags.clone();
        settings.agenda.span = resolved.agenda_span.clone();
        settings.agenda.start_day = resolved.agenda_start_day.clone();
        settings.agenda.week_start = resolved.week_start.clone();
        settings.agenda.deadline_warning_days = resolved.deadline_warning_days;
        settings.agenda.skip_scheduled_if_done = resolved.skip_scheduled_if_done;
        settings.agenda.skip_deadline_if_done = resolved.skip_deadline_if_done;
        settings.agenda.skip_timestamp_if_done = resolved.skip_timestamp_if_done;
        settings.agenda.sorting_strategy = resolved.sorting_strategy.clone();
        settings.agenda.time_grid = resolved.time_grid.clone();
        settings.agenda.highest_priority = resolved.highest_priority.clone();
        settings.agenda.lowest_priority = resolved.lowest_priority.clone();
        settings.agenda.default_priority = resolved.default_priority.clone();
        settings.agenda.tag_inheritance_exclusions = resolved.tag_inheritance_exclusions.clone();
        settings.agenda.log_done = resolved.log_done.clone();
        settings.agenda.log_repeat = resolved.log_repeat.clone();
        settings.agenda.log_into_drawer = resolved.log_into_drawer;
        settings.capture = resolved.capture.clone();
        settings.views.clear();
        for command in &resolved.commands {
            let key = format!("view {}", command.key);
            let unusable_match = command
                .blocks
                .iter()
                .find_map(|block| crate::agenda_engine::validate_view_block(block).err());
            if let Some(error) = &unusable_match {
                diagnostics.push(ImportDiagnostic {
                    status: ImportStatus::Unsupported,
                    key: key.clone(),
                    message: error.to_string(),
                });
            }
            if command.enabled && unusable_match.is_none() {
                settings.views.insert(
                    command.key.clone(),
                    ViewSettings {
                        desc: command.description.clone(),
                        blocks: command.blocks.clone(),
                    },
                );
            }
            let status = if command.enabled {
                ImportStatus::Difference
            } else {
                ImportStatus::Unsupported
            };
            for message in &command.diagnostics {
                diagnostics.push(ImportDiagnostic {
                    status: status.clone(),
                    key: key.clone(),
                    message: message.clone(),
                });
            }
        }

        let digest_input = serde_json::to_vec(&(&settings, &resolved.groups, &diagnostics))
            .map_err(|error| AgdError::with_source("snapshot", "cannot encode snapshot", error))?;
        let digest = format!("{:x}", Sha256::digest(digest_input));
        Ok(Self {
            schema: schema.to_string(),
            metadata: SnapshotMetadata {
                editor: resolved.editor,
                editor_version: resolved.editor_version,
                org_version: resolved.org_version,
                source: resolved.source,
                host: host.into(),
                extracted_at: Local::now().to_rfc3339(),
                digest,
            },
            settings,
            groups: resolved.groups,
            diagnostics,
        })
    }

    pub fn to_toml(&self) -> Result<String, AgdError> {
        toml::to_string_pretty(self).map_err(|error| {
            AgdError::with_source("snapshot", "cannot encode import snapshot", error)
        })
    }

    pub fn write(&self, path: &Path) -> Result<(), AgdError> {
        let contents = self.to_toml()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                AgdError::with_source(
                    "io",
                    format!("cannot create {}: {error}", parent.display()),
                    error,
                )
            })?;
        }
        let temporary = path.with_extension("toml.tmp");
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).map_err(|error| {
            AgdError::with_source(
                "io",
                format!("cannot write {}: {error}", temporary.display()),
                error,
            )
        })?;
        file.write_all(contents.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|error| AgdError::with_source("io", "cannot persist snapshot", error))?;
        fs::rename(&temporary, path).map_err(|error| {
            AgdError::with_source(
                "io",
                format!("cannot replace {}: {error}", path.display()),
                error,
            )
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| {
                AgdError::with_source("io", "cannot secure snapshot permissions", error)
            })?;
        }
        Ok(())
    }
}

fn validate_resolved(resolved: &ResolvedEditorConfig) -> Result<(), AgdError> {
    if resolved.todo_sequences.is_empty()
        || resolved
            .todo_sequences
            .iter()
            .any(|sequence| sequence.is_empty())
    {
        return Err(AgdError::new(
            "unsupported_import_shape",
            "editor returned no usable TODO sequence",
        ));
    }
    for (name, value) in [
        ("highest priority", &resolved.highest_priority),
        ("lowest priority", &resolved.lowest_priority),
        ("default priority", &resolved.default_priority),
    ] {
        if value.chars().count() != 1 {
            return Err(AgdError::new(
                "unsupported_import_shape",
                format!("{name} must be one character"),
            ));
        }
    }
    if resolved.week_start != "monday" && resolved.week_start != "sunday" {
        return Err(AgdError::new(
            "unsupported_import_shape",
            format!("unsupported week start: {}", resolved.week_start),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_never_broadens_approved_roots() {
        let temp = tempfile::tempdir().unwrap();
        let inside = temp.path().join("inside.org");
        fs::write(&inside, "* TODO Inside\n").unwrap();
        let mut approved = Settings::default();
        approved.files.roots = vec![temp.path().to_string_lossy().into_owned()];
        let resolved = ResolvedEditorConfig {
            editor: "emacs".to_string(),
            editor_version: "29.4".to_string(),
            org_version: "9.6".to_string(),
            source: "daemon".to_string(),
            agenda_files: vec![
                inside.to_string_lossy().into_owned(),
                "/unapproved/private.org".to_string(),
            ],
            todo_sequences: vec![vec![
                "TODO".to_string(),
                "|".to_string(),
                "DONE".to_string(),
            ]],
            tag_alist: vec!["work".to_string()],
            persistent_tags: Vec::new(),
            tag_inheritance_exclusions: Vec::new(),
            highest_priority: "A".to_string(),
            lowest_priority: "C".to_string(),
            default_priority: "B".to_string(),
            deadline_warning_days: 14,
            agenda_span: "week".to_string(),
            agenda_start_day: None,
            week_start: "monday".to_string(),
            skip_scheduled_if_done: false,
            skip_deadline_if_done: false,
            skip_timestamp_if_done: false,
            sorting_strategy: vec!["time-up".to_string()],
            time_grid: Vec::new(),
            log_done: "time".to_string(),
            log_repeat: "time".to_string(),
            log_into_drawer: false,
            capture: BTreeMap::new(),
            groups: Vec::new(),
            commands: Vec::new(),
            diagnostics: Vec::new(),
            verification: BTreeMap::new(),
        };
        let snapshot = ImportSnapshot::from_resolved(resolved, &approved, "test").unwrap();
        assert_eq!(snapshot.settings.files.agenda.len(), 1);
        assert_eq!(snapshot.diagnostics.len(), 1);
    }
}
