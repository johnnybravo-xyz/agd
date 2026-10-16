//! The configuration's types: `Settings`, the view, capture and agent profile
//! records, the value enums (`IdPolicy`, `WeekStart`, `LogMode`), the lists of
//! accepted values (`SORTING_KEYS`, `VIEW_BLOCK_KINDS`, `VIEW_GROUP_BY`,
//! `COLOR_NAMES`), the defaults, the built-in `n` view, and lookups such as
//! `Settings::view`.
//!
//! The serde shapes are the config file format: field names are the keys.
//! Checks the types cannot express live in `validation`.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::{AgdError, AgendaSpan, ErrorCode};

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

    /// org-todo-keywords, one `#+TODO:` line per sequence, such as
    /// "TODO NEXT WAIT(w@/!) | DONE CANCELLED": read by the same code as a
    /// file's own `#+TODO:` lines, which replace these. A line without `|`
    /// makes its last keyword the done state, as in Org.
    #[serde(deserialize_with = "deserialize_todo_lines")]
    pub todo_keywords: Vec<String>,
    /// org-todo-keyword-faces for the TUI: a color name (COLOR_NAMES) per
    /// keyword. A view section takes the color its rows' keywords share.
    pub todo_keyword_colors: BTreeMap<String, String>,
    /// "preserve" leaves headings alone; "add-on-write" adds an org-id :ID:
    /// on a heading's first edit.
    pub ids: IdPolicy,
    /// org-tags-column: negative right-aligns tags to end at that column,
    /// positive starts them there, as Emacs does when it rewrites a headline.
    pub tags_column: i64,
    pub tag_inheritance_exclusions: Vec<String>,
    pub highest_priority: String,
    pub lowest_priority: String,
    pub default_priority: String,

    /// org-agenda-span: "day", "week", "month" or a number of days
    /// (`agenda_span = 10`).
    pub agenda_span: AgendaSpan,
    pub agenda_start_day: Option<String>,
    /// org-agenda-start-on-weekday: "monday" or "sunday".
    pub week_start: WeekStart,
    pub deadline_warning_days: i64,
    pub skip_scheduled_if_done: bool,
    pub skip_deadline_if_done: bool,
    pub skip_timestamp_if_done: bool,
    /// org-agenda-sorting-strategy for agenda days, from SORTING_KEYS.
    pub sorting_strategy: Vec<String>,

    /// org-log-done, org-log-repeat, org-log-reschedule, org-log-redeadline:
    /// "off" (Emacs's nil), "time" or "note". `#+STARTUP` options win per
    /// file.
    pub log_done: LogMode,
    pub log_repeat: LogMode,
    pub log_reschedule: LogMode,
    pub log_redeadline: LogMode,
    pub log_into_drawer: bool,

    /// Program and arguments to open a file; `{file}`, `{line}` and `{col}`
    /// are substituted. Run directly, never through a shell.
    pub editor: Vec<String>,
    /// auto (follow the terminal background), dark or light.
    pub theme: String,
    /// Round pill and chip caps with Nerd Font glyphs; turn on when your
    /// terminal font has them (Ghostty, WezTerm and kitty bundle them).
    pub nerd_font: bool,
    /// The view key `agd` opens on, such as your dashboard or the built-in
    /// `n`; empty opens Today.
    pub start_view: String,
    /// A `[[capture]]` key: every agent capture goes through that template
    /// without dates, so agent work reaches your day only through triage.
    /// Absent, agents choose a template.
    pub agent_inbox: Option<String>,
    pub someday_tag: String,
    /// Someday also removes the dates that put the task on the agenda
    /// (SCHEDULED, and an overdue DEADLINE), so it leaves the agenda.
    pub someday_clears_dates: bool,
    /// Triage suggests Someday after this many reschedules.
    pub reschedule_nudge: usize,

    #[serde(rename = "capture")]
    pub captures: Vec<CaptureTemplate>,
    /// The user's own views only; `all_views` and `view` add the built-in
    /// ones.
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
    /// For agenda blocks: "day", "week", "month" or a number of days;
    /// without one, `agenda_span`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<AgendaSpan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    /// The header's color in the TUI, one of COLOR_NAMES; by default it
    /// follows the rows' TODO keywords.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// org-super-agenda style groups over the block's rows.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<ViewGroup>,
}

