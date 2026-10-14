use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::AgdError;

/// Everything agd reads from `config.toml`, one flat table: every setting is
/// a top-level key, and only capture templates, views and agent profiles are
/// lists of records (`[[capture]]`, `[[view]]`, `[[agent_profile]]`).
/// Missing keys take the defaults below; unknown keys are errors.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// The only directories agd reads or writes.
    pub roots: Vec<String>,
    /// org-agenda-files: files, directories (their `*.org` files) or globs.
    /// Empty means every `.org` file under the roots.
    pub agenda_files: Vec<String>,

    /// org-todo-keywords: one list per sequence, with exactly one "|".
    pub todo_keywords: Vec<Vec<String>>,
    /// "preserve" leaves headings alone; "add-on-write" adds an org-id :ID:
    /// on a heading's first edit.
    pub ids: IdPolicy,
    pub tag_alist: Vec<String>,
    pub persistent_tags: Vec<String>,
    /// org-tags-column: negative right-aligns tags to end at that column,
    /// positive starts them there, as Emacs does when it rewrites a headline.
    pub tags_column: i64,
    pub tag_inheritance_exclusions: Vec<String>,
    pub highest_priority: String,
    pub lowest_priority: String,
    pub default_priority: String,

    /// day, week, month or a number of days.
    pub agenda_span: String,
    pub agenda_start_day: Option<String>,
    /// monday or sunday.
    pub week_start: String,
    pub deadline_warning_days: i64,
    pub skip_scheduled_if_done: bool,
    pub skip_deadline_if_done: bool,
    pub skip_timestamp_if_done: bool,
    /// org-agenda-sorting-strategy for agenda days, from SORTING_KEYS.
    pub sorting_strategy: Vec<String>,

    /// org-log-done, org-log-repeat, org-log-reschedule, org-log-redeadline:
    /// nil, time or note. `#+STARTUP` options win per file.
    pub log_done: String,
    pub log_repeat: String,
    pub log_reschedule: String,
    pub log_redeadline: String,
    pub log_into_drawer: bool,

    /// Program and arguments to open a file; `{file}`, `{line}` and `{col}`
    /// are substituted. Run directly, never through a shell.
    pub editor: Vec<String>,
    /// auto (follow the terminal background), dark or light.
    pub theme: String,
    /// "template" captures through the agent's chosen template; "inbox" sends
    /// every agent capture through `agent_inbox_template` without dates.
    pub agent_capture_to: String,
    pub agent_inbox_template: String,
    pub someday_tag: String,
    /// Someday also removes SCHEDULED, so the task leaves the agenda.
    pub someday_clears_scheduled: bool,
    /// Triage suggests Someday after this many reschedules.
    pub reschedule_nudge: usize,

    #[serde(rename = "capture")]
    pub captures: Vec<CaptureTemplate>,
    #[serde(rename = "view")]
    pub views: Vec<ViewSettings>,
    #[serde(rename = "agent_profile")]
    pub agent_profiles: Vec<AgentProfile>,
}

/// A saved agenda view, the declarative part of an org-agenda custom
/// command: one block, or several shown one after another like a block
/// agenda.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ViewSettings {
    pub key: String,
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureTemplate {
    pub key: String,
    pub desc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline_path: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub datetree: bool,
    pub template: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_file: Option<String>,
}

/// A named, filtered agenda an agent may read at session start.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProfile {
    pub name: String,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default, rename = "match", skip_serializing_if = "Option::is_none")]
    pub match_expression: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum IdPolicy {
    #[default]
    Preserve,
    AddOnWrite,
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

#[derive(Clone, Debug)]
pub struct LoadedSettings {
    pub settings: Settings,
    pub config_path: PathBuf,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            roots: vec!["~/org".to_string()],
            agenda_files: Vec::new(),
            todo_keywords: vec![strings(&["TODO", "NEXT", "WAIT", "|", "DONE", "CANCELLED"])],
            ids: IdPolicy::Preserve,
            tag_alist: Vec::new(),
            persistent_tags: Vec::new(),
            tags_column: -77,
            tag_inheritance_exclusions: Vec::new(),
            highest_priority: "A".to_string(),
            lowest_priority: "C".to_string(),
            default_priority: "B".to_string(),
            agenda_span: "week".to_string(),
            agenda_start_day: None,
            week_start: "monday".to_string(),
            deadline_warning_days: 14,
            skip_scheduled_if_done: false,
            skip_deadline_if_done: false,
            skip_timestamp_if_done: false,
            // Emacs's default for agenda days (Org 9.7+ names).
            sorting_strategy: strings(&["habit-down", "time-up", "urgency-down", "category-keep"]),
            log_done: "time".to_string(),
            log_repeat: "time".to_string(),
            log_reschedule: "nil".to_string(),
            log_redeadline: "nil".to_string(),
            log_into_drawer: false,
            editor: vec![
                std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string()),
                "+{line}".to_string(),
                "{file}".to_string(),
            ],
            theme: "auto".to_string(),
            agent_capture_to: "template".to_string(),
            agent_inbox_template: "t".to_string(),
            someday_tag: "someday".to_string(),
            someday_clears_scheduled: true,
            reschedule_nudge: 3,
            captures: vec![CaptureTemplate::default()],
            // Org's built-in `n` command: "Agenda and all TODOs".
            views: vec![ViewSettings {
                key: "n".to_string(),
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
            }],
            agent_profiles: Vec::new(),
        }
    }
}

