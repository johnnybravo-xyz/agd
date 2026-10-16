//! The records agd hands to callers: [`Item`], [`AgendaEntry`], [`Mutation`]
//! and their parts, every schema name a record carries, and errors:
//! [`AgdError`], its [`ErrorResponse`], and [`ErrorCode`], the one list of
//! agd.error/v1 codes.
//!
//! These are plain data with no behavior beyond construction and display.
//! A schema name or an error code is spelled only here, never as a literal
//! where a record or error is made.
//!
//! # Error codes
//!
//! Every code names what went wrong with one rule, so a consumer can tell
//! the kind of failure from the code alone:
//!
//! - `invalid_<thing>`: the input is wrong (`invalid_date`, `invalid_span`,
//!   `invalid_config`); fix it and try again.
//! - `unknown_<thing>`: a name that is not configured (`unknown_view`,
//!   `unknown_template`); the message lists the names there are.
//! - `<thing>_not_found`: a target that is missing (`file_not_found`,
//!   `journal_not_found`).
//! - `<thing>_conflict`: something changed or clashes since it was read
//!   (`ref_conflict`, `id_conflict`, `undo_conflict`); nothing is written.
//! - `<thing>_failed`: an operation that failed (`io_failed`,
//!   `lock_failed`, `parse_failed`).
//!
//! One failure has one code on every surface: the CLI's JSON, MCP and
//! `agd setup check`.

use std::fmt;

use serde::{Deserialize, Serialize};

pub const ITEM_SCHEMA: &str = "agd.item/v1";
pub const AGENDA_ENTRY_SCHEMA: &str = "agd.agenda-entry/v1";
pub const MUTATION_SCHEMA: &str = "agd.mutation/v1";
pub const ERROR_SCHEMA: &str = "agd.error/v1";
/// One block of `agd agenda --view KEY --json`.
pub const VIEW_SECTION_SCHEMA: &str = "agd.view-section/v1";
/// The payload of `agd agenda --views --json`.
pub const VIEWS_SCHEMA: &str = "agd.views/v1";
/// The payload of `agd agenda --status --json`: the status band's counts
/// (`AgendaSummary`) and the overdue rows (`overdue_entries`).
pub const STATUS_SCHEMA: &str = "agd.status/v1";
/// An `agd setup check` report (`CheckReport`).
pub const CHECK_SCHEMA: &str = "agd.check/v1";
/// One line of the write journal (`JournalRecord`).
pub const JOURNAL_SCHEMA: &str = "agd.journal/v1";
/// What an undo wrote (`UndoResult`).
pub const UNDO_SCHEMA: &str = "agd.undo/v1";
/// What a refile wrote (`RefileResult`).
pub const REFILE_SCHEMA: &str = "agd.refile/v1";
/// The MCP `describe` tool's answer.
pub const DESCRIBE_SCHEMA: &str = "agd.describe/v1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Item {
    pub schema: String,
    #[serde(rename = "ref")]
    pub item_ref: String,
    pub id: Option<String>,
    pub file: String,
    pub line: usize,
    pub olpath: Vec<String>,
    pub todo: Option<String>,
    pub done: bool,
    pub priority: Option<String>,
    pub title: String,
    pub tags: Vec<String>,
    pub inherited_tags: Vec<String>,
    pub category: String,
    pub planning: Planning,
    /// The entry's checklist progress, which lists show as `1/3`; absent
    /// without a checklist, or when the heading has its own `[/]` or `[%]`
    /// cookie.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checklist: Option<Progress>,
}

/// A checklist's progress: `done` of its `total` boxes are checked.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Planning {
    pub scheduled: Option<PlanningTimestamp>,
    pub deadline: Option<PlanningTimestamp>,
    pub closed: Option<PlanningTimestamp>,
}

