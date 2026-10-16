//! Every write the TUI makes, and the prompts and pickers that lead to one:
//! state changes, dates, priority, tags, someday, refile, undo, accepting
//! agent changes and copying a link. A write acts on the marked rows, else
//! the cursor row, as one guarded transaction; a prompt previews the exact
//! change with a dry run first; each write is confirmed on the status line
//! as `words::change` says it and can be undone with `u`.

use std::collections::BTreeSet;
use std::path::Path;

use agd_core::{
    AgdError, Change, EditIntent, ErrorCode, Item, ItemEdit, JournalStatus, LogKind,
    PlanningTimestamp, PriorityRange, WriteEngine, parse_date_input,
};
use anyhow::{Result, bail};
use chrono::{Duration, NaiveDate};

use super::input::TextInput;
use super::{App, Overlay, PromptKind, Screen, write_context};
use crate::words;

/// Triage outcomes, counted for the summary line.
pub(super) const TODAY: usize = 0;
pub(super) const TOMORROW: usize = 1;
pub(super) const NEXT_WEEK: usize = 2;
pub(super) const SOMEDAY: usize = 3;
pub(super) const DONE: usize = 4;
pub(super) const REFILED: usize = 5;
pub(super) const OTHER: usize = 6;

/// A write this TUI made, undone as a unit by `u`: a journal entry or
/// transaction, where it was, and what to call it on the status line
/// (`scheduled Tue 20 Oct · Pay rent`).
#[derive(Debug)]
pub(super) struct UndoStep {
    journal_id: String,
    location: String,
    label: String,
}

/// One triage decision, undone as a unit: its journal entry or transaction
/// and the outcome it counted.
#[derive(Debug)]
pub(super) struct TriageStep {
    journal_id: String,
    outcome: usize,
}

/// A one-line prompt over the cursor row or the marked rows, with a dry
/// run of exactly what Enter writes.
#[derive(Debug)]
pub(super) struct Prompt {
    pub kind: PromptKind,
    pub input: TextInput,
    /// For a Note prompt, the TODO state the note goes with.
    pub state: String,
    /// For the filter, which applies as you type: what esc puts back.
    pub restore: String,
    /// The changed lines Enter would write (and a count for marked rows),
    /// or why it would write nothing, as a sentence.
    pub preview: Result<Vec<String>, String>,
    /// The preview is out of date; it is rebuilt once before the next draw.
    pub stale: bool,
}

impl Prompt {
    /// A prompt holding `input`, selected so typing replaces it.
    pub(super) fn new(kind: PromptKind, input: String) -> Self {
        let mut input = TextInput::new(input);
        input.select_all();
        Self {
            kind,
            input,
            state: String::new(),
            restore: String::new(),
            preview: Ok(Vec::new()),
            stale: true,
        }
    }

    /// `/`: the filter as it stands, which esc restores.
    pub(super) fn filter(current: &str) -> Self {
        Self {
            restore: current.to_string(),
            ..Self::new(PromptKind::Filter, current.to_string())
        }
    }

    /// The note for a change to `state`, asked before anything is written.
    fn note(state: &str) -> Self {
        Self {
            state: state.to_string(),
            ..Self::new(PromptKind::Note, String::new())
        }
    }
}

/// What a prompt writes: the edit, its triage outcome, the status label,
/// and the sentence shown when it would change nothing.
struct PromptEdit {
    intent: EditIntent,
    outcome: usize,
    label: String,
    unchanged: String,
}

/// Rebuilds a stale prompt preview with one dry run per target, as for the
/// edit popup.
pub(super) fn refresh_stale_prompt(app: &mut App) {
    let Overlay::Prompt(prompt) = &app.overlay else {
        return;
    };
    if !prompt.stale {
        return;
    }
    let preview = app.prompt_preview(prompt);
    if let Overlay::Prompt(prompt) = &mut app.overlay {
        prompt.preview = preview;
        prompt.stale = false;
    }
}

/// The DEADLINE edit for an org date, with an optional warning (`-3d`).
fn deadline_edit(value: &str, today: NaiveDate) -> Result<PromptEdit, String> {
    let date = parse_date_input(value, today).map_err(|error| error.to_string())?;
    let when = when(&date, today);
    Ok(PromptEdit {
        intent: EditIntent {
            deadline: Change::Set(date),
            ..EditIntent::default()
        },
        outcome: OTHER,
        label: format!("deadline {when}"),
        unchanged: format!("the deadline is already {when}"),
    })
}