/// One group of a view block, as an org-super-agenda group: each row goes
/// to the first group, in list order, whose match it passes; `discard`
/// drops those rows; `by` makes one group per category, parent heading,
/// TODO keyword or priority cookie. Groups are shown by `order`, then list
/// order, and rows no group takes come last as "Other items".
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ViewGroup {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Tags/TODO match, as for a tags block; without one the group takes
    /// every remaining row.
    #[serde(default, rename = "match", skip_serializing_if = "Option::is_none")]
    pub match_expression: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    /// org-super-agenda's `:effort<` and `:effort>`: the heading's own
    /// `:Effort:` is at most / at least this duration (`0:15`, `30`, `1h`).
    /// A heading without an effort matches neither.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort_max: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort_min: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub discard: bool,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub order: i64,
    /// As for a block; a `by` group's color applies to each group it makes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

fn is_zero(value: &i64) -> bool {
    *value == 0
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureTemplate {
    pub key: String,
    pub desc: String,
    /// The target, as org-capture's: `id` alone, or `file` alone
    /// (file), with `heading` (file+headline) or with `olpath`
    /// (file+olp). `datetree` goes with `file` alone (file+datetree) or with
    /// `olpath` (file+olp+datetree).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub olpath: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub datetree: bool,
    /// The entry's text with org-capture escapes (`%?`, `%U`, `%^{prompt}`).
    /// A template has exactly one of `template` and `template_file`; empty
    /// is none. `load_settings` reads a `template_file` into `template`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub template: String,
    /// A file holding the template text; a relative path is under the first
    /// root, as Org's are under org-directory.
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

/// The day a week agenda starts on.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WeekStart {
    #[default]
    Monday,
    Sunday,
}

/// What a state or planning change records, as an org-log-* variable: no
/// log, a timestamp, or a timestamp and a note.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogMode {
    Off,
    Time,
    Note,
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
pub const VIEW_GROUP_BY: [&str; 4] = ["category", "parent", "todo", "priority"];
/// Colors a keyword or view section may name; the TUI maps them onto its
/// light or dark palette.
pub const COLOR_NAMES: [&str; 10] = [
    "blue", "green", "yellow", "orange", "red", "dim-red", "purple", "pink", "teal", "grey",
];

impl Default for Settings {
    fn default() -> Self {
        Self {
            roots: vec!["~/org".to_string()],
            agenda_files: Vec::new(),
            todo_keywords: strings(&["TODO NEXT WAIT | DONE CANCELLED"]),
            todo_keyword_colors: BTreeMap::new(),
            ids: IdPolicy::Preserve,
            tags_column: -77,
            tag_inheritance_exclusions: Vec::new(),
            highest_priority: "A".to_string(),
            lowest_priority: "C".to_string(),
            default_priority: "B".to_string(),
            agenda_span: AgendaSpan::Week,
            agenda_start_day: None,
            week_start: WeekStart::Monday,
            deadline_warning_days: 14,
            skip_scheduled_if_done: false,
            skip_deadline_if_done: false,
            skip_timestamp_if_done: false,
            // Emacs's default for agenda days (Org 9.7+ names).
            sorting_strategy: strings(&["habit-down", "time-up", "urgency-down", "category-keep"]),
            log_done: LogMode::Time,
            log_repeat: LogMode::Time,
            log_reschedule: LogMode::Off,
            log_redeadline: LogMode::Off,
            log_into_drawer: false,
            editor: vec![
                std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string()),
                "+{line}".to_string(),
                "{file}".to_string(),
            ],
            theme: "auto".to_string(),
            nerd_font: false,
            start_view: String::new(),
            agent_inbox: None,
            someday_tag: "someday".to_string(),
            someday_clears_dates: true,
            reschedule_nudge: 3,
            captures: vec![CaptureTemplate::default()],
            views: Vec::new(),
            agent_profiles: Vec::new(),
        }
    }
}

/// Org's built-in agenda commands that agd runs, beside the user's
/// `[[view]]`s: `n`, "Agenda and all TODOs". A `[[view]]` with the same key
/// replaces one.
static BUILTIN_VIEWS: LazyLock<Vec<ViewSettings>> = LazyLock::new(|| {
    let block = |kind: &str| ViewBlock {
        kind: kind.to_string(),
        match_expression: None,
        span: None,
        header: None,
        color: None,
        groups: Vec::new(),
    };
    vec![ViewSettings {
        key: "n".to_string(),
        desc: "Agenda and all TODOs".to_string(),
        blocks: vec![block("agenda"), block("alltodo")],
    }]
});

/// The default `t` template: a TODO in inbox.org under Inbox, stamped with
/// its creation time.
impl Default for CaptureTemplate {
    fn default() -> Self {
        Self {
            key: "t".to_string(),
            desc: "Todo".to_string(),
            file: Some("inbox.org".to_string()),
            heading: Some("Inbox".to_string()),
            olpath: None,
            id: None,
            datetree: false,
            template: "* TODO %?\n:PROPERTIES:\n:CREATED: %U\n:END:".to_string(),
            template_file: None,
        }
    }
}

pub(super) fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

impl Settings {
    /// The first agenda day: `agenda_start_day` from `today`, or today.
    pub fn agenda_start(&self, today: NaiveDate) -> Result<NaiveDate, AgdError> {
        match self.agenda_start_day.as_deref() {
            Some(value) => parse_agenda_day(value, today),
            None => Ok(today),
        }
    }

    pub fn capture_template(&self, key: &str) -> Option<&CaptureTemplate> {
        self.captures.iter().find(|template| template.key == key)
    }

    /// The template a capture goes through when none is named: the first
    /// `[[capture]]`. (An agent's capture then goes to `agent_inbox` when
    /// it is set, as every agent capture does.)
    pub fn default_capture_template(&self) -> Result<&CaptureTemplate, AgdError> {
        self.captures.first().ok_or_else(|| {
            AgdError::new(
                ErrorCode::UnknownTemplate,
                "no [[capture]] template is configured",
            )
        })
    }

    /// How a capture line names its template, the one rule for every
    /// surface: the first word is the template key when it names a
    /// `[[capture]]` and more text follows; otherwise the whole line is the
    /// text for the default template. Returns the template and the text.
    pub fn capture_line_template<'a>(
        &self,
        line: &'a str,
    ) -> Result<(&CaptureTemplate, &'a str), AgdError> {
        let line = line.trim();
        if let Some((key, text)) = line.split_once(char::is_whitespace)
            && let Some(template) = self.capture_template(key)
        {
            return Ok((template, text.trim_start()));
        }
        Ok((self.default_capture_template()?, line))
    }

    /// The template `key` names, or an `unknown_template` error that lists
    /// the templates there are.
    pub fn find_capture_template(&self, key: &str) -> Result<&CaptureTemplate, AgdError> {
        self.capture_template(key).ok_or_else(|| {
            let keys = self.captures.iter().map(|template| template.key.as_str());
            let known = if self.captures.is_empty() {
                "no [[capture]] template is configured".to_string()
            } else {
                format!("the templates are {}", quoted_list(keys))
            };
            AgdError::new(
                ErrorCode::UnknownTemplate,
                format!("unknown capture template '{key}'; {known}"),
            )
        })
    }

    /// The view `key` runs, or an `unknown_view` error that lists every
    /// view, the built-in ones too.
    pub fn find_view(&self, key: &str) -> Result<&ViewSettings, AgdError> {
        self.view(key).ok_or_else(|| {
            AgdError::new(
                ErrorCode::UnknownView,
                format!("unknown view '{key}'; the views are {}", self.view_keys()),
            )
        })
    }

    /// The view `agd agenda --view` without a key runs: start_view.
    pub fn find_start_view(&self) -> Result<&ViewSettings, AgdError> {
        if self.start_view.is_empty() {
            return Err(AgdError::new(
                ErrorCode::UnknownView,
                format!(
                    "--view without a key runs start_view, which is not set; name one of {}",
                    self.view_keys()
                ),
            ));
        }
        self.find_view(&self.start_view)
    }

    /// Every view's key, quoted, for messages: `'w' and 'n'`.
    pub(super) fn view_keys(&self) -> String {
        quoted_list(self.all_views().iter().map(|view| view.key.as_str()))
    }

    /// The view `key` runs: the user's `[[view]]`, else a built-in one.
    pub fn view(&self, key: &str) -> Option<&ViewSettings> {
        self.all_views().into_iter().find(|view| view.key == key)
    }

    /// Every view a key can run: the user's `[[view]]`s in config order,
    /// then the built-in views they do not replace.
    pub fn all_views(&self) -> Vec<&ViewSettings> {
        let builtins = BUILTIN_VIEWS
            .iter()
            .filter(|builtin| self.views.iter().all(|view| view.key != builtin.key));
        self.views.iter().chain(builtins).collect()
    }

    /// Whether `key` runs a built-in view rather than one of the user's.
    pub fn is_builtin_view(&self, key: &str) -> bool {
        self.views.iter().all(|view| view.key != key)
            && BUILTIN_VIEWS.iter().any(|builtin| builtin.key == key)
    }

    pub fn agent_profile(&self, name: &str) -> Option<&AgentProfile> {
        self.agent_profiles
            .iter()
            .find(|profile| profile.name == name)
    }

    /// Narrows these settings to `[[agent_profile]]` `name`, as `agd
    /// agenda --profile` and MCP `agenda` do: its files replace
    /// agenda_files, and its match is the match unless `match_expression`
    /// gives one, which this returns.
    pub fn apply_agent_profile(
        &mut self,
        name: &str,
        match_expression: Option<String>,
    ) -> Result<Option<String>, AgdError> {
        let profile = self.find_agent_profile(name)?.clone();
        if !profile.files.is_empty() {
            self.agenda_files = profile.files;
        }
        Ok(match_expression.or(profile.match_expression))
    }

    /// The `[[agent_profile]]` `name` names, or an `unknown_profile` error
    /// that lists the profiles there are.
    pub fn find_agent_profile(&self, name: &str) -> Result<&AgentProfile, AgdError> {
        self.agent_profile(name).ok_or_else(|| {
            let names = self
                .agent_profiles
                .iter()
                .map(|profile| profile.name.as_str());
            let known = if self.agent_profiles.is_empty() {
                "no [[agent_profile]] is configured".to_string()
            } else {
                format!("the profiles are {}", quoted_list(names))
            };
            AgdError::new(
                ErrorCode::UnknownProfile,
                format!("unknown agent profile '{name}'; {known}"),
            )
        })
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

/// Names for a message, each in single quotes: `'a'`, `'a' and 'b'`,
/// `'a', 'b' and 'c'`.
pub(crate) fn quoted_list<'a>(names: impl IntoIterator<Item = &'a str>) -> String {
    let quoted: Vec<String> = names.into_iter().map(|name| format!("'{name}'")).collect();
    match quoted.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

/// `todo_keywords` as strings. A list of words instead of a line gets an
/// error that spells out the line to write.
fn deserialize_todo_lines<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum TodoLine {
        Line(String),
        Words(Vec<String>),
    }
    Vec::<TodoLine>::deserialize(deserializer)?
        .into_iter()
        .map(|line| match line {
            TodoLine::Line(line) => Ok(line),
            TodoLine::Words(words) => Err(serde::de::Error::custom(format!(
                "todo_keywords takes one #+TODO: line per sequence; write \"{}\"",
                words.join(" ")
            ))),
        })
        .collect()
}

/// A day as org-agenda-start-day and `agd agenda --from` take it: a date,
/// an offset from `today` such as -1d or +2w, or what `parse_date_input`
/// reads, such as "fri".
pub fn parse_agenda_day(value: &str, today: NaiveDate) -> Result<NaiveDate, AgdError> {
    crate::date::parse_org_date(&crate::parse_date_input(value, today)?.date)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::config::test_support::{load_body, load_error};
    use crate::render_config;

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
    fn a_capture_line_names_its_template_only_with_text_after_the_key() {
        let settings = Settings {
            captures: vec![
                CaptureTemplate::default(),
                CaptureTemplate {
                    key: "n".to_string(),
                    ..CaptureTemplate::default()
                },
            ],
            ..Settings::default()
        };
        let read = |line: &str| {
            let (template, text) = settings.capture_line_template(line).unwrap();
            (template.key.clone(), text.to_string())
        };
        assert_eq!(
            read("n Call back"),
            ("n".to_string(), "Call back".to_string())
        );
        assert_eq!(
            read("Renew passport"),
            ("t".to_string(), "Renew passport".to_string())
        );
        assert_eq!(read("n"), ("t".to_string(), "n".to_string()));
        assert_eq!(
            read("x -1 point"),
            ("t".to_string(), "x -1 point".to_string())
        );
    }

    #[test]
    fn unknown_names_list_what_is_configured_or_say_nothing_is() {
        let settings = Settings {
            agent_profiles: vec![AgentProfile {
                name: "coding".to_string(),
                ..AgentProfile::default()
            }],
            ..Settings::default()
        };
        let message = |error: AgdError| (error.code(), error.to_string());
        assert_eq!(
            message(settings.find_view("x").unwrap_err()),
            (
                ErrorCode::UnknownView,
                "unknown view 'x'; the views are 'n'".to_string()
            )
        );
        assert_eq!(
            message(settings.find_start_view().unwrap_err()).1,
            "--view without a key runs start_view, which is not set; name one of 'n'"
        );
        assert_eq!(
            message(settings.find_agent_profile("x").unwrap_err()).1,
            "unknown agent profile 'x'; the profiles are 'coding'"
        );
        assert_eq!(
            message(Settings::default().find_agent_profile("x").unwrap_err()).1,
            "unknown agent profile 'x'; no [[agent_profile]] is configured"
        );
        let none = Settings {
            captures: Vec::new(),
            ..Settings::default()
        };
        assert_eq!(
            message(none.find_capture_template("t").unwrap_err()),
            (
                ErrorCode::UnknownTemplate,
                "unknown capture template 't'; no [[capture]] template is configured".to_string()
            )
        );
        assert_eq!(quoted_list(["a", "b", "c"]), "'a', 'b' and 'c'");
    }

    #[test]
    fn builtin_n_stays_beside_user_views_until_replaced() {
        let defaults = Settings::default();
        assert!(defaults.views.is_empty());
        assert_eq!(defaults.view("n").unwrap().desc, "Agenda and all TODOs");
        assert!(defaults.is_builtin_view("n"));

        let settings = load_body(concat!(
            "start_view = \"n\"\n",
            "[[view]]\nkey = \"w\"\ndesc = \"Work\"\n",
            "blocks = [{ kind = \"tags-todo\", match = \"work\" }]\n",
        ))
        .unwrap();
        let keys: Vec<&str> = settings
            .all_views()
            .iter()
            .map(|view| view.key.as_str())
            .collect();
        assert_eq!(keys, ["w", "n"]);
        assert!(!settings.is_builtin_view("w"));
        let rendered = render_config(&settings, "views").unwrap();
        assert!(rendered.contains("key = \"w\""), "{rendered}");
        assert!(!rendered.contains("key = \"n\""), "{rendered}");

        let replaced = load_body(concat!(
            "[[view]]\nkey = \"n\"\ndesc = \"Next\"\n",
            "blocks = [{ kind = \"todo\", match = \"NEXT\" }]\n",
        ))
        .unwrap();
        assert_eq!(replaced.all_views().len(), 1);
        assert_eq!(replaced.view("n").unwrap().desc, "Next");
        assert!(!replaced.is_builtin_view("n"));
    }

    #[test]
    fn week_start_is_monday_or_sunday() {
        let settings = load_body("week_start = \"sunday\"\n").unwrap();
        assert_eq!(settings.week_start, WeekStart::Sunday);
        assert_eq!(
            load_error("week_start = \"friday\"\n"),
            concat!(
                "cannot parse CONFIG: TOML parse error at line 2, column 14\n",
                "  |\n",
                "2 | week_start = \"friday\"\n",
                "  |              ^^^^^^^^\n",
                "unknown variant `friday`, expected `monday` or `sunday`\n",
            )
        );
    }

    #[test]
    fn log_settings_are_off_time_or_note() {
        let settings = load_body("log_done = \"note\"\nlog_repeat = \"off\"\n").unwrap();
        assert_eq!(
            (settings.log_done, settings.log_repeat),
            (LogMode::Note, LogMode::Off)
        );
        assert_eq!(
            load_error("log_reschedule = \"yes\"\n"),
            concat!(
                "cannot parse CONFIG: TOML parse error at line 2, column 18\n",
                "  |\n",
                "2 | log_reschedule = \"yes\"\n",
                "  |                  ^^^^^\n",
                "unknown variant `yes`, expected one of `off`, `time`, `note`\n",
            )
        );
        assert!(
            load_error("log_done = \"nil\"\n")
                .ends_with("unknown variant `nil`, expected one of `off`, `time`, `note`\n")
        );
    }

    #[test]
    fn todo_keyword_lines_read_like_a_files_todo_lines() {
        let settings = Settings {
            todo_keywords: strings(&["TODO(t) WAIT(w@/!) | DONE(d!)", "IDEA DROPPED"]),
            ..Settings::default()
        };
        let document = crate::IndexedDocument::parse(
            PathBuf::from("tasks.org"),
            b"* WAIT Reply\n".to_vec(),
            &settings,
        )
        .unwrap();
        let keywords: Vec<(&str, bool, usize)> = document
            .todo_keywords()
            .iter()
            .map(|keyword| (keyword.name.as_str(), keyword.done, keyword.sequence))
            .collect();
        assert_eq!(
            keywords,
            [
                ("TODO", false, 0),
                ("WAIT", false, 0),
                ("DONE", true, 0),
                ("IDEA", false, 1),
                ("DROPPED", true, 1)
            ]
        );
        let wait = &document.todo_keywords()[1];
        assert_eq!(
            (wait.log_enter, wait.log_leave),
            (Some(crate::LogKind::Note), Some(crate::LogKind::Time))
        );
    }
}
