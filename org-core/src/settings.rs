use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::AgdError;

pub const EMACS_IMPORT_SCHEMA: &str = "agd.import/emacs/v1";
pub const NVIM_IMPORT_SCHEMA: &str = "agd.import/nvim/v1";

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
    pub views: BTreeMap<String, ViewSettings>,
}

/// A saved agenda view, the declarative part of an org-agenda custom
/// command: one block, or several shown one after another like a block
/// agenda.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ViewSettings {
    pub desc: String,
    pub blocks: Vec<ViewBlock>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ViewBlock {
    /// agenda, todo, alltodo, tags or tags-todo, as in org-agenda.
    pub kind: String,
    /// Tags/TODO match; for `todo` blocks, TODO keywords such as `NEXT|WAIT`.
    #[serde(default, rename = "match", skip_serializing_if = "Option::is_none")]
    pub match_expression: Option<String>,
    /// For agenda blocks: day, week, month or a number of days.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
}

/// Agenda sorting keys agd implements; habit-* and category-keep keep
/// collection order.
pub const SORTING_KEYS: [&str; 15] = [
    "time-up",
    "time-down",
    "urgency-up",
    "urgency-down",
    "priority-up",
    "priority-down",
    "category-keep",
    "category-up",
    "category-down",
    "alpha-up",
    "alpha-down",
    "todo-state-up",
    "todo-state-down",
    "habit-up",
    "habit-down",
];

pub const VIEW_BLOCK_KINDS: [&str; 5] = ["agenda", "todo", "alltodo", "tags", "tags-todo"];

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

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
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
    /// org-agenda-sorting-strategy for agenda days, from SORTING_KEYS.
    pub sorting_strategy: Vec<String>,
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
    pub inbox_template: String,
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
            // Org's built-in `n` command: "Agenda and all TODOs".
            views: BTreeMap::from([(
                "n".to_string(),
                ViewSettings {
                    desc: "Agenda and all TODOs".to_string(),
                    blocks: vec![
                        ViewBlock {
                            kind: "agenda".to_string(),
                            match_expression: None,
                            span: None,
                            header: None,
                        },
                        ViewBlock {
                            kind: "alltodo".to_string(),
                            match_expression: None,
                            span: None,
                            header: None,
                        },
                    ],
                },
            )]),
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
            // Emacs's default for agenda days (Org 9.7+ names).
            sorting_strategy: vec![
                "habit-down".to_string(),
                "time-up".to_string(),
                "urgency-down".to_string(),
                "category-keep".to_string(),
            ],
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