/// `+tag -tag` (also `tag`, `:tag:` or commas); None until a tag is typed.
fn tags_edit(input: &str) -> Option<PromptEdit> {
    let mut intent = EditIntent::default();
    let mut words = Vec::new();
    for word in input.split([',', ' ', ':']) {
        let (remove, tag) = match word.strip_prefix('-') {
            Some(tag) => (true, tag),
            None => (false, word.trim_start_matches('+')),
        };
        if tag.is_empty() {
            continue;
        }
        if remove {
            intent.tags_remove.insert(tag.to_string());
            words.push(format!("-{tag}"));
        } else {
            intent.tags_add.insert(tag.to_string());
            words.push(format!("+{tag}"));
        }
    }
    if words.is_empty() {
        return None;
    }
    Some(PromptEdit {
        intent,
        outcome: OTHER,
        label: format!("tags {}", words.join(" ")),
        unchanged: "the tags already read that way".to_string(),
    })
}

/// A planning date as people say it (words::when): `Fri 16 Oct 14:00`.
pub(super) fn when(timestamp: &PlanningTimestamp, today: NaiveDate) -> String {
    words::when(&timestamp.date, timestamp.time.as_deref(), today)
}

/// A date two weeks out, for prompt examples that never go stale.
pub(super) fn example_date(today: NaiveDate) -> String {
    (today + Duration::days(14)).format("%Y-%m-%d").to_string()
}

/// Fuzzy refile target picker over every heading, recent targets first.
#[derive(Debug)]
pub(super) struct RefilePicker {
    pub input: TextInput,
    pub selected: usize,
    targets: Vec<(String, String)>,
}

impl RefilePicker {
    /// (item ref, `path › outline`) pairs matching the typed text.
    pub(super) fn matches(&self) -> Vec<&(String, String)> {
        self.targets
            .iter()
            .filter(|(_, label)| fuzzy_match(label, self.input.text()))
            .collect()
    }
}

impl App {
    /// Applies one edit to every target (marked rows, else the selected
    /// row) as one transaction: if any target is refused, nothing is
    /// written. Marked rows that already read that way are left out, as the
    /// prompt's preview says. Confirms it on the status line the way
    /// org-agenda does, naming the targets:
    /// `work.org:142  TODO → DONE · Pay rent · u undo`.
    fn edit_targets(&mut self, intent: EditIntent, outcome: usize, change: &str) -> Result<()> {
        let engine = WriteEngine::with_default_journal(self.settings.clone())?;
        let targets = self.targets();
        if targets.is_empty() {
            return Ok(());
        }
        let mut edits = Vec::new();
        for item_ref in targets.iter().cloned() {
            let unchanged = targets.len() > 1
                && matches!(
                    engine.edit(&item_ref, &intent, true, &write_context()),
                    Err(error) if error.code() == "no_change"
                );
            if !unchanged {
                edits.push(ItemEdit {
                    item_ref,
                    intent: intent.clone(),
                });
            }
        }
        if edits.is_empty() {
            return Err(AgdError::new("no_change", "edit does not change the items").into());
        }
        let batch = engine.edit_batch(&edits, false, &write_context())?;
        let files: BTreeSet<String> = batch
            .mutations
            .iter()
            .map(|mutation| mutation.item.file.clone())
            .collect();
        for file in files {
            self.index.refresh_path(Path::new(&file))?;
        }
        self.record_triage(&batch.transaction, outcome);
        self.marks.clear();
        let items: Vec<&Item> = batch
            .mutations
            .iter()
            .map(|mutation| &mutation.item)
            .collect();
        let location = self.targets_location(&items);
        // A repeating task that was marked done moved on instead.
        let change = match batch.mutations.as_slice() {
            [single] => single.repeated.as_ref().map_or_else(
                || change.to_string(),
                |repeat| {
                    format!(
                        "repeated → {}",
                        words::when(&repeat.next_date, None, self.today)
                    )
                },
            ),
            _ => change.to_string(),
        };
        let summary = targets_summary(&items);
        self.set_status(format!(
            "{} · u undo",
            words::change(&location, &change, &summary)
        ));
        self.remember_undo(
            &batch.transaction,
            &location,
            format!("{change} · {summary}"),
        );
        self.reload_rows()
    }

