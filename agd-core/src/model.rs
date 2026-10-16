//! The records agd hands to callers: [`Item`], [`AgendaEntry`], [`Mutation`]
//! and their parts, the `agd.<name>/v1` schema names, and errors: [`AgdError`],
//! its [`ErrorResponse`], and [`ErrorCode`], the one list of agd.error/v1
//! codes.
//!
//! These are plain data with no behavior beyond construction and display,
//! and a code is raised only through [`ErrorCode`], never spelled as a
//! literal.

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
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Planning {
    pub scheduled: Option<PlanningTimestamp>,
    pub deadline: Option<PlanningTimestamp>,
    pub closed: Option<PlanningTimestamp>,
}

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
    pub time: Option<String>,
    /// The end of a time range such as `10:00-11:30`; null without one.
    #[serde(default)]
    pub end_time: Option<String>,
    pub extra: String,
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

/// Every agd.error/v1 `code` agd-core raises, in one place: each
/// variant's `as_str` is the snake_case code consumers match on, and an
/// error is raised as `AgdError::new(ErrorCode::Variant, message)`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorCode {
    /// A relative path more than one root could hold.
    AmbiguousPath,
    /// A capture template's heading, outline path or ID target that is
    /// missing, or not unique on its level.
    CaptureTargetNotFound,
    /// A config file that is missing, unreadable or holds an invalid value,
    /// or a config or data directory that cannot be determined.
    Config,
    /// An `:ID:` that more than one heading under the roots has, or that a
    /// write would give a second heading.
    DuplicateId,
    /// A file an editor holds unsaved changes for: an Emacs `.#` lock exists.
    EditorDirty,
    /// A change to, or removal of, a heading's existing `:ID:`.
    IdImmutable,
    /// Editor configuration that cannot be found or read for an import.
    Import,
    /// An agenda file entry without the `.org` extension.
    InvalidAgendaFile,
    /// An agenda file pattern that is not a valid glob, or an absolute entry
    /// that is not an exact file under a root.
    InvalidAgendaPattern,
    /// A capture body line that would start a heading.
    InvalidBody,
    /// A capture line whose title is empty.
    InvalidCapture,
    /// A date or time that does not parse; the date prompt's message lists
    /// the forms it takes.
    InvalidDate,
    /// An edit with nothing to change, a cleared title, or a batch that
    /// lists one item twice.
    InvalidEdit,
    /// A review of a journal entry that is not awaiting review.
    InvalidJournalState,
    /// A tags/TODO match expression that does not parse.
    InvalidMatch,
    /// A log note of several lines.
    InvalidNote,
    /// A patch that overlaps another, runs past the source or splits a UTF-8
    /// character.
    InvalidPatch,
    /// A path with no parent, one that cannot be resolved, or a target that
    /// is not a regular file.
    InvalidPath,
    /// A priority outside the file's range.
    InvalidPriority,
    /// A capture prompt answer of several lines.
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
    /// No roots, or a root that cannot be resolved or is not a directory.
    InvalidRoot,
    /// An agenda span that is not day, week, month or a number of days.
    InvalidSpan,
    /// A tag with characters outside Org's tag characters.
    InvalidTag,
    /// A title that is not one line, or that the parser would read as a
    /// TODO keyword, priority cookie or tags.
    InvalidTitle,
    /// A TODO state the item's file does not define.
    InvalidTodo,
    /// A view group's effort bound that is not a duration.
    InvalidView,
    /// A file system operation that failed; the message names the path.
    Io,
    /// A write met an interrupted earlier write whose file changed since.
    JournalConflict,
    /// A journal line that is not a record, or an inverse patch that does
    /// not restore the bytes the entry recorded.
    JournalCorrupt,
    /// A journal record that cannot be encoded.
    JournalError,
    /// A journal entry or transaction id that is not in the journal.
    JournalNotFound,
    /// A write lock that cannot be opened or taken.
    LockFailed,
    /// A capture template prompt with no answer.
    MissingPrompt,
    /// An edit that would leave the item's bytes as they are.
    NoChange,
    /// An org file that is not UTF-8.
    NonUtf8,
    /// A file that is not under any root, or cannot be resolved.
    NotFound,
    /// An undo of a journal entry that is not a committed change.
    NothingToUndo,
    /// A path that resolves outside the configured roots.
    OutsideRoots,
    /// A file the Org parser would not reproduce byte for byte.
    ParseDrift,
    /// Org text that does not read as agd expects: an unterminated property
    /// drawer, or a written heading that cannot be found again.
    ParseError,
    /// A path with a `..` segment.
    PathTraversal,
    /// A ref whose heading changed, moved out of the agenda files or no
    /// longer exists.
    StaleRef,
    /// A write whose target is a symlink.
    SymlinkWriteRefused,
    /// An undo whose change was edited since; nothing is written.
    UndoConflict,
    /// A capture template key that is not configured.
    UnknownTemplate,
    /// Imported editor configuration agd cannot turn into a config that
    /// loads.
    UnsupportedImportShape,
    /// A capture template with neither a file nor an ID target.
    UnsupportedTarget,
    /// A capture template with a `%(sexp)` form, which agd never runs.
    UnsupportedTemplate,
    /// A directory under a root that cannot be listed.
    WalkError,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AmbiguousPath => "ambiguous_path",
            Self::CaptureTargetNotFound => "capture_target_not_found",
            Self::Config => "config",
            Self::DuplicateId => "duplicate_id",
            Self::EditorDirty => "editor_dirty",
            Self::IdImmutable => "id_immutable",
            Self::Import => "import",
            Self::InvalidAgendaFile => "invalid_agenda_file",
            Self::InvalidAgendaPattern => "invalid_agenda_pattern",
            Self::InvalidBody => "invalid_body",
            Self::InvalidCapture => "invalid_capture",
            Self::InvalidDate => "invalid_date",
            Self::InvalidEdit => "invalid_edit",
            Self::InvalidJournalState => "invalid_journal_state",
            Self::InvalidMatch => "invalid_match",
            Self::InvalidNote => "invalid_note",
            Self::InvalidPatch => "invalid_patch",
            Self::InvalidPath => "invalid_path",
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
            Self::InvalidTitle => "invalid_title",
            Self::InvalidTodo => "invalid_todo",
            Self::InvalidView => "invalid_view",
            Self::Io => "io",
            Self::JournalConflict => "journal_conflict",
            Self::JournalCorrupt => "journal_corrupt",
            Self::JournalError => "journal_error",
            Self::JournalNotFound => "journal_not_found",
            Self::LockFailed => "lock_failed",
            Self::MissingPrompt => "missing_prompt",
            Self::NoChange => "no_change",
            Self::NonUtf8 => "non_utf8",
            Self::NotFound => "not_found",
            Self::NothingToUndo => "nothing_to_undo",
            Self::OutsideRoots => "outside_roots",
            Self::ParseDrift => "parse_drift",
            Self::ParseError => "parse_error",
            Self::PathTraversal => "path_traversal",
            Self::StaleRef => "stale_ref",
            Self::SymlinkWriteRefused => "symlink_write_refused",
            Self::UndoConflict => "undo_conflict",
            Self::UnknownTemplate => "unknown_template",
            Self::UnsupportedImportShape => "unsupported_import_shape",
            Self::UnsupportedTarget => "unsupported_target",
            Self::UnsupportedTemplate => "unsupported_template",
            Self::WalkError => "walk_error",
        }
    }
}

impl From<ErrorCode> for String {
    fn from(code: ErrorCode) -> Self {
        code.as_str().to_string()
    }
}

#[derive(Debug)]
pub struct AgdError {
    response: Box<ErrorResponse>,
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl AgdError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            response: Box::new(ErrorResponse {
                schema: ERROR_SCHEMA.to_string(),
                code: code.into(),
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
        code: impl Into<String>,
        message: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            response: Box::new(ErrorResponse {
                schema: ERROR_SCHEMA.to_string(),
                code: code.into(),
                message: message.into(),
                current: None,
            }),
            source: Some(Box::new(source)),
        }
    }

    pub fn code(&self) -> &str {
        &self.response.code
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
        Self::with_source(ErrorCode::Io, error.to_string(), error)
    }
}