impl EditorSettings {
    /// Substitutes `{file}`, `{line}` and `{col}` (both 1-based) inside each
    /// argument. The result is passed straight to the process API, never to a
    /// shell, so file names with spaces or quotes stay one argument.
    pub fn args_for(&self, file: &str, line: usize, col: usize) -> Vec<String> {
        self.args
            .iter()
            .map(|argument| {
                argument
                    .replace("{file}", file)
                    .replace("{line}", &line.to_string())
                    .replace("{col}", &col.to_string())
            })
            .collect()
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
            inbox_template: "t".to_string(),
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

pub fn default_config_path() -> Result<PathBuf, AgdError> {
    let base = dirs::config_dir()
        .ok_or_else(|| config_error("cannot determine the platform configuration directory"))?;
    Ok(base.join("agd").join("config.toml"))
}

pub fn load_settings(
    path: Option<&Path>,
    root_override: Option<&Path>,
) -> Result<LoadedSettings, AgdError> {
    let config_path = path
        .map(Path::to_path_buf)
        .unwrap_or(default_config_path()?);
    let config_dir = config_path
        .parent()
        .ok_or_else(|| {
            config_error(format!(
                "configuration path has no parent: {}",
                config_path.display()
            ))
        })?
        .to_path_buf();

    let manual = if config_path.exists() {
        parse_toml_file(&config_path)?
    } else if path.is_some() {
        return Err(config_error(format!(
            "configuration file does not exist: {}",
            config_path.display()
        )));
    } else {
        toml::Value::Table(toml::map::Map::new())
    };

    let manual_imports: ImportSettings = manual
        .get("imports")
        .cloned()
        .map(toml::Value::try_into)
        .transpose()
        .map_err(|error| config_error(format!("invalid imports configuration: {error}")))?
        .unwrap_or_default();

    let mut merged = toml::Value::try_from(Settings::default())
        .map_err(|error| config_error(format!("cannot build defaults: {error}")))?;
    merge_import_snapshot(
        &mut merged,
        &config_dir,
        &snapshot_or_default(&manual_imports.emacs.snapshot, "emacs.toml"),
        EMACS_IMPORT_SCHEMA,
    )?;
    merge_import_snapshot(
        &mut merged,
        &config_dir,
        &snapshot_or_default(&manual_imports.nvim.snapshot, "nvim.toml"),
        NVIM_IMPORT_SCHEMA,
    )?;
    merge_toml(&mut merged, manual);

    let mut settings: Settings = merged
        .try_into()
        .map_err(|error| config_error(format!("invalid configuration: {error}")))?;
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

fn config_error(message: impl Into<String>) -> AgdError {
    AgdError::new("config", message)
}

/// `agd setup import` writes to imports/<editor>.toml next to the config
/// unless imports.<editor>.snapshot names another file; read the same path.
pub fn snapshot_or_default(configured: &str, default_name: &str) -> String {
    if configured.is_empty() {
        format!("imports/{default_name}")
    } else {
        configured.to_string()
    }
}

fn merge_import_snapshot(
    merged: &mut toml::Value,
    config_dir: &Path,
    snapshot: &str,
    expected_schema: &str,
) -> Result<(), AgdError> {
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
        return Err(config_error(format!(
            "snapshot {} has schema '{schema}', expected '{expected_schema}'",
            path.display()
        )));
    }
    let mut values = imported.get("settings").cloned().ok_or_else(|| {
        config_error(format!(
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

fn parse_toml_file(path: &Path) -> Result<toml::Value, AgdError> {
    let source = fs::read_to_string(path).map_err(|error| {
        AgdError::with_source(
            "io",
            format!("cannot read {}: {error}", path.display()),
            error,
        )
    })?;
    toml::from_str(&source)
        .map_err(|error| config_error(format!("cannot parse {}: {error}", path.display())))
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

fn validate_settings(settings: &Settings) -> Result<(), AgdError> {
    if settings.files.roots.is_empty() {
        return Err(config_error(
            "files.roots must contain at least one approved root",
        ));
    }
    let mut roots = BTreeSet::new();
    for root in &settings.files.roots {
        let expanded = shellexpand::tilde(root).into_owned();
        let canonical = PathBuf::from(&expanded).canonicalize().map_err(|error| {
            config_error(format!("cannot resolve files root '{root}': {error}"))
        })?;
        if !canonical.is_dir() {
            return Err(config_error(format!(
                "files root is not a directory: {root}"
            )));
        }
        if !roots.insert(canonical) {
            return Err(config_error(format!("duplicate files root: {root}")));
        }
    }
    for sequence in &settings.todo.sequences {
        if sequence.iter().filter(|word| word.as_str() == "|").count() != 1 {
            return Err(config_error(
                "every todo.sequences entry must contain exactly one '|' separator",
            ));
        }
    }
    match settings.agents.capture_to.as_str() {
        "template" => {}
        "inbox" => {
            if !settings
                .capture
                .contains_key(&settings.agents.inbox_template)
            {
                return Err(config_error(format!(
                    "agents.capture_to = \"inbox\" needs capture template '{}' (agents.inbox_template)",
                    settings.agents.inbox_template
                )));
            }
        }
        other => {
            return Err(config_error(format!(
                "agents.capture_to must be \"template\" or \"inbox\", not \"{other}\""
            )));
        }
    }
    for key in &settings.agenda.sorting_strategy {
        if !SORTING_KEYS.contains(&key.as_str()) {
            return Err(config_error(format!(
                "agenda.sorting_strategy key '{key}' is not supported; use {}",
                SORTING_KEYS.join(", ")
            )));
        }
    }
    for (key, view) in &settings.views {
        if view.blocks.is_empty() {
            return Err(config_error(format!(
                "view '{key}' needs at least one block"
            )));
        }
        for block in &view.blocks {
            if !VIEW_BLOCK_KINDS.contains(&block.kind.as_str()) {
                return Err(config_error(format!(
                    "view '{key}' has unsupported block kind '{}'; use one of {}",
                    block.kind,
                    VIEW_BLOCK_KINDS.join(", ")
                )));
            }
            crate::agenda_engine::validate_view_block(block)
                .map_err(|error| config_error(format!("view '{key}': {error}")))?;
        }
    }
    for (key, template) in &settings.capture {
        if key.is_empty() || key.chars().any(char::is_whitespace) {
            return Err(config_error(format!(
                "invalid capture template key '{key}'"
            )));
        }
        if template.file.is_none() && template.id.is_none() {
            return Err(config_error(format!(
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
    fn editor_args_substitute_every_placeholder_per_argument() {
        let editor = EditorSettings {
            command: "emacsclient".to_string(),
            args: vec![
                "-n".to_string(),
                "+{line}:{col}".to_string(),
                "{file}".to_string(),
            ],
        };
        assert_eq!(
            editor.args_for("/org/my tasks.org", 42, 1),
            vec!["-n", "+42:1", "/org/my tasks.org"]
        );
    }

    #[test]
    fn default_snapshot_is_merged_below_manual_values_without_roots() {
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
                "[files]\nroots = [{}]\n[agenda]\ndeadline_warning_days = 9\n",
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
    fn explicit_missing_config_file_is_an_error() {
        let temp = tempfile::tempdir().unwrap();
        let error = load_settings(Some(&temp.path().join("missing.toml")), None).unwrap_err();
        assert_eq!(error.code(), "config");
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
