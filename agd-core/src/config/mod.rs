//! The configuration: [`Settings`] and the records in `config.toml`, reading
//! the file ([`load_settings`]), writing it back ([`render_config`]), and
//! checking every value before anything uses it.
//!
//! `config.toml` is one flat table. Settings are top-level snake_case keys,
//! and only `[[capture]]`, `[[view]]` and `[[agent_profile]]` are lists of
//! records. Values are lowercase, kebab-case when they name an Org option
//! (`add-on-write`, `time-up`, `tags-todo`). Unknown keys are errors, and each
//! problem is an `invalid_config` error that names the key and how to fix it. A file's
//! own Org syntax overrides these settings for that file; [`crate::document`]
//! reads it.

mod file;
mod model;
#[cfg(test)]
mod test_support;
mod validation;

pub use file::{default_config_path, load_settings, parse_config_file, render_config};
pub use model::{
    AgentProfile, COLOR_NAMES, CaptureTemplate, IdPolicy, LogMode, Notation, SORTING_KEYS,
    Settings, VIEW_BLOCK_KINDS, VIEW_GROUP_BY, ViewBlock, ViewGroup, ViewSettings, WeekStart,
    parse_agenda_day,
};
pub(crate) use validation::validate_values;