    /// `work.org:142` for one target, `3 items` for several.
    fn targets_location(&self, items: &[&Item]) -> String {
        match items {
            [item] => self.short_location(item),
            _ => format!("{} items", items.len()),
        }
    }

    /// Sets one TODO state on every target. When the file's workflow logs
    /// the change with a note (`WAIT(w@)`), the note is asked for first, as
    /// Org does, and nothing is written until it is given.
    pub(super) fn set_state(&mut self, state: &str) -> Result<()> {
        if self
            .targets()
            .iter()
            .any(|target| self.wants_note(target, state))
        {
            self.overlay = Overlay::Prompt(Prompt::note(state));
            return Ok(());
        }
        let edit = self.state_edit(state, None);
        self.apply(edit)
    }

    /// The state change, with the note when one was asked for. The status
    /// names the change from the target's own state, which with marks is
    /// not the cursor row's.
    fn state_edit(&self, state: &str, note: Option<String>) -> PromptEdit {
        let targets = self.targets();
        let label = match targets.as_slice() {
            [only] => {
                let from = self
                    .index
                    .resolve(only)
                    .ok()
                    .and_then(|(_, headline)| headline.item.todo.clone())
                    .unwrap_or_else(|| "·".to_string());
                format!("{from} → {state}")
            }
            _ => format!("→ {state}"),
        };
        let outcome = if self.is_done_keyword(state) {
            DONE
        } else {
            OTHER
        };
        PromptEdit {
            intent: EditIntent {
                state: Change::Set(state.to_string()),
                note,
                ..EditIntent::default()
            },
            outcome,
            label,
            unchanged: format!("already {state}"),
        }
    }

    /// Whether Org would ask for a note when `state` is set on `target`:
    /// the new keyword's `@`, else the old one's `@` on leaving it; with no
    /// keyword marker, `lognotedone` for done states (`lognoterepeat` when
    /// the task repeats). The core writes the note into that log entry.
    fn wants_note(&self, target: &str, state: &str) -> bool {
        let Ok((document, headline)) = self.index.resolve(target) else {
            return false;
        };
        let current = headline.item.todo.as_deref();
        if current == Some(state) {
            return false;
        }
        let keyword = document.todo_keyword(state);
        let marker = keyword.and_then(|keyword| keyword.log_enter).or_else(|| {
            current
                .and_then(|current| document.todo_keyword(current))
                .and_then(|keyword| keyword.log_leave)
        });
        let planning = &headline.item.planning;
        let repeats = [&planning.scheduled, &planning.deadline]
            .into_iter()
            .flatten()
            .any(|timestamp| timestamp.repeater.is_some());
        let done = keyword.is_some_and(|keyword| keyword.done);
        let logging = document.logging();
        let kind = if done && repeats {
            logging.repeat
        } else if marker.is_some() || !done {
            marker
        } else {
            logging.done
        };
        kind == Some(LogKind::Note)
    }

    /// `x`: the first done keyword of the item's own sequence.
    pub(super) fn mark_done(&mut self) -> Result<()> {
        let state = self.selected_keywords().into_iter().find(|(_, done)| *done);
        match state {
            Some((state, _)) => self.set_state(&state),
            None => bail!("no done state is configured for this file"),
        }
    }

    /// `t`: next keyword in the item's sequence, wrapping, like org-todo.
    pub(super) fn cycle_state(&mut self) -> Result<()> {
        let keywords = self.selected_keywords();
        let Some(current) = self.selected_row().map(|row| row.item.todo.clone()) else {
            return Ok(());
        };
        let position = keywords
            .iter()
            .position(|(keyword, _)| Some(keyword) == current.as_ref());
        let next = match position {
            Some(index) if index + 1 < keywords.len() => keywords[index + 1].0.clone(),
            Some(_) | None => keywords
                .first()
                .map(|(keyword, _)| keyword.clone())
                .unwrap_or_default(),
        };
        if next.is_empty() {
            return Ok(());
        }
        self.set_state(&next)
    }

