pub mod check;
pub mod agenda_engine;
pub mod config;
pub mod date;
pub mod document;
pub mod error;
pub mod model;
pub mod imports;
pub mod org_mode;
pub mod paths;
pub mod settings;
pub mod write;
pub mod utils;

#[cfg(test)]
mod error_tests;

pub use agenda_engine::{AgendaEngine, AgendaQuery, AgendaSpan, agenda_csv};
pub use check::{
    CHECK_SCHEMA, CheckFileReport, CheckProblem, CheckReport, CheckSummary, DocumentParseError,
    IndexedHeadline, OrgDocument, SourceSpan, check_paths, check_settings,
};
pub use config::{LoggingConfig, OrgConfig};
pub use date::{
    CaptureDraft, format_active_timestamp, now_local_naive, parse_capture_line, parse_date_input,
    render_capture_template,
};
pub use document::{
    ByteSpan, HeadlineSpan, OrgDocument as IndexedDocument, PropertySpan, SourceLine,
    TimestampSpan, WorkspaceIndex, parse_timestamp,
};
pub use error::OrgModeError;
pub use imports::{
    ImportDiagnostic, ImportSnapshot, ImportStatus, PortableEntry, ResolvedEditorConfig,
    SnapshotMetadata,
};
pub use model::{
    AGENDA_ENTRY_SCHEMA, AgendaEntry, AgendumError, ERROR_SCHEMA, ErrorResponse, ITEM_SCHEMA, Item,
    MUTATION_SCHEMA, Mutation, Occurrence, OccurrenceType, Planning, PlanningTimestamp,
};
pub use org_mode::{
    AgendaItem, AgendaView, CaptureEntry, CaptureResult, ClearField, OrgMode, Priority,
    PropertyPair, TodoState, UpdateEntry, UpdateResult,
};
pub use paths::RootSet;
pub use settings::{
    AgendaSettings, AgentProfile, AgentSettings, CaptureTemplate, CommandProfile,
    EMACS_IMPORT_SCHEMA, EditorImportSettings, EditorSettings, FilesSettings, IdPolicy,
    ImportSettings, LoadedSettings, NVIM_IMPORT_SCHEMA, Settings, TagsSettings, TodoSettings,
    TriageSettings, load_settings,
    resolve_config_path,
};
pub use write::{
    Change, EditIntent, JournalRecord, JournalStatus, Patch, RefileResult, StoredPatch, UndoResult,
    WriteContext, WriteEngine, apply_patches,
};