/// An Org timestamp: `date` is `YYYY-MM-DD`, and `time` and `end_time` are
/// always `HH:MM` (`<… 9:30>` reads `09:30`); `raw` is the text as the file
/// writes it, which a write that leaves the timestamp alone keeps.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PlanningTimestamp {
    pub date: String,
    pub time: Option<String>,
    pub end_time: Option<String>,
    pub repeater: Option<String>,
    pub warning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgendaEntry {
    pub schema: String,
    pub item: Item,
    pub occurrence: Occurrence,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Occurrence {
    #[serde(rename = "type")]
    pub kind: OccurrenceType,
    pub date: String,
    /// `HH:MM`, as every time in the model.
    pub time: Option<String>,
    /// The end of a time range such as `10:00-11:30`; null without one.
    #[serde(default)]
    pub end_time: Option<String>,
    /// Org's leader for the row, as the agenda prints it (`Sched.12x:`,
    /// `3 d. ago:`, `In   5 d.:`).
    pub extra: String,
    /// Days late on an overdue row (`Sched.12x:` is 12, `3 d. ago:` is 3),
    /// minus the days left on an upcoming deadline (`In   5 d.:` is -5),
    /// and 0 on every other row.
    #[serde(default)]
    pub days: i64,
    /// Whether the row is overdue: a past-scheduled reminder or a missed
    /// deadline, both shown on today.
    #[serde(default)]
    pub late: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OccurrenceType {
    Scheduled,
    PastScheduled,
    Deadline,
    UpcomingDeadline,
    Timestamp,
    Block,
    Closed,
    Todo,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Mutation {
    pub schema: String,
    pub ok: bool,
    pub item: Item,
    pub diff: String,
    pub journal_id: String,
    /// Set when marking a repeating task done moved its dates on instead
    /// of closing it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeated: Option<Repeat>,
}

/// A repeating task marked done (org-auto-repeat-maybe): the done state
/// that was recorded, and the next occurrence the item now has.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Repeat {
    /// The done keyword requested, such as `DONE`.
    pub done_state: String,
    /// `YYYY-MM-DD` of the repeating SCHEDULED date, else of the repeating
    /// DEADLINE.
    pub next_date: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ErrorResponse {
    pub schema: String,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<Item>,
}

/// Every agd.error/v1 `code`, in one place, for agd-core and the `agd`
/// binary alike: each variant's `as_str` is the snake_case code consumers
/// match on, and an error is raised as `AgdError::new(ErrorCode::Variant,
/// message)`. `agd setup check` reports its problems with the same codes.
///
/// Every name follows one rule (see the module documentation):
/// `invalid_<thing>`, `unknown_<thing>`, `<thing>_not_found`,
/// `<thing>_conflict` or `<thing>_failed`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorCode {
    /// A capture template's heading, outline path or ID target that is
    /// missing, or not unique on its level.
    CaptureTargetNotFound,
    /// A command that failed outside agd-core's own errors, such as output
    /// that cannot be written.
    CommandFailed,
    /// A file an editor holds unsaved changes for: an Emacs `.#` lock exists.
    EditorConflict,
    /// A result that cannot be encoded as JSON.
    EncodeFailed,
    /// A file that is not under any root, or cannot be resolved.
    FileNotFound,
    /// An `:ID:` that more than one heading under the roots has, or that a
    /// write would give a second heading.
    IdConflict,
    /// Editor configuration that cannot be found or read for an import.
    ImportFailed,
    /// An agenda file entry without the `.org` extension.
    InvalidAgendaFile,
    /// An agenda file pattern that is not a valid glob, or an absolute entry
    /// that is not an exact file under a root.
    InvalidAgendaPattern,
    /// A command line, or an MCP tool call's arguments, that do not parse:
    /// an unknown flag or argument, a missing value, a value of the wrong
    /// type.
    InvalidArguments,
    /// A batch of edits that lists one item twice.
    InvalidBatch,
    /// A capture body line that would start a heading.
    InvalidBody,
    /// A capture line whose title is empty.
    InvalidCapture,
    /// A config file that is missing, unreadable or holds an invalid value,
    /// or a config or data directory that cannot be determined.
    InvalidConfig,
    /// A date or time that does not parse; the message lists the forms it
    /// takes.
    InvalidDate,
    /// An edit that would change nothing: it names no change, or the item's
    /// bytes would stay as they are.
    InvalidEdit,
    /// A change to, or removal of, a heading's existing `:ID:`.
    InvalidIdChange,
    /// Imported editor configuration agd cannot turn into a config that
    /// loads.
    InvalidImport,
    /// A journal line that is not a record, or an inverse patch that does
    /// not restore the bytes the entry recorded.
    InvalidJournal,
    /// A review or undo of a journal entry in the wrong state: a review of
    /// one not awaiting review, an undo of one that is not a committed
    /// change.
    InvalidJournalState,
    /// A tags/TODO match expression that does not parse.
    InvalidMatch,
    /// A log note of several lines.
    InvalidNote,
    /// A patch that overlaps another, runs past the source or splits a UTF-8
    /// character.
    InvalidPatch,
    /// A path agd will not touch: with a `..` segment, outside the roots, a
    /// symlink to write through, without a parent, or not a regular file.
    InvalidPath,
    /// An MCP `plan` call with neither a scheduled nor a deadline date.
    InvalidPlan,
    /// A priority outside the file's range.
    InvalidPriority,
    /// A capture template prompt with no answer, or an answer of several
    /// lines.
    InvalidPrompt,
    /// A property key outside `[A-Za-z0-9_-]`, or a value of several lines.
    InvalidProperty,
    /// An org-ql query that does not parse; the message lists the supported
    /// predicates.
    InvalidQuery,
    /// An agenda range that ends before it starts.
    InvalidRange,
    /// An item ref that is neither an ID nor `file:line#sha256:digest`.
    InvalidRef,
    /// A refile into the item itself or its subtree, or one that would leave
    /// a heading without stars.
    InvalidRefile,
    /// A repeater that does not parse, repeats every 0 units, or repeats past
    /// the calendar.
    InvalidRepeater,
    /// No roots, or a root that cannot be resolved, is not a directory or is
    /// listed twice.
    InvalidRoot,
    /// An agenda span that is not day, week, month or a number of days.
    InvalidSpan,
    /// A tag with characters outside Org's tag characters.
    InvalidTag,
    /// A capture template agd cannot use: one with a `%(sexp)` form, which
    /// agd never runs, or one with neither a file nor an ID target.
    InvalidTemplate,
    /// A title that is not one line, that is cleared, or that the parser
    /// would read as a TODO keyword, priority cookie or tags.
    InvalidTitle,
    /// A TODO state the item's file does not define.
    InvalidTodo,
    /// An org file that is not UTF-8.
    InvalidUtf8,
    /// A view group's effort bound that is not a duration.
    InvalidView,
    /// A file system operation that failed; the message names the path.
    IoFailed,
    /// A write met an interrupted earlier write whose file changed since.
    JournalConflict,
    /// A journal record that cannot be encoded.
    JournalFailed,
    /// A journal entry or transaction id that is not in the journal, or no
    /// entry that `agd undo` may undo.
    JournalNotFound,
    /// A write lock that cannot be opened or taken.
    LockFailed,
    /// A file agd cannot read faithfully: the Org parser would not
    /// reproduce it byte for byte, its text does not read as agd expects
    /// (an unterminated property drawer), or a written heading cannot be
    /// found again.
    ParseFailed,
    /// A relative path more than one root could hold.
    PathConflict,
    /// A ref whose heading changed, moved out of the agenda files or no
    /// longer exists since it was read; the error's `current` is the item
    /// as it reads now, when there is one.
    RefConflict,
    /// An undo whose change was edited since; nothing is written.
    UndoConflict,
    /// An agent profile name that is not configured.
    UnknownProfile,
    /// A capture template key that is not configured.
    UnknownTemplate,
    /// A view key that names no view, or `--view` without a key when no
    /// start_view is set.
    UnknownView,
    /// A directory under a root that cannot be listed.
    WalkFailed,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CaptureTargetNotFound => "capture_target_not_found",
            Self::CommandFailed => "command_failed",
            Self::EditorConflict => "editor_conflict",
            Self::EncodeFailed => "encode_failed",
            Self::FileNotFound => "file_not_found",
            Self::IdConflict => "id_conflict",
            Self::ImportFailed => "import_failed",
            Self::InvalidAgendaFile => "invalid_agenda_file",
            Self::InvalidAgendaPattern => "invalid_agenda_pattern",
            Self::InvalidArguments => "invalid_arguments",
            Self::InvalidBatch => "invalid_batch",
            Self::InvalidBody => "invalid_body",
            Self::InvalidCapture => "invalid_capture",
            Self::InvalidConfig => "invalid_config",
            Self::InvalidDate => "invalid_date",
            Self::InvalidEdit => "invalid_edit",
            Self::InvalidIdChange => "invalid_id_change",
            Self::InvalidImport => "invalid_import",
            Self::InvalidJournal => "invalid_journal",
            Self::InvalidJournalState => "invalid_journal_state",
            Self::InvalidMatch => "invalid_match",
            Self::InvalidNote => "invalid_note",
            Self::InvalidPatch => "invalid_patch",
            Self::InvalidPath => "invalid_path",
            Self::InvalidPlan => "invalid_plan",
            Self::InvalidPriority => "invalid_priority",
            Self::InvalidPrompt => "invalid_prompt",
            Self::InvalidProperty => "invalid_property",
            Self::InvalidQuery => "invalid_query",
            Self::InvalidRange => "invalid_range",
            Self::InvalidRef => "invalid_ref",
            Self::InvalidRefile => "invalid_refile",
            Self::InvalidRepeater => "invalid_repeater",
            Self::InvalidRoot => "invalid_root",
            Self::InvalidSpan => "invalid_span",
            Self::InvalidTag => "invalid_tag",
            Self::InvalidTemplate => "invalid_template",
            Self::InvalidTitle => "invalid_title",
            Self::InvalidTodo => "invalid_todo",
            Self::InvalidUtf8 => "invalid_utf8",
            Self::InvalidView => "invalid_view",
            Self::IoFailed => "io_failed",
            Self::JournalConflict => "journal_conflict",
            Self::JournalFailed => "journal_failed",
            Self::JournalNotFound => "journal_not_found",
            Self::LockFailed => "lock_failed",
            Self::ParseFailed => "parse_failed",
            Self::PathConflict => "path_conflict",
            Self::RefConflict => "ref_conflict",
            Self::UndoConflict => "undo_conflict",
            Self::UnknownProfile => "unknown_profile",
            Self::UnknownTemplate => "unknown_template",
            Self::UnknownView => "unknown_view",
            Self::WalkFailed => "walk_failed",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug)]
pub struct AgdError {
    code: ErrorCode,
    response: Box<ErrorResponse>,
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl AgdError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            response: Box::new(ErrorResponse {
                schema: ERROR_SCHEMA.to_string(),
                code: code.as_str().to_string(),
                message: message.into(),
                current: None,
            }),
            source: None,
        }
    }

    /// The same error with `context: ` before its message, such as which
    /// of several targets it is about.
    pub fn with_context(mut self, context: &str) -> Self {
        self.response.message = format!("{context}: {}", self.response.message);
        self
    }

    pub fn with_current(mut self, current: Item) -> Self {
        self.response.current = Some(current);
        self
    }

    pub fn with_source(
        code: ErrorCode,
        message: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            source: Some(Box::new(source)),
            ..Self::new(code, message)
        }
    }

    pub fn code(&self) -> ErrorCode {
        self.code
    }

    pub fn response(&self) -> &ErrorResponse {
        &self.response
    }

    pub fn into_response(self) -> ErrorResponse {
        *self.response
    }
}

impl fmt::Display for AgdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.response.message)
    }
}

impl std::error::Error for AgdError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn std::error::Error + 'static))
    }
}

impl From<std::io::Error> for AgdError {
    fn from(error: std::io::Error) -> Self {
        Self::with_source(ErrorCode::IoFailed, error.to_string(), error)
    }
}