    /// `X`: the last done keyword of the item's own sequence, as in
    /// `TODO | DONE CANCELLED` or `TODO | FINISHED DROPPED`. With a single
    /// done keyword the workflow has no cancel state, and `x` is done.
    pub(super) fn cancel(&mut self) -> Result<()> {
        let done: Vec<String> = self
            .selected_keywords()
            .into_iter()
            .filter(|(_, done)| *done)
            .map(|(keyword, _)| keyword)
            .collect();
        match done.as_slice() {
            [_, .., last] => {
                let last = last.clone();
                self.set_state(&last)
            }
            [only] => bail!("this file's workflow has no cancel state, only {only} (x)"),
            [] => bail!("this file's workflow has no done state"),
        }
    }

    /// (keyword, done) for the selected row's TODO sequence in its file.
    pub(super) fn selected_keywords(&self) -> Vec<(String, bool)> {
        self.selected_row()
            .map(|row| self.keywords_of(&row.item))
            .unwrap_or_default()
    }

    /// (keyword, done) for `item`'s TODO sequence in its file.
    pub(super) fn keywords_of(&self, item: &Item) -> Vec<(String, bool)> {
        let Some(document) = self.index.document(Path::new(&item.file)) else {
            return Vec::new();
        };
        let sequence = item
            .todo
            .as_deref()
            .and_then(|todo| document.todo_keyword(todo))
            .map_or(0, |keyword| keyword.sequence);
        document
            .todo_keywords()
            .iter()
            .filter(|keyword| keyword.sequence == sequence)
            .map(|keyword| (keyword.name.clone(), keyword.done))
            .collect()
    }

    fn is_done_keyword(&self, state: &str) -> bool {
        self.selected_row()
            .and_then(|row| self.index.document(Path::new(&row.item.file)))
            .and_then(|document| document.todo_keyword(state))
            .is_some_and(|keyword| keyword.done)
    }

    /// Reschedules to an org date (`today`, `+1d`, `fri`, `YYYY-MM-DD`).
    /// A refusal is a sentence, such as the core's date error.
    pub(super) fn schedule(&mut self, value: &str) -> Result<()> {
        let edit = self.schedule_edit(value).map_err(anyhow::Error::msg)?;
        self.apply(edit)
    }

    /// Writes a prompt's edit; a change to nothing says what is already so.
    fn apply(&mut self, edit: PromptEdit) -> Result<()> {
        let unchanged = edit.unchanged;
        self.edit_targets(edit.intent, edit.outcome, &edit.label)
            .map_err(|error| match error.downcast_ref::<AgdError>() {
                Some(refused) if refused.code() == "no_change" => anyhow::Error::msg(unchanged),
                _ => error,
            })
    }

    /// The SCHEDULED edit for an org date, counted for triage by how far
    /// off it is.
    fn schedule_edit(&self, value: &str) -> Result<PromptEdit, String> {
        let date = parse_date_input(value, self.today).map_err(|error| error.to_string())?;
        let planned = date
            .date
            .parse::<NaiveDate>()
            .map_err(|error| error.to_string())?;
        let outcome = if planned <= self.today {
            TODAY
        } else if planned == self.today + Duration::days(1) {
            TOMORROW
        } else if planned <= self.today + Duration::days(7) {
            NEXT_WEEK
        } else {
            OTHER
        };
        let when = when(&date, self.today);
        Ok(PromptEdit {
            intent: EditIntent {
                scheduled: Change::Set(date),
                ..EditIntent::default()
            },
            outcome,
            label: format!("scheduled {when}"),
            unchanged: format!("already scheduled for {when}"),
        })
    }

    /// Someday: add the someday tag and, unless configured otherwise, drop
    /// the date that made it show up (SCHEDULED, or an overdue DEADLINE), so
    /// the task leaves the agenda without being deleted. Marked rows share
    /// one edit, so they keep their deadlines: only the cursor row is known
    /// to be overdue.
    pub(super) fn someday(&mut self) -> Result<()> {
        let tag = self.settings.someday_tag.clone();
        let clears = self.settings.someday_clears_scheduled;
        let overdue_deadline = self.marks.is_empty()
            && self.selected_row().is_some_and(|row| {
                row.occurrence
                    .as_ref()
                    .is_some_and(|occurrence| occurrence.extra.ends_with("ago:"))
            });
        self.edit_targets(
            EditIntent {
                scheduled: if clears { Change::Clear } else { Change::Keep },
                deadline: if clears && overdue_deadline {
                    Change::Clear
                } else {
                    Change::Keep
                },
                tags_add: BTreeSet::from([tag.clone()]),
                ..EditIntent::default()
            },
            SOMEDAY,
            &format!("→ :{tag}:"),
        )
    }

