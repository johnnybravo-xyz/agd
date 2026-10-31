use std::collections::{BTreeMap, BTreeSet};
use std::io::IsTerminal;
use std::path::Path;

use agd_core::{
    AgdError, Change, EditIntent, ErrorCode, RootSet, WorkspaceIndex, WriteEngine, load_settings,
    parse_date_change,
};
use anyhow::Result;
use chrono::Local;
use clap::Args;

use super::write_stdout;
use crate::words;

#[derive(Args, Debug)]
pub struct EditCommand {
    /// Item ref returned by agenda, query, capture, or the last edit
    #[arg(value_name = "REF")]
    item_ref: String,
    /// Replace the heading title
    #[arg(long)]
    title: Option<String>,
    /// Replace the plain-text description; an empty value removes it
    #[arg(long)]
    description: Option<String>,
    /// Set a TODO keyword from this file's workflow
    #[arg(long, value_name = "KEYWORD", conflicts_with = "done")]
    todo: Option<String>,
    /// Use the first done keyword in this item's workflow
    #[arg(long)]
    done: bool,
    /// Set a scheduled date, or `none` to remove it
    #[arg(long, allow_hyphen_values = true)]
    scheduled: Option<String>,
    /// Set a deadline, or `none` to remove it
    #[arg(long, allow_hyphen_values = true)]
    deadline: Option<String>,
    /// Set a priority letter, or `none` to remove it
    #[arg(long)]
    priority: Option<String>,
    /// Add a tag without colons
    #[arg(long = "add-tag", value_name = "TAG")]
    tags_add: Vec<String>,
    /// Remove a tag without colons
    #[arg(long = "remove-tag", value_name = "TAG")]
    tags_remove: Vec<String>,
    /// Set an Org property
    #[arg(long = "property", value_name = "KEY=VALUE")]
    properties: Vec<String>,
    /// Remove an Org property
    #[arg(long = "remove-property", value_name = "KEY")]
    properties_remove: Vec<String>,
    /// Add a timestamped note; the text may contain newlines
    #[arg(long)]
    note: Option<String>,
    /// Print the exact diff without writing
    #[arg(long)]
    dry_run: bool,
    /// Print the agd.mutation/v1 result, with `location`, the terminal's
    /// line as `summary` and any `notes`
    #[arg(long)]
    pub json: bool,
}

impl EditCommand {
    pub fn execute(self, config: Option<String>, root: Option<String>) -> Result<()> {
        let settings = load_settings(
            config.as_deref().map(Path::new),
            root.as_deref().map(Path::new),
        )?;
        // The task's state before the edit names the change, `TODO → DONE`.
        let index = WorkspaceIndex::load(settings.clone())?;
        let (document, headline) = index.resolve(&self.item_ref)?;
        let from = headline.item.todo.clone();
        let state = if self.done {
            let done = document
                .sequence(from.as_deref())
                .into_iter()
                .find(|keyword| keyword.done)
                .ok_or_else(|| {
                    AgdError::new(
                        ErrorCode::InvalidEdit,
                        "this file's workflow has no done state",
                    )
                })?;
            Change::Set(done.name.clone())
        } else {
            self.todo.map(Change::Set).unwrap_or_default()
        };
        let today = Local::now().date_naive();
        let changes_planning = self.scheduled.is_some() || self.deadline.is_some();
        let new_deadline = self.deadline.is_some();
        let intent = EditIntent {
            state,
            title: self.title.map(Change::Set).unwrap_or_default(),
            description: self.description.map_or(Change::Keep, |description| {
                if description.is_empty() {
                    Change::Clear
                } else {
                    Change::Set(description)
                }
            }),
            priority: self.priority.map_or(Change::Keep, |priority| {
                if priority == "none" {
                    Change::Clear
                } else {
                    Change::Set(priority)
                }
            }),
            tags_add: self.tags_add.into_iter().collect::<BTreeSet<_>>(),
            tags_remove: self.tags_remove.into_iter().collect::<BTreeSet<_>>(),
            scheduled: planning_change(self.scheduled.as_deref(), today)?,
            deadline: planning_change(self.deadline.as_deref(), today)?,
            properties: properties(self.properties, self.properties_remove)?,
            note: self.note,
            ..EditIntent::default()
        };
        let mutation = WriteEngine::with_default_journal(settings.clone())?.edit(
            &self.item_ref,
            &intent,
            self.dry_run,
            &super::cli_context(),
        )?;
        let mut notes = Vec::new();
        if changes_planning
            && let (Some(scheduled), Some(deadline)) = (
                &mutation.item.planning.scheduled,
                &mutation.item.planning.deadline,
            )
            && let Some(mut clash) = agd_core::deadline_before_scheduled(scheduled, deadline, today)
        {
            if !new_deadline {
                clash.instead = None;
            }
            notes.push(words::deadline_before_scheduled(
                &clash,
                "--deadline ",
                today,
            ));
        }
        let roots = RootSet::new(&settings.roots, &settings.agenda_files)
            .map(|roots| roots.roots().to_vec())
            .unwrap_or_default();
        let location = words::location(&mutation.item.file, mutation.item.line, &roots);
        let change = match (&intent.state, &mutation.repeated) {
            (Change::Set(_), Some(repeat)) => {
                format!("repeated → {}", words::when(&repeat.next_date, None, today))
            }
            (Change::Set(to), None) => format!("{} → {to}", from.as_deref().unwrap_or("—")),
            _ => "edited".to_string(),
        };
        let summary = words::change(&location, &change, &mutation.item.title);
        let output = if self.json {
            let mut json = serde_json::to_value(&mutation)?;
            json["location"] = location.into();
            json["summary"] = summary.into();
            json["notes"] = notes.into();
            format!("{}\n", serde_json::to_string_pretty(&json)?)
        } else {
            // Notes go to stderr, so stdout stays what scripts read.
            for note in &notes {
                eprintln!("note: {note}");
            }
            if self.dry_run {
                mutation.diff
            } else if std::io::stdout().is_terminal() {
                format!("{summary}\n")
            } else {
                format!("{}\n", mutation.item.item_ref)
            }
        };
        Ok(write_stdout(&output)?)
    }
}

fn planning_change(
    value: Option<&str>,
    today: chrono::NaiveDate,
) -> Result<Change<agd_core::PlanningTimestamp>, AgdError> {
    value.map_or(Ok(Change::Keep), |value| parse_date_change(value, today))
}

fn properties(
    set: Vec<String>,
    remove: Vec<String>,
) -> Result<BTreeMap<String, Option<String>>, AgdError> {
    let mut properties = BTreeMap::new();
    for value in set {
        let (key, value) = value.split_once('=').ok_or_else(|| {
            AgdError::new(
                ErrorCode::InvalidEdit,
                format!("--property must be KEY=VALUE: {value}"),
            )
        })?;
        if key.is_empty() {
            return Err(AgdError::new(
                ErrorCode::InvalidEdit,
                "--property key cannot be empty",
            ));
        }
        properties.insert(key.to_string(), Some(value.to_string()));
    }
    for key in remove {
        properties.insert(key, None);
    }
    Ok(properties)
}
