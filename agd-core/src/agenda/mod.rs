//! Reading the agenda from a [`WorkspaceIndex`](crate::WorkspaceIndex): agenda
//! days, TODO lists, tags matches, org-ql queries and saved views as org-agenda
//! shows them, and their Emacs text and CSV exports.
//!
//! Nothing here writes; changes go through [`crate::write`]. Results are
//! deterministic: rows sort by the configured org-agenda sorting strategy and
//! otherwise keep file and buffer order, checked against pinned Emacs in
//! `conformance/agenda`.

mod color;
mod engine;
mod export;
mod matcher;
mod query;
mod repeat;
mod sort;
mod span;
#[cfg(test)]
mod test_support;
mod view;

pub use color::{keyword_color, keyword_colors};
pub use engine::{AgendaEngine, AgendaQuery};
pub use export::{agenda_csv, agenda_row, agenda_text, agenda_title, item_row, items_csv};
pub(crate) use repeat::{add_period, parse_repeater};
pub use span::AgendaSpan;
pub(crate) use view::duration_minutes;
pub use view::{ViewSection, ViewSectionGroup, validate_view_block};
