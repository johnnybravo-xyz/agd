use std::path::Path;

use agd_core::{
    AgdError, ErrorCode, JournalRecord, JournalStatus, RootSet, WorkspaceIndex, WriteEngine,
    load_settings,
};
use anyhow::Result;
use clap::Args;

use super::{CLI_ACTOR, cli_context, write_stdout};
use crate::words;

#[derive(Args, Debug)]
pub struct UndoCommand {
    /// Print the agd.undo/v1 result
    #[arg(long)]
    json: bool,
}

impl UndoCommand {
    /// True when the output, and so any error, is JSON.
    pub fn json(&self) -> bool {
        self.json
    }

    /// Reverts the newest change made from the command line that is still
    /// in effect, never another actor's (the TUI's, an agent's), and only
    /// where the file still holds what it wrote. Says what it undid as the
    /// TUI does: `inbox.org:2  undid capture · Buy milk`.
    pub fn execute(self, config: Option<String>, root: Option<String>) -> Result<()> {
        let settings = load_settings(
            config.as_deref().map(Path::new),
            root.as_deref().map(Path::new),
        )?
        .settings;
        let engine = WriteEngine::with_default_journal(settings.clone())?;
        // Newest first.
        let journal = engine.journal()?;
        let Some(record) = journal.iter().find(|record| {
            record.actor == CLI_ACTOR
                && matches!(
                    record.status,
                    JournalStatus::Committed | JournalStatus::Reviewed
                )
        }) else {
            return Err(AgdError::new(
                ErrorCode::JournalNotFound,
                "nothing to undo: no change made from the command line is still in effect",
            )
            .into());
        };
        // Named before the undo, while the file still holds the task.
        let sentence = undone_sentence(record, &settings);
        let result = engine.undo(&record.id, &cli_context())?;
        let output = if self.json {
            format!("{}\n", serde_json::to_string_pretty(&result)?)
        } else {
            format!("{sentence}\n")
        };
        Ok(write_stdout(&output)?)
    }
}

/// `inbox.org:2  undid capture · Buy milk`: the task as the files read it
/// now, else as the change wrote it.
fn undone_sentence(record: &JournalRecord, settings: &agd_core::Settings) -> String {
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
    let added = |sign: char| {
        record
            .diff
            .lines()
            .filter(move |line| !line.starts_with("+++") && !line.starts_with("---"))
            .filter_map(move |line| line.strip_prefix(sign))
            .filter(|line| line.starts_with('*'))
            .collect::<Vec<_>>()
    };
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
            let heading = added('+').last().copied().unwrap_or_default();
            (
                words::headline_title(heading),
                words::location(file, line, &roots),
            )
        }
    };
    words::change(&location, &change, &title)
}