    /// `+`/`-`: raise or lower priority within the configured range, as
    /// org-agenda's priority-up and priority-down. Each row would step from
    /// its own priority, which one shared edit cannot do, so marked rows
    /// take `,` instead.
    pub(super) fn shift_priority(&mut self, up: bool) -> Result<()> {
        if !self.marks.is_empty() {
            bail!("+ and - change one row; , sets a priority on every marked row");
        }
        let Some(row) = self.selected_row() else {
            return Ok(());
        };
        let range = self.priority_range(&row.item);
        let current = row
            .item
            .priority
            .as_deref()
            .and_then(|value| value.chars().next())
            .unwrap_or(range.default);
        let next = if up {
            (current as u8).saturating_sub(1).max(range.highest as u8) as char
        } else {
            (current as u8 + 1).min(range.lowest as u8) as char
        };
        self.edit_targets(
            EditIntent {
                priority: Change::Set(next.to_string()),
                ..EditIntent::default()
            },
            OTHER,
            &format!("priority {current} → {next}"),
        )
    }

    /// The priority range of the item's file (`#+PRIORITIES`, else the
    /// configured one).
    pub(super) fn priority_range(&self, item: &Item) -> PriorityRange {
        self.index.document(Path::new(&item.file)).map_or_else(
            || PriorityRange::from_settings(&self.settings),
            |document| document.priorities(),
        )
    }

    /// The range a priority prompt offers: the cursor row's file's.
    pub(super) fn prompt_priority_range(&self) -> PriorityRange {
        self.selected_row().map_or_else(
            || PriorityRange::from_settings(&self.settings),
            |row| self.priority_range(&row.item),
        )
    }

    /// The edit a prompt's text asks for; None while there is nothing to
    /// write (no date or tag typed yet, or the filter).
    fn prompt_edit(&self, prompt: &Prompt) -> Result<Option<PromptEdit>, String> {
        let input = prompt.input.text().trim();
        match prompt.kind {
            PromptKind::Filter => Ok(None),
            PromptKind::Schedule | PromptKind::Deadline if input.is_empty() => Ok(None),
            PromptKind::Schedule => self.schedule_edit(input).map(Some),
            PromptKind::Deadline => deadline_edit(input, self.today).map(Some),
            PromptKind::Priority => self.priority_edit(input).map(Some),
            PromptKind::Tags => Ok(tags_edit(input)),
            // An empty note logs the change without one, as Org does.
            PromptKind::Note => {
                let note = (!input.is_empty()).then(|| input.to_string());
                Ok(Some(self.state_edit(&prompt.state, note)))
            }
        }
    }

    /// A priority in the file's range, or empty to clear it.
    fn priority_edit(&self, input: &str) -> Result<PromptEdit, String> {
        let range = self.prompt_priority_range();
        if input.is_empty() {
            return Ok(PromptEdit {
                intent: EditIntent {
                    priority: Change::Clear,
                    ..EditIntent::default()
                },
                outcome: OTHER,
                label: "priority cleared".to_string(),
                unchanged: "it has no priority".to_string(),
            });
        }
        let letter = input.to_ascii_uppercase();
        if !range.contains(&letter) {
            return Err(format!(
                "priority is one of {} to {}; empty clears it",
                range.highest, range.lowest
            ));
        }
        Ok(PromptEdit {
            intent: EditIntent {
                priority: Change::Set(letter.clone()),
                ..EditIntent::default()
            },
            outcome: OTHER,
            label: format!("priority {letter}"),
            unchanged: format!("already priority {letter}"),
        })
    }

