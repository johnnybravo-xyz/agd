//! Changing org files: [`WriteEngine`] edits items, alone or as a batch,
//! captures through templates, refiles subtrees and undoes its own changes,
//! journaling every write so it can be reverted.
//!
//! Org files are user data: a write replaces only the bytes it changes and
//! every other byte stays exactly as it was, or the write is refused and
//! nothing is written. Each write is computed again under its file's lock,
//! refuses a file an editor holds unsaved (an Emacs `.#` lock), replaces the
//! file atomically, and is journaled before and after, so an interrupted write
//! is settled from the file's bytes. A dry run computes the same diff and
//! touches nothing.

mod capture;
mod edit;
mod engine;
mod file;
mod journal;
mod org_edit;
mod patch;
mod placement;
mod refile;
#[cfg(test)]
mod test_support;
mod undo;
mod validation;

pub use edit::{BatchMutation, Change, EditIntent, ItemEdit};
pub use engine::{WriteContext, WriteEngine, actor_name, agent_actor, agent_name};
pub use journal::{JournalOperation, JournalRecord, JournalStatus};
pub(crate) use patch::line_ending;
pub use patch::{Patch, StoredPatch, apply_patches};
pub(crate) use placement::{CaptureTarget, find_olpath, heading_titled};
pub use refile::RefileResult;
pub use undo::UndoResult;
