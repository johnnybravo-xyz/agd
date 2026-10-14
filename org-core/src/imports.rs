use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{AgdError, CaptureTemplate, RootSet, SORTING_KEYS, Settings, ViewBlock, ViewSettings};

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

/// One capture template as the editor reports it; disabled templates are
/// reported, not imported.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImportedCapture {
    pub desc: String,
    pub file: Option<String>,
    pub heading: Option<String>,
    pub outline_path: Option<Vec<String>>,
    pub id: Option<String>,
    #[serde(default)]
    pub datetree: bool,
    pub template: String,
    pub template_file: Option<String>,
    pub enabled: bool,
    #[serde(default)]
    pub diagnostics: Vec<String>,
}

/// One custom agenda command as the editor reports it. Enabled commands
/// become views; `diagnostics` name what was left out.
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
    pub tags_column: i64,
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
    pub log_done: String,
    pub log_repeat: String,
    pub log_reschedule: String,
    pub log_redeadline: String,
    pub log_into_drawer: bool,
    #[serde(default)]
    pub capture: BTreeMap<String, ImportedCapture>,
    #[serde(default)]
    pub groups: Vec<PortableEntry>,
    #[serde(default)]
    pub commands: Vec<ImportedCommand>,
    #[serde(default)]
    pub diagnostics: Vec<ImportDiagnostic>,
    #[serde(default)]
    pub verification: BTreeMap<String, String>,
}

/// The editor's settings applied over `base`, and what could not be.
#[derive(Clone, Debug)]
pub struct ImportResult {
    pub settings: Settings,
    pub diagnostics: Vec<ImportDiagnostic>,
}

/// Applies resolved editor settings over `base` (the current config). The
/// editor's values replace the keys it reports; agd-only keys (roots,
/// editor, theme, agents, triage) keep their `base` values. Agenda files
/// outside the roots are left out with a diagnostic.
pub fn apply_import(
    resolved: &ResolvedEditorConfig,
    base: &Settings,
) -> Result<ImportResult, AgdError> {
    validate_resolved(resolved)?;
    let roots = RootSet::new(&base.roots, &[])?;
    let mut diagnostics = resolved.diagnostics.clone();
    let mut agenda_files = Vec::new();
    for value in &resolved.agenda_files {
        match roots.resolve_read(&PathBuf::from(value)) {
            Ok(path) => agenda_files.push(path.to_string_lossy().into_owned()),
            Err(error) => diagnostics.push(ImportDiagnostic {
                status: ImportStatus::Unsupported,
                key: value.clone(),
                message: format!("agenda file left out, outside roots: {error}"),
            }),
        }
    }
    agenda_files.sort();
    agenda_files.dedup();

    let mut settings = base.clone();
    settings.agenda_files = agenda_files;
    settings.todo_keywords = resolved.todo_sequences.clone();
    settings.tag_alist = resolved.tag_alist.clone();
    settings.persistent_tags = resolved.persistent_tags.clone();
    settings.tags_column = resolved.tags_column;
    settings.agenda_span = resolved.agenda_span.clone();
    settings.agenda_start_day = resolved.agenda_start_day.clone();
    settings.week_start = resolved.week_start.clone();
    settings.deadline_warning_days = resolved.deadline_warning_days;
    settings.skip_scheduled_if_done = resolved.skip_scheduled_if_done;
    settings.skip_deadline_if_done = resolved.skip_deadline_if_done;
    settings.skip_timestamp_if_done = resolved.skip_timestamp_if_done;
    settings.sorting_strategy.clear();
    for key in &resolved.sorting_strategy {
        if SORTING_KEYS.contains(&key.as_str()) {
            settings.sorting_strategy.push(key.clone());
        } else {
            diagnostics.push(ImportDiagnostic {
                status: ImportStatus::Difference,
                key: "sorting_strategy".to_string(),
                message: format!("sorting key {key} is not supported and was left out"),
            });
        }
    }
    settings.highest_priority = resolved.highest_priority.clone();
    settings.lowest_priority = resolved.lowest_priority.clone();
    settings.default_priority = resolved.default_priority.clone();
    settings.tag_inheritance_exclusions = resolved.tag_inheritance_exclusions.clone();
    settings.log_done = resolved.log_done.clone();
    settings.log_repeat = resolved.log_repeat.clone();
    settings.log_reschedule = resolved.log_reschedule.clone();
    settings.log_redeadline = resolved.log_redeadline.clone();
    settings.log_into_drawer = resolved.log_into_drawer;

    settings.captures.clear();
    for (key, capture) in &resolved.capture {
        if !capture.enabled {
            diagnostics.push(ImportDiagnostic {
                status: ImportStatus::Unsupported,
                key: format!("capture {key}"),
                message: capture.diagnostics.join("; "),
            });
            continue;
        }
        settings.captures.push(CaptureTemplate {
            key: key.clone(),
            desc: capture.desc.clone(),
            file: capture.file.clone(),
            heading: capture.heading.clone(),
            outline_path: capture.outline_path.clone(),
            id: capture.id.clone(),
            datetree: capture.datetree,
            template: capture.template.clone(),
            template_file: capture.template_file.clone(),
        });
    }
    if settings.captures.is_empty() {
        settings.captures = base.captures.clone();
    }

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
            settings.views.push(ViewSettings {
                key: command.key.clone(),
                desc: command.description.clone(),
                blocks: command.blocks.clone(),
            });
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
    if settings.views.is_empty() {
        settings.views = base.views.clone();
    }

    for group in &resolved.groups {
        diagnostics.push(ImportDiagnostic {
            status: ImportStatus::Unsupported,
            key: format!("org-super-agenda {}", group.key),
            message: "agd does not group agenda rows; the group is not imported".to_string(),
        });
    }
    Ok(ImportResult {
        settings,
        diagnostics,
    })
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
    use std::fs;

    #[test]
    fn import_never_broadens_roots_and_keeps_agd_only_keys() {
        let temp = tempfile::tempdir().unwrap();
        let inside = temp.path().join("inside.org");
        fs::write(&inside, "* TODO Inside\n").unwrap();
        let base = Settings {
            roots: vec![temp.path().to_string_lossy().into_owned()],
            theme: "dark".to_string(),
            ..Settings::default()
        };
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
            tags_column: -77,
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
            log_done: "time".to_string(),
            log_repeat: "time".to_string(),
            log_reschedule: "nil".to_string(),
            log_redeadline: "nil".to_string(),
            log_into_drawer: false,
            capture: BTreeMap::new(),
            groups: Vec::new(),
            commands: Vec::new(),
            diagnostics: Vec::new(),
            verification: BTreeMap::new(),
        };
        let result = apply_import(&resolved, &base).unwrap();
        assert_eq!(result.settings.agenda_files.len(), 1);
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.settings.roots, base.roots);
        assert_eq!(result.settings.theme, "dark");
        assert_eq!(result.settings.captures, base.captures);
    }
}