    /// Dry-runs `edit` on every target: the first changed target's lines,
    /// a count when rows are marked, or why nothing would be written.
    fn dry_run(&self, edit: &PromptEdit) -> Result<Vec<String>, String> {
        let engine = WriteEngine::with_default_journal(self.settings.clone())
            .map_err(|error| words::error_sentence(&error))?;
        let targets = self.targets();
        let mut first = None;
        let mut changing = 0;
        for target in &targets {
            match engine.edit(target, &edit.intent, true, &write_context()) {
                Ok(mutation) => {
                    changing += 1;
                    first.get_or_insert_with(|| super::edit::changed_lines(&mutation.diff));
                }
                Err(error) if error.code() == "no_change" => {}
                Err(error) => return Err(words::error_sentence(&error)),
            }
        }
        let Some(first) = first else {
            return Err(edit.unchanged.clone());
        };
        let mut lines: Vec<String> = first.lines().map(str::to_string).collect();
        if targets.len() > 1 {
            lines.push(format!(
                "{changing} of {} marked rows change",
                targets.len()
            ));
        }
        Ok(lines)
    }

    /// The preview a prompt shows: what Enter would write, or why not.
    pub(super) fn prompt_preview(&self, prompt: &Prompt) -> Result<Vec<String>, String> {
        match self.prompt_edit(prompt)? {
            Some(edit) => self.dry_run(&edit),
            None => Ok(Vec::new()),
        }
    }

    /// Enter in a prompt: writes exactly what the preview shows; a refusal
    /// or a change to nothing keeps the prompt open with the reason.
    pub(super) fn commit_prompt(&mut self) -> Result<()> {
        let Overlay::Prompt(prompt) = &self.overlay else {
            return Ok(());
        };
        if prompt.kind == PromptKind::Filter {
            self.filter = prompt.input.text().trim().to_string();
            self.overlay = Overlay::None;
            return self.reload_rows();
        }
        let checked = self.prompt_edit(prompt).and_then(|edit| match edit {
            Some(edit) => self.dry_run(&edit).map(|_| Some(edit)),
            None => Ok(None),
        });
        match checked {
            Err(reason) => {
                if let Overlay::Prompt(prompt) = &mut self.overlay {
                    prompt.preview = Err(reason);
                    prompt.stale = false;
                }
                Ok(())
            }
            Ok(None) => {
                self.overlay = Overlay::None;
                Ok(())
            }
            Ok(Some(edit)) => {
                self.overlay = Overlay::None;
                self.apply(edit)
            }
        }
    }

    /// A decision in Triage: count it once (a refile across files writes
    /// two journal entries under one id) and move on to the next card.
    fn record_triage(&mut self, journal_id: &str, outcome: usize) {
        if self.screen == Screen::Triage {
            self.triage.outcomes[outcome] += 1;
            self.triage.undo.push(TriageStep {
                journal_id: journal_id.to_string(),
                outcome,
            });
            self.triage.decided.push(self.triage.position);
            self.triage.position += 1;
            self.triage.last_decision = Some(std::time::Instant::now());
        }
    }

    /// Keeps a write this TUI made, so `u` can undo it later; `label`
    /// names it on the status line, such as `scheduled Mon 19 Oct · Pay
    /// rent`, after where it was.
    pub(super) fn remember_undo(&mut self, journal_id: &str, location: &str, label: String) {
        self.undo_steps.push(UndoStep {
            journal_id: journal_id.to_string(),
            location: location.to_string(),
            label,
        });
    }

    /// `u` in the lists and Triage undoes the latest change this TUI made,
    /// never another actor's (agent changes are undone per card on the
    /// Agents tab); at the end of a triage pass it undoes the whole pass.
    pub(super) fn undo(&mut self) -> Result<()> {
        if self.screen == Screen::Triage && !self.triage.undo.is_empty() {
            return self.undo_triage();
        }
        let Some(step) = self.undo_steps.last() else {
            self.set_status("nothing to undo from this session");
            return Ok(());
        };
        let result = WriteEngine::with_default_journal(self.settings.clone())?
            .undo(&step.journal_id, &write_context())?;
        let step = self.undo_steps.pop().expect("the step just undone");
        self.refresh_files(&result.file)?;
        self.set_status(format!("{}  undid {}", step.location, step.label));
        self.reload_rows()
    }

