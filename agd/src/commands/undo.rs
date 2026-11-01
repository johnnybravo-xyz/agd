use std::path::Path;

use agd_core::{
    AgdError, ErrorCode, JournalRecord, RootSet, Settings, UndoResult, WorkspaceIndex,
    WriteContext, WriteEngine, load_settings,
};
use anyhow::Result;
use clap::Args;

use super::{cli_context, write_stdout};
use crate::words;

#[derive(Args, Debug)]
pub struct UndoCommand {
    /// Print the agd.undo/v1 result
    #[arg(long)]
    pub json: bool,
}

impl UndoCommand {
    /// Reverts your newest change still in effect, from any surface (the
    /// TUI, this command line, a mod), never an agent's, and only where the
    /// file still holds what it wrote. Says what it undid as the TUI does:
    /// `inbox.org:2  undid capture · Buy milk`.
    pub fn execute(self, config: Option<String>, root: Option<String>) -> Result<()> {
        let settings = load_settings(
            config.as_deref().map(Path::new),
            root.as_deref().map(Path::new),
        )?;
        let Some((result, sentence)) = undo_latest(&settings, &cli_context())? else {
            return Err(AgdError::new(
                ErrorCode::JournalNotFound,
                "nothing to undo: none of your changes is still in effect",
            )
            .into());
        };
        let output = if self.json {
            format!("{}\n", serde_json::to_string_pretty(&result)?)
        } else {
            format!("{sentence}\n")
        };
        Ok(write_stdout(&output)?)
    }
}

/// Undoes your newest change (WriteEngine::latest_own_change) and says
/// what it undid; None when there is none. The TUI's `u` and `agd undo`
/// share it.
pub fn undo_latest(
    settings: &Settings,
    context: &WriteContext,
) -> Result<Option<(UndoResult, String)>> {
    let engine = WriteEngine::with_default_journal(settings.clone())?;
    let Some(record) = engine.latest_own_change()? else {
        return Ok(None);
    };
    // Named before the undo, while the file still holds the task.
    let sentence = undone_sentence(&record, settings);
    let result = engine.undo(&record.id, context)?;
    Ok(Some((result, sentence)))
}

/// `inbox.org:2  undid capture · Buy milk`: the task as the files read it
/// now, else as the change wrote it.
pub fn undone_sentence(record: &JournalRecord, settings: &Settings) -> String {
    let roots = RootSet::new(&settings.roots, &settings.agenda_files)
        .map(|roots| roots.roots().to_vec())
        .unwrap_or_default();
    let item = WorkspaceIndex::load(settings.clone())
        .ok()
        .and_then(|index| {
            index
                .items()
                .find(|item| record.item_ref.as_deref() == Some(item.item_ref.as_str()))
                .cloned()
        });
    let change = format!("undid {}", record.operation_label());
    let (title, location) = match &item {
        Some(item) => (
            item.title.clone(),
            words::location(&item.file, item.line, &roots),
        ),
        None => {
            let line = record
                .item_ref
                .as_deref()
                .and_then(|item_ref| item_ref.split_once("#sha256:"))
                .and_then(|(place, _)| place.rsplit_once(':'))
                .and_then(|(_, line)| line.parse().ok())
                .unwrap_or(1);
            let file = record.file.split(',').next().unwrap_or_default();
            // The last headline the change added.
            let heading = record
                .diff
                .lines()
                .rev()
                .filter(|line| !line.starts_with("+++"))
                .filter_map(|line| line.strip_prefix('+'))
                .find(|line| line.starts_with('*'))
                .unwrap_or_default();
            (
                words::headline_title(heading),
                words::location(file, line, &roots),
            )
        }
    };
    words::change(&location, &change, &title)
}
