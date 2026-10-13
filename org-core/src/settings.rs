use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::OrgModeError;

pub const EMACS_IMPORT_SCHEMA: &str = "agendum.import/emacs/v1";
pub const NVIM_IMPORT_SCHEMA: &str = "agendum.import/nvim/v1";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub files: FilesSettings,
    pub imports: ImportSettings,
    pub todo: TodoSettings,
    pub tags: TagsSettings,
    pub agenda: AgendaSettings,
    pub capture: BTreeMap<String, CaptureTemplate>,
    pub editor: EditorSettings,
    pub agents: AgentSettings,
    pub triage: TriageSettings,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct FilesSettings {
    pub roots: Vec<String>,
    pub agenda: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ImportSettings {
    pub emacs: EditorImportSettings,
    pub nvim: EditorImportSettings,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct EditorImportSettings {
    pub snapshot: String,
    pub prefer_daemon: bool,
    pub server: Option<String>,
    pub batch_profile: Option<String>,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub batch_profiles: BTreeMap<String, CommandProfile>,
    pub profiles: BTreeMap<String, CommandProfile>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommandProfile {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct TodoSettings {
    pub sequences: Vec<Vec<String>>,
    pub ids: IdPolicy,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct TagsSettings {
    pub alist: Vec<String>,
    pub persistent: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum IdPolicy {
    #[default]
    Preserve,
    AddOnWrite,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct AgendaSettings {
    pub span: String,
    pub start_day: Option<String>,
    pub week_start: String,
    pub deadline_warning_days: i64,
    pub skip_scheduled_if_done: bool,
    pub skip_deadline_if_done: bool,
    pub skip_timestamp_if_done: bool,
    pub sorting_strategy: Vec<String>,
    pub time_grid: Vec<String>,
    pub highest_priority: String,
    pub lowest_priority: String,
    pub default_priority: String,
    pub tag_inheritance_exclusions: Vec<String>,
    pub log_done: String,
    pub log_repeat: String,
    pub log_into_drawer: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureTemplate {
    pub desc: String,
    pub file: Option<String>,
    pub heading: Option<String>,
    pub outline_path: Option<Vec<String>>,
    pub id: Option<String>,
    #[serde(default)]
    pub datetree: bool,
    pub template: String,
    pub template_file: Option<String>,
    #[serde(default = "enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub diagnostics: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct EditorSettings {
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct AgentSettings {
    pub capture_to: String,
    pub profiles: BTreeMap<String, AgentProfile>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct AgentProfile {
    pub files: Vec<String>,
    #[serde(rename = "match")]
    pub match_expression: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct TriageSettings {
    pub someday_tag: String,
    pub reschedule_nudge: usize,
}

#[derive(Clone, Debug)]
pub struct LoadedSettings {
    pub settings: Settings,
    pub config_path: PathBuf,
    pub config_dir: PathBuf,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            files: FilesSettings::default(),
            imports: ImportSettings::default(),
            todo: TodoSettings::default(),
            tags: TagsSettings::default(),
            agenda: AgendaSettings::default(),
            capture: BTreeMap::from([("t".to_string(), CaptureTemplate::default())]),
            editor: EditorSettings::default(),
            agents: AgentSettings::default(),
            triage: TriageSettings::default(),
        }
    }
}

impl Default for FilesSettings {
    fn default() -> Self {
        Self {
            roots: vec!["~/org".to_string()],
            agenda: Vec::new(),
        }
    }
}

impl Default for EditorImportSettings {
    fn default() -> Self {
        Self {
            snapshot: String::new(),
            prefer_daemon: false,
            server: None,
            batch_profile: None,
            command: None,
            args: Vec::new(),
            batch_profiles: BTreeMap::new(),
            profiles: BTreeMap::new(),
        }
    }
}

impl Default for TodoSettings {
    fn default() -> Self {
        Self {
            sequences: vec![vec![
                "TODO".to_string(),
                "NEXT".to_string(),
                "WAIT".to_string(),
                "|".to_string(),
                "DONE".to_string(),
                "CANCELLED".to_string(),
            ]],
            ids: IdPolicy::Preserve,
        }
    }
}

impl Default for AgendaSettings {
    fn default() -> Self {
        Self {
            span: "week".to_string(),
            start_day: None,
            week_start: "monday".to_string(),
            deadline_warning_days: 14,
            skip_scheduled_if_done: false,
            skip_deadline_if_done: false,
            skip_timestamp_if_done: false,
            sorting_strategy: vec![
                "time-up".to_string(),
                "priority-down".to_string(),
                "category-keep".to_string(),
            ],
            time_grid: Vec::new(),
            highest_priority: "A".to_string(),
            lowest_priority: "C".to_string(),
            default_priority: "B".to_string(),
            tag_inheritance_exclusions: Vec::new(),
            log_done: "time".to_string(),
            log_repeat: "time".to_string(),
            log_into_drawer: false,
        }
    }
}

impl Default for CaptureTemplate {
    fn default() -> Self {
        Self {
            desc: "Todo".to_string(),
            file: Some("inbox.org".to_string()),
            heading: Some("Inbox".to_string()),
            outline_path: None,
            id: None,
            datetree: false,
            template: "* TODO %?\n:PROPERTIES:\n:CREATED: %U\n:END:".to_string(),
            template_file: None,
            enabled: true,
            diagnostics: Vec::new(),
        }
    }
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            command: std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string()),
            args: vec!["+{line}".to_string(), "{file}".to_string()],
        }
    }
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            capture_to: "template".to_string(),
            profiles: BTreeMap::new(),
        }
    }
}

impl Default for TriageSettings {
    fn default() -> Self {
        Self {
            someday_tag: "someday".to_string(),
            reschedule_nudge: 3,
        }
    }
}

pub fn default_config_path() -> Result<PathBuf, OrgModeError> {
    let base = dirs::config_dir().ok_or_else(|| {
        OrgModeError::ConfigError("cannot determine the platform configuration directory".into())
    })?;
    Ok(base.join("agendum").join("config.toml"))
}

pub fn load_settings(
    path: Option<&Path>,
    root_override: Option<&Path>,
) -> Result<LoadedSettings, OrgModeError> {
    let config_path = path
        .map(Path::to_path_buf)
        .unwrap_or(default_config_path()?);
    let config_dir = config_path
        .parent()
        .ok_or_else(|| {
            OrgModeError::ConfigError(format!(
                "configuration path has no parent: {}",
                config_path.display()
            ))
        })?
        .to_path_buf();

    let manual = if config_path.exists() {
        parse_toml_file(&config_path)?
    } else {
        toml::Value::Table(toml::map::Map::new())
    };

    let manual_imports: ImportSettings = manual
        .get("imports")
        .cloned()
        .map(toml::Value::try_into)
        .transpose()
        .map_err(|error| {
            OrgModeError::ConfigError(format!("invalid imports configuration: {error}"))
        })?
        .unwrap_or_default();

    let mut merged = toml::Value::try_from(Settings::default())
        .map_err(|error| OrgModeError::ConfigError(format!("cannot build defaults: {error}")))?;
    merge_import_snapshot(
        &mut merged,
        &config_dir,
        &manual_imports.emacs.snapshot,
        EMACS_IMPORT_SCHEMA,
    )?;
    merge_import_snapshot(
        &mut merged,
        &config_dir,
        &manual_imports.nvim.snapshot,
        NVIM_IMPORT_SCHEMA,
    )?;
    merge_toml(&mut merged, manual);

    let mut settings: Settings = merged
        .try_into()
        .map_err(|error| OrgModeError::ConfigError(format!("invalid configuration: {error}")))?;
    if let Some(root) = root_override {
        settings.files.roots = vec![root.to_string_lossy().into_owned()];
    }
    validate_settings(&settings)?;

    Ok(LoadedSettings {
        settings,
        config_path,
        config_dir,
    })
}

fn merge_import_snapshot(
    merged: &mut toml::Value,
    config_dir: &Path,
    snapshot: &str,
    expected_schema: &str,
) -> Result<(), OrgModeError> {
    if snapshot.is_empty() {
        return Ok(());
    }
    let path = resolve_config_path(config_dir, snapshot);
    if !path.exists() {
        return Ok(());
    }
    let imported = parse_toml_file(&path)?;
    let schema = imported
        .get("schema")
        .and_then(toml::Value::as_str)
        .unwrap_or_default();
    if schema != expected_schema {
        return Err(OrgModeError::ConfigError(format!(
            "snapshot {} has schema '{schema}', expected '{expected_schema}'",
            path.display()
        )));
    }
    let mut values = imported.get("settings").cloned().ok_or_else(|| {
        OrgModeError::ConfigError(format!(
            "snapshot {} has no [settings] payload",
            path.display()
        ))
    })?;
    if let Some(table) = values.as_table_mut() {
        if let Some(files) = table.get_mut("files").and_then(toml::Value::as_table_mut) {
            files.remove("roots");
        }
        table.remove("imports");
        table.remove("editor");
    }
    merge_toml(merged, values);
    Ok(())
}

fn parse_toml_file(path: &Path) -> Result<toml::Value, OrgModeError> {
    let source = fs::read_to_string(path).map_err(OrgModeError::IoError)?;
    toml::from_str(&source).map_err(|error| {
        OrgModeError::ConfigError(format!("cannot parse {}: {error}", path.display()))
    })
}

pub fn resolve_config_path(config_dir: &Path, value: &str) -> PathBuf {
    let expanded = shellexpand::tilde(value).into_owned();
    let path = PathBuf::from(expanded);
    if path.is_absolute() {
        path
    } else {
        config_dir.join(path)
    }
}

fn merge_toml(base: &mut toml::Value, overlay: toml::Value) {
    match (base, overlay) {
        (toml::Value::Table(base), toml::Value::Table(overlay)) => {
            for (key, value) in overlay {
                match base.get_mut(&key) {
                    Some(existing) => merge_toml(existing, value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, overlay) => *base = overlay,
    }
}

fn validate_settings(settings: &Settings) -> Result<(), OrgModeError> {
    if settings.files.roots.is_empty() {
        return Err(OrgModeError::ConfigError(
            "files.roots must contain at least one approved root".into(),
        ));
    }
    let mut roots = BTreeSet::new();
    for root in &settings.files.roots {
        let expanded = shellexpand::tilde(root).into_owned();
        let canonical = PathBuf::from(&expanded).canonicalize().map_err(|error| {
            OrgModeError::ConfigError(format!("cannot resolve files root '{root}': {error}"))
        })?;
        if !canonical.is_dir() {
            return Err(OrgModeError::ConfigError(format!(
                "files root is not a directory: {root}"
            )));
        }
        if !roots.insert(canonical) {
            return Err(OrgModeError::ConfigError(format!(
                "duplicate files root: {root}"
            )));
        }
    }
    for sequence in &settings.todo.sequences {
        if sequence.iter().filter(|word| word.as_str() == "|").count() != 1 {
            return Err(OrgModeError::ConfigError(
                "every todo.sequences entry must contain exactly one '|' separator".into(),
            ));
        }
    }
    for (key, template) in &settings.capture {
        if key.is_empty() || key.chars().any(char::is_whitespace) {
            return Err(OrgModeError::ConfigError(format!(
                "invalid capture template key '{key}'"
            )));
        }
        if template.file.is_none() && template.id.is_none() {
            return Err(OrgModeError::ConfigError(format!(
                "capture template '{key}' needs a file or ID target"
            )));
        }
    }
    Ok(())
}

fn enabled() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_values_override_import_without_broadening_roots() {
        let temp = tempfile::tempdir().unwrap();
        let approved = temp.path().join("approved");
        fs::create_dir(&approved).unwrap();
        let imports = temp.path().join("imports");
        fs::create_dir(&imports).unwrap();
        fs::write(
            imports.join("emacs.toml"),
            format!(
                "schema = \"{EMACS_IMPORT_SCHEMA}\"\n[settings.files]\nroots = [\"/unapproved\"]\nagenda = [\"imported.org\"]\n[settings.agenda]\ndeadline_warning_days = 5\n"
            ),
        )
        .unwrap();
        let config = temp.path().join("config.toml");
        fs::write(
            &config,
            format!(
                "[files]\nroots = [{}]\n[agenda]\ndeadline_warning_days = 9\n[imports.emacs]\nsnapshot = \"imports/emacs.toml\"\n",
                toml::Value::String(approved.to_string_lossy().into_owned())
            ),
        )
        .unwrap();

        let loaded = load_settings(Some(&config), None).unwrap();
        assert_eq!(
            loaded.settings.files.roots,
            vec![approved.to_string_lossy()]
        );
        assert_eq!(loaded.settings.files.agenda, vec!["imported.org"]);
        assert_eq!(loaded.settings.agenda.deadline_warning_days, 9);
    }

    #[test]
    fn cli_root_override_has_highest_precedence() {
        let temp = tempfile::tempdir().unwrap();
        let configured = temp.path().join("configured");
        let overridden = temp.path().join("overridden");
        fs::create_dir(&configured).unwrap();
        fs::create_dir(&overridden).unwrap();
        let config = temp.path().join("config.toml");
        fs::write(
            &config,
            format!(
                "[files]\nroots = [{}]\n",
                toml::Value::String(configured.to_string_lossy().into_owned())
            ),
        )
        .unwrap();

        let loaded = load_settings(Some(&config), Some(&overridden)).unwrap();
        assert_eq!(
            loaded.settings.files.roots,
            vec![overridden.to_string_lossy()]
        );
    }
}