    /// Undoes the last triage decision, or the whole pass once it is
    /// through, newest first.
    fn undo_triage(&mut self) -> Result<()> {
        let engine = WriteEngine::with_default_journal(self.settings.clone())?;
        let undo_all = self.triage.finished();
        let steps = if undo_all {
            std::mem::take(&mut self.triage.undo)
        } else {
            self.triage.undo.pop().into_iter().collect()
        };
        let count = steps.len();
        let mut undone = None;
        for step in steps.into_iter().rev() {
            let result = engine.undo(&step.journal_id, &write_context())?;
            self.refresh_files(&result.file)?;
            self.triage.outcomes[step.outcome] =
                self.triage.outcomes[step.outcome].saturating_sub(1);
            if let Some(index) = self
                .undo_steps
                .iter()
                .position(|undo| undo.journal_id == step.journal_id)
            {
                undone = Some(self.undo_steps.remove(index));
            }
        }
        match undone {
            Some(step) if !undo_all => {
                self.set_status(format!("{}  undid {}", step.location, step.label))
            }
            _ => self.set_status(format!(
                "{}  undid the whole triage pass",
                crate::commands::counted(count, "item", "items")
            )),
        }
        self.reload_rows()
    }

    fn refresh_files(&mut self, files: &str) -> Result<()> {
        for file in files.split(',') {
            self.index.refresh_path(Path::new(file))?;
        }
        Ok(())
    }

    /// `a` on the Agents tab: accepts the marked changes shown, else the
    /// one under the cursor. Accepting writes nothing to the org files: it
    /// only marks each change reviewed in the journal.
    pub(super) fn accept_changes(&mut self) -> Result<()> {
        let chosen: Vec<&agd_core::JournalRecord> = if self.marked_changes.is_empty() {
            self.journals.get(self.selected).into_iter().collect()
        } else {
            self.journals
                .iter()
                .filter(|record| self.marked_changes.contains(&record.id))
                .collect()
        };
        let unreviewed: Vec<&agd_core::JournalRecord> = chosen
            .into_iter()
            .filter(|record| record.status == JournalStatus::Committed)
            .collect();
        let status = match unreviewed.as_slice() {
            [] => {
                self.marked_changes.clear();
                self.set_status("already accepted");
                return Ok(());
            }
            [record] => format!(
                "{} · nothing written",
                words::change(
                    &super::review::subject_location(self, record),
                    "accepted",
                    &super::review::subject_title(self, record),
                )
            ),
            [first, rest @ ..] => format!(
                "{} · nothing written",
                words::change(
                    &crate::commands::counted(rest.len() + 1, "change", "changes"),
                    "accepted",
                    &format!(
                        "{} + {} more",
                        super::review::subject_title(self, first),
                        rest.len()
                    ),
                )
            ),
        };
        let ids: Vec<String> = unreviewed.iter().map(|record| record.id.clone()).collect();
        let engine = WriteEngine::with_default_journal(self.settings.clone())?;
        for id in &ids {
            engine.review(id)?;
        }
        self.marked_changes.clear();
        self.set_status(status);
        self.reload_rows()
    }

    /// Undo applies the journal's reverse patch only where those bytes are
    /// unchanged; otherwise the conflict is reported and nothing is written.
    pub(super) fn undo_selected_change(&mut self) -> Result<()> {
        let Some(record) = self.journals.get(self.selected) else {
            return Ok(());
        };
        let id = record.id.clone();
        let location = super::review::subject_location(self, record);
        let title = super::review::subject_title(self, record);
        let change = format!("undid {}'s change", super::review::agent_name(record));
        let result = WriteEngine::with_default_journal(self.settings.clone())?
            .undo(&id, &write_context())
            .map_err(|error| {
                if error.code() == ErrorCode::UndoConflict.as_str() {
                    AgdError::new(
                        ErrorCode::UndoConflict,
                        format!("{error}: ⏎ here, C-e in $EDITOR"),
                    )
                } else {
                    error
                }
            })?;
        self.refresh_files(&result.file)?;
        self.set_status(words::change(&location, &change, &title));
        self.reload_rows()
    }

    /// Cycles the review list through each agent and back to all.
    pub(super) fn cycle_agent_filter(&mut self) -> Result<()> {
        let journal = WriteEngine::with_default_journal(self.settings.clone())?.journal()?;
        let mut agents: Vec<String> = journal
            .iter()
            .filter_map(|record| record.actor.strip_prefix("mcp:").map(str::to_string))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        agents.insert(0, String::new());
        let current = self.agent_filter.clone().unwrap_or_default();
        let index = agents
            .iter()
            .position(|agent| *agent == current)
            .unwrap_or(0);
        let next = agents[(index + 1) % agents.len()].clone();
        self.agent_filter = (!next.is_empty()).then_some(next);
        self.selected = 0;
        self.reload_rows()
    }

