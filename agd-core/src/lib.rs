//! agd-core reads and safely changes org-mode files. It is the library behind
//! the `agd` command line, terminal UI and stdio MCP server, and it works on
//! local files only.
//!
//! Every caller follows the same path:
//!
//! 1. [`load_settings`] reads `config.toml` into [`Settings`]: the roots agd
//!    may touch, the agenda files, TODO keywords and the other Org options.
//! 2. [`WorkspaceIndex::load`] parses every agenda file under the roots into an
//!    [`IndexedDocument`] and gives each heading an [`Item`] with a stable
//!    ref: its `:ID:` when unique, else `file:line#sha256:digest`.
//! 3. [`AgendaEngine`] reads the index: agenda days, TODO lists, tags matches,
//!    org-ql queries and saved views, and exports them as Emacs prints them.
//! 4. [`WriteEngine`] changes files: edits, captures, refiles and undo, each
//!    journaled so it can be reverted.
//!
//! # Conventions
//!
//! These hold in every module:
//!
//! - Org files are user data. A write replaces only the bytes it changes and
//!   leaves every other byte exactly as it was, or it is refused and nothing is
//!   written. A file's own syntax (`#+TODO`, `#+PRIORITIES`, `#+STARTUP`) wins
//!   over configuration.
//! - An error is an [`AgdError`] whose `code` is one of [`ErrorCode`], the one
//!   list of agd.error/v1 codes, named by one rule (`invalid_<thing>`,
//!   `unknown_<thing>`, `<thing>_not_found`, `<thing>_conflict`,
//!   `<thing>_failed`; see [`ErrorCode`]); its message says what to do.
//! - Every serialized record names its schema, `agd.<name>/v1`, with a
//!   `*_SCHEMA` constant such as [`ITEM_SCHEMA`] or [`MUTATION_SCHEMA`].
//! - Config keys are snake_case. Values are lowercase, kebab-case when they
//!   name an Org option (`ids = "add-on-write"`, `sorting_strategy =
//!   ["time-up"]`, `kind = "tags-todo"`).
//! - Output is deterministic: lists keep a defined order, and dates are
//!   computed from the `today` or clock the caller passes.
//! - Nothing touches the network.
//!
//! # Example
//!
//! ```
//! use agd_core::{
//!     AgendaEngine, Change, EditIntent, WorkspaceIndex, WriteContext, WriteEngine, load_settings,
//! };
//! use chrono::NaiveDate;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let dir = tempfile::tempdir()?;
//! let tasks = dir.path().join("tasks.org");
//! std::fs::write(&tasks, "* TODO Renew passport\n")?;
//! let config = dir.path().join("config.toml");
//! std::fs::write(&config, "deadline_warning_days = 7\n")?;
//!
//! // `load_settings(None, None)` reads the user's own config.toml instead.
//! let settings = load_settings(Some(&config), Some(dir.path()))?.settings;
//! let index = WorkspaceIndex::load(settings.clone())?;
//! let today = NaiveDate::from_ymd_opt(2026, 10, 16).unwrap();
//! let todo = AgendaEngine::new(&index).todo(None, today)?;
//! assert_eq!(todo[0].title, "Renew passport");
//!
//! let engine = WriteEngine::new(settings, dir.path().join("journal.jsonl"))?;
//! let context = WriteContext {
//!     actor: "example".to_string(),
//!     project: "docs".to_string(),
//!     now: None,
//! };
//! let done = EditIntent {
//!     state: Change::Set("DONE".to_string()),
//!     ..EditIntent::default()
//! };
//! let mutation = engine.edit(&todo[0].item_ref, &done, false, &context)?;
//! assert!(mutation.diff.contains("+* DONE Renew passport"));
//!
//! engine.undo(&mutation.journal_id, &context)?;
//! assert_eq!(std::fs::read_to_string(&tasks)?, "* TODO Renew passport\n");
//! # Ok(())
//! # }
//! ```

// Every module is private; each public item has one path, the re-export
// from this root below.
mod agenda;
mod capture_line;
mod capture_template;
mod check;
mod config;
mod date;
mod document;
mod import;
mod model;
mod path;
mod write;

pub use agenda::{
    AgendaEngine, AgendaQuery, AgendaSpan, AgendaSummary, ViewSection, ViewSectionGroup,
    agenda_csv, agenda_row, agenda_text, agenda_title, item_row, items_csv, keyword_color,
    keyword_colors, overdue_entries,
};
pub use capture_line::{CaptureDraft, parse_capture_line};
pub use capture_template::{CapturePrompt, capture_prompts, render_capture_template};
pub use check::{
    CheckFileReport, CheckProblem, CheckReport, CheckSummary, check_settings, journal_problems,
};
pub use config::{
    AgentProfile, COLOR_NAMES, CaptureTemplate, IdPolicy, LoadedSettings, LogMode, Notation,
    SORTING_KEYS, Settings, VIEW_BLOCK_KINDS, VIEW_GROUP_BY, ViewBlock, ViewGroup, ViewSettings,
    WeekStart, default_config_path, load_settings, parse_agenda_day, parse_config_file,
    render_config,
};
pub use date::{
    date_preview, format_active_timestamp, now_local_naive, parse_date_change, parse_date_input,
    read_date,
};
pub use document::{
    ByteSpan, HeadlineSpan, LogKind, LogSettings, OrgDocument as IndexedDocument, PriorityRange,
    PropertySpan, SourceLine, TimestampSpan, TodoKeyword, WorkspaceIndex, parse_timestamp,
};
pub use import::{
    ConfigEditor, ConfigSources, ImportDiagnostic, ImportResult, ImportStatus, ImportedCapture,
    ImportedCommand, ResolvedEditorConfig, apply_import, find_config_sources, import_root,
};
pub use model::{
    AGENDA_ENTRY_SCHEMA, AgdError, AgendaEntry, CHECK_SCHEMA, DESCRIBE_SCHEMA, ERROR_SCHEMA,
    ErrorCode, ErrorResponse, ITEM_SCHEMA, Item, JOURNAL_SCHEMA, MUTATION_SCHEMA, Mutation,
    Occurrence, OccurrenceType, Planning, PlanningTimestamp, REFILE_SCHEMA, Repeat, STATUS_SCHEMA,
    UNDO_SCHEMA, VIEW_SECTION_SCHEMA, VIEWS_SCHEMA,
};
pub use path::{OrgFiles, RootSet, SkipReason};
pub use write::{
    BatchMutation, Change, EditIntent, ItemEdit, JournalOperation, JournalRecord, JournalStatus,
    Patch, RefileResult, StoredPatch, UndoResult, WriteContext, WriteEngine, actor_name,
    agent_actor, agent_name, apply_patches,
};