/// The default `t` template: a TODO in inbox.org under Inbox, stamped with
/// its creation time.
impl Default for CaptureTemplate {
    fn default() -> Self {
        Self {
            key: "t".to_string(),
            desc: "Todo".to_string(),
            file: Some("inbox.org".to_string()),
            heading: Some("Inbox".to_string()),
            outline_path: None,
            id: None,
            datetree: false,
            template: "* TODO %?\n:PROPERTIES:\n:CREATED: %U\n:END:".to_string(),
            template_file: None,
        }
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

impl Settings {
    pub fn capture_template(&self, key: &str) -> Option<&CaptureTemplate> {
        self.captures.iter().find(|template| template.key == key)
    }

    pub fn view(&self, key: &str) -> Option<&ViewSettings> {
        self.views.iter().find(|view| view.key == key)
    }

    pub fn agent_profile(&self, name: &str) -> Option<&AgentProfile> {
        self.agent_profiles
            .iter()
            .find(|profile| profile.name == name)
    }

    /// The editor program and its arguments with `{file}`, `{line}` and
    /// `{col}` (both 1-based) substituted inside each argument. The result
    /// goes straight to the process API, never to a shell, so file names
    /// with spaces or quotes stay one argument.
    pub fn editor_command(&self, file: &str, line: usize, col: usize) -> (String, Vec<String>) {
        let mut parts = self.editor.iter().map(|argument| {
            argument
                .replace("{file}", file)
                .replace("{line}", &line.to_string())
                .replace("{col}", &col.to_string())
        });
        let program = parts.next().unwrap_or_else(|| "vi".to_string());
        (program, parts.collect())
    }
}

pub fn default_config_path() -> Result<PathBuf, AgdError> {
    Ok(crate::paths::config_dir()?.join("config.toml"))
}

pub fn load_settings(
    path: Option<&Path>,
    root_override: Option<&Path>,
) -> Result<LoadedSettings, AgdError> {
    let config_path = path
        .map(Path::to_path_buf)
        .unwrap_or(default_config_path()?);
    let mut settings = if config_path.exists() {
        parse_config_file(&config_path)?
    } else if path.is_some() {
        return Err(config_error(format!(
            "configuration file does not exist: {}",
            config_path.display()
        )));
    } else {
        Settings::default()
    };
    if let Some(root) = root_override {
        settings.roots = vec![root.to_string_lossy().into_owned()];
    }
    validate_settings(&settings)?;
    Ok(LoadedSettings {
        settings,
        config_path,
    })
}

/// Parses a config file without validating roots, so `agd setup import` can
/// read one whose roots do not exist yet.
pub fn parse_config_file(path: &Path) -> Result<Settings, AgdError> {
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

fn config_error(message: impl Into<String>) -> AgdError {
    AgdError::new("config", message)
}

/// Top-level keys in the order `render_config` writes them, under these
/// comments.
const CONFIG_GROUPS: [(&str, &[&str]); 6] = [
    ("Files agd may read and write", &["roots", "agenda_files"]),
    (
        "Org syntax, as in your editor",
        &[
            "todo_keywords",
            "ids",
            "tag_alist",
            "persistent_tags",
            "tags_column",
            "tag_inheritance_exclusions",
            "highest_priority",
            "lowest_priority",
            "default_priority",
        ],
    ),
    (
        "Agenda",
        &[
            "agenda_span",
            "agenda_start_day",
            "week_start",
            "deadline_warning_days",
            "skip_scheduled_if_done",
            "skip_deadline_if_done",
            "skip_timestamp_if_done",
            "sorting_strategy",
        ],
    ),
    (
        "Logging; #+STARTUP options in a file win",
        &[
            "log_done",
            "log_repeat",
            "log_reschedule",
            "log_redeadline",
            "log_into_drawer",
        ],
    ),
    ("Editor and terminal", &["editor", "theme"]),
    (
        "Agents and triage",
        &[
            "agent_capture_to",
            "agent_inbox_template",
            "someday_tag",
            "someday_clears_scheduled",
            "reschedule_nudge",
        ],
    ),
];

/// Writes `settings` as a config file: grouped, commented top-level keys,
/// then `[[capture]]`, `[[view]]` and `[[agent_profile]]` records. Values
/// equal to the defaults are left out, so the file shows only what differs.
pub fn render_config(settings: &Settings, header: &str) -> Result<String, AgdError> {
    let encode = |value: &Settings| {
        toml::Value::try_from(value)
            .map_err(|error| config_error(format!("cannot encode settings: {error}")))
    };
    let current = encode(settings)?;
    let defaults = encode(&Settings::default())?;
    let (Some(current), Some(defaults)) = (current.as_table(), defaults.as_table()) else {
        return Err(config_error("settings did not encode as a table"));
    };
    let mut output = String::new();
    for line in header.lines() {
        output.push_str(&format!("# {line}\n").replace("# \n", "#\n"));
    }
    for (comment, keys) in CONFIG_GROUPS {
        let lines: Vec<String> = keys
            .iter()
            .filter_map(|key| {
                let value = current.get(*key)?;
                (defaults.get(*key) != Some(value)).then(|| render_key(key, value))
            })
            .collect();
        if !lines.is_empty() {
            output.push_str(&format!("\n# {comment}\n"));
            output.extend(lines);
        }
    }
    // Records are encoded from the structs, not the sorted table above, so
    // each keeps its declared field order with `key` first.
    let defaults = Settings::default();
    let records = Records {
        capture: (settings.captures != defaults.captures).then_some(&settings.captures),
        view: (settings.views != defaults.views).then_some(&settings.views),
        agent_profile: (!settings.agent_profiles.is_empty()).then_some(&settings.agent_profiles),
    };
    let records = toml::to_string_pretty(&records)
        .map_err(|error| config_error(format!("cannot encode records: {error}")))?;
    if !records.is_empty() {
        output.push('\n');
        output.push_str(&records);
    }
    Ok(output)
}

#[derive(Serialize)]
struct Records<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    capture: Option<&'a Vec<CaptureTemplate>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    view: Option<&'a Vec<ViewSettings>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_profile: Option<&'a Vec<AgentProfile>>,
}

/// `key = value`, with a list that does not fit in 80 columns written one
/// item per line.
fn render_key(key: &str, value: &toml::Value) -> String {
    let line = format!("{key} = {value}\n");
    match value.as_array() {
        Some(items) if line.len() > 80 && !items.is_empty() => {
            let items: String = items.iter().map(|item| format!("  {item},\n")).collect();
            format!("{key} = [\n{items}]\n")
        }
        _ => line,
    }
}

fn validate_settings(settings: &Settings) -> Result<(), AgdError> {
    if settings.roots.is_empty() {
        return Err(config_error("roots must contain at least one directory"));
    }
    let mut roots = BTreeSet::new();
    for root in &settings.roots {
        let expanded = shellexpand::tilde(root).into_owned();
        let canonical = PathBuf::from(&expanded)
            .canonicalize()
            .map_err(|error| config_error(format!("cannot resolve root '{root}': {error}")))?;
        if !canonical.is_dir() {
            return Err(config_error(format!("root is not a directory: {root}")));
        }
        if !roots.insert(canonical) {
            return Err(config_error(format!("duplicate root: {root}")));
        }
    }
    for sequence in &settings.todo_keywords {
        if sequence.iter().filter(|word| word.as_str() == "|").count() != 1 {
            return Err(config_error(
                "every todo_keywords sequence must contain exactly one '|' separator",
            ));
        }
    }
    if settings.editor.is_empty() {
        return Err(config_error("editor needs at least the program to run"));
    }
    match settings.agent_capture_to.as_str() {
        "template" => {}
        "inbox" => {
            if settings
                .capture_template(&settings.agent_inbox_template)
                .is_none()
            {
                return Err(config_error(format!(
                    "agent_capture_to = \"inbox\" needs capture template '{}' (agent_inbox_template)",
                    settings.agent_inbox_template
                )));
            }
        }
        other => {
            return Err(config_error(format!(
                "agent_capture_to must be \"template\" or \"inbox\", not \"{other}\""
            )));
        }
    }
    if !["auto", "dark", "light"].contains(&settings.theme.as_str()) {
        return Err(config_error(format!(
            "theme must be auto, dark or light, not '{}'",
            settings.theme
        )));
    }
    for key in &settings.sorting_strategy {
        if !SORTING_KEYS.contains(&key.as_str()) {
            return Err(config_error(format!(
                "sorting_strategy key '{key}' is not supported; use {}",
                SORTING_KEYS.join(", ")
            )));
        }
    }
    let mut view_keys = BTreeSet::new();
    for view in &settings.views {
        if !view_keys.insert(view.key.as_str()) {
            return Err(config_error(format!("duplicate view key '{}'", view.key)));
        }
        if view.blocks.is_empty() {
            return Err(config_error(format!(
                "view '{}' needs at least one block",
                view.key
            )));
        }
        for block in &view.blocks {
            if !VIEW_BLOCK_KINDS.contains(&block.kind.as_str()) {
                return Err(config_error(format!(
                    "view '{}' has unsupported block kind '{}'; use one of {}",
                    view.key,
                    block.kind,
                    VIEW_BLOCK_KINDS.join(", ")
                )));
            }
            crate::agenda_engine::validate_view_block(block)
                .map_err(|error| config_error(format!("view '{}': {error}", view.key)))?;
        }
    }
    let mut capture_keys = BTreeSet::new();
    for template in &settings.captures {
        let key = &template.key;
        if key.is_empty() || key.chars().any(char::is_whitespace) {
            return Err(config_error(format!(
                "invalid capture template key '{key}'"
            )));
        }
        if !capture_keys.insert(key.as_str()) {
            return Err(config_error(format!(
                "duplicate capture template key '{key}'"
            )));
        }
        if template.file.is_none() && template.id.is_none() {
            return Err(config_error(format!(
                "capture template '{key}' needs a file or ID target"
            )));
        }
    }
    let mut profile_names = BTreeSet::new();
    for profile in &settings.agent_profiles {
        if !profile_names.insert(profile.name.as_str()) {
            return Err(config_error(format!(
                "duplicate agent_profile name '{}'",
                profile.name
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_command_substitutes_every_placeholder_per_argument() {
        let settings = Settings {
            editor: strings(&["emacsclient", "-n", "+{line}:{col}", "{file}"]),
            ..Settings::default()
        };
        assert_eq!(
            settings.editor_command("/org/my tasks.org", 42, 1),
            (
                "emacsclient".to_string(),
                strings(&["-n", "+42:1", "/org/my tasks.org"])
            )
        );
    }

    #[test]
    fn flat_keys_and_records_load_and_unknown_keys_are_errors() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("config.toml");
        fs::write(
            &config,
            format!(
                concat!(
                    "roots = [{root}]\n",
                    "deadline_warning_days = 9\n",
                    "todo_keywords = [[\"TODO\", \"|\", \"DONE\"]]\n",
                    "[[capture]]\n",
                    "key = \"n\"\n",
                    "desc = \"Note\"\n",
                    "file = \"notes.org\"\n",
                    "template = \"* %?\"\n",
                    "[[view]]\n",
                    "key = \"w\"\n",
                    "desc = \"Work\"\n",
                    "blocks = [{{ kind = \"tags-todo\", match = \"work\" }}]\n",
                ),
                root = toml::Value::String(temp.path().to_string_lossy().into_owned())
            ),
        )
        .unwrap();
        let loaded = load_settings(Some(&config), None).unwrap();
        assert_eq!(loaded.settings.deadline_warning_days, 9);
        assert_eq!(loaded.settings.captures.len(), 1);
        assert_eq!(loaded.settings.capture_template("n").unwrap().desc, "Note");
        assert_eq!(
            loaded.settings.view("w").unwrap().blocks[0].kind,
            "tags-todo"
        );

        fs::write(
            &config,
            format!(
                "roots = [{}]\n[agenda]\nspan = \"day\"\n",
                toml::Value::String(temp.path().to_string_lossy().into_owned())
            ),
        )
        .unwrap();
        let error = load_settings(Some(&config), None).unwrap_err();
        assert!(error.to_string().contains("agenda"), "{error}");
    }

    #[test]
    fn rendered_config_round_trips_and_omits_defaults() {
        let settings = Settings {
            roots: strings(&["~/notes"]),
            deadline_warning_days: 5,
            captures: vec![CaptureTemplate {
                key: "j".to_string(),
                desc: "Journal".to_string(),
                file: Some("journal.org".to_string()),
                heading: None,
                outline_path: None,
                id: None,
                datetree: true,
                template: "* %?".to_string(),
                template_file: None,
            }],
            ..Settings::default()
        };
        let rendered = render_config(&settings, "written by a test").unwrap();
        assert!(rendered.starts_with("# written by a test\n"), "{rendered}");
        assert!(rendered.contains("deadline_warning_days = 5"), "{rendered}");
        assert!(!rendered.contains("agenda_span"), "{rendered}");
        assert!(!rendered.contains("[[view]]"), "{rendered}");
        let parsed: Settings = toml::from_str(&rendered).unwrap();
        assert_eq!(parsed, settings);
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
                "roots = [{}]\n",
                toml::Value::String(configured.to_string_lossy().into_owned())
            ),
        )
        .unwrap();

        let loaded = load_settings(Some(&config), Some(&overridden)).unwrap();
        assert_eq!(loaded.settings.roots, vec![overridden.to_string_lossy()]);
    }
}