    pub(super) fn open_refile(&mut self) {
        let Some(source) = self.selected_row().map(|row| row.item.item_ref.clone()) else {
            return;
        };
        let mut targets: Vec<(String, String)> = self
            .index
            .items()
            .filter(|item| item.item_ref != source)
            .map(|item| {
                let location = self.short_location(item);
                let file = location.split(':').next().unwrap_or_default().to_string();
                let mut path = item.olpath[..item.olpath.len().saturating_sub(1)].to_vec();
                path.push(super::peek::display_title(item));
                (
                    item.item_ref.clone(),
                    format!("{file} › {}", path.join(" › ")),
                )
            })
            .collect();
        targets.sort_by_key(|(item_ref, label)| {
            (
                self.recent_targets
                    .iter()
                    .position(|recent| recent == item_ref)
                    .unwrap_or(usize::MAX),
                label.to_lowercase(),
            )
        });
        self.overlay = Overlay::Refile(RefilePicker {
            input: TextInput::default(),
            selected: 0,
            targets,
        });
    }

    pub(super) fn commit_refile(&mut self) -> Result<()> {
        let Overlay::Refile(picker) = &self.overlay else {
            return Ok(());
        };
        let Some((target, label)) = picker
            .matches()
            .get(picker.selected)
            .map(|entry| (*entry).clone())
        else {
            return Ok(());
        };
        let Some(source) = self.selected_row().map(|row| row.item.item_ref.clone()) else {
            return Ok(());
        };
        self.overlay = Overlay::None;
        let result = WriteEngine::with_default_journal(self.settings.clone())?.refile(
            &source,
            &target,
            false,
            &write_context(),
        )?;
        for diff in &result.diffs {
            if let Some(file) = diff
                .lines()
                .next()
                .and_then(|line| line.strip_prefix("--- "))
            {
                self.index.refresh_path(Path::new(file.trim()))?;
            }
        }
        self.index.refresh_path(Path::new(&result.item.file))?;
        self.recent_targets.retain(|recent| recent != &target);
        self.recent_targets.insert(0, target);
        self.recent_targets.truncate(10);
        let location = self.short_location(&result.item);
        let change = format!("refiled → {label}");
        if let Some(journal_id) = result.journal_ids.first() {
            self.record_triage(journal_id, REFILED);
            self.remember_undo(
                journal_id,
                &location,
                format!("{change} · {}", result.item.title),
            );
        }
        self.set_status(format!(
            "{} · u undo",
            words::change(&location, &change, &result.item.title)
        ));
        self.reload_rows()
    }

    /// `y`: copies the first link in the entry, else an org link to it.
    pub(super) fn copy_link(&mut self) -> Result<()> {
        let Some(row) = self.selected_row() else {
            return Ok(());
        };
        let body = super::peek::entry_text(&self.index, &row.item);
        let link = super::peek::links(&body)
            .into_iter()
            .next()
            .map(|(target, _)| target)
            .unwrap_or_else(|| match &row.item.id {
                Some(id) => format!("[[id:{id}][{}]]", row.item.title),
                None => format!(
                    "[[file:{}::{}][{}]]",
                    row.item.file, row.item.line, row.item.title
                ),
            });
        super::copy_to_clipboard(&link)?;
        self.set_status(format!("copied {link}"));
        Ok(())
    }
}

/// The first target's title, and how many more: `Pay rent + 2 more`.
pub(super) fn targets_summary(items: &[&Item]) -> String {
    match items {
        [] => String::new(),
        [item] => item.title.clone(),
        [first, rest @ ..] => format!("{} + {} more", first.title, rest.len()),
    }
}

/// Characters of `query` appear in `value` in order, ignoring case.
pub(super) fn fuzzy_match(value: &str, query: &str) -> bool {
    let mut wanted = query.chars().flat_map(char::to_lowercase).peekable();
    for character in value.chars().flat_map(char::to_lowercase) {
        if wanted.peek() == Some(&character) {
            wanted.next();
        }
    }
    wanted.peek().is_none()
}
