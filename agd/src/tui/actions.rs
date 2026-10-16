use std::collections::BTreeSet;
use std::path::Path;

use agd_core::{
    Change, EditIntent, JournalStatus, PlanningTimestamp, WriteEngine, parse_date_input,
};
use anyhow::{Result, bail};
use chrono::{Duration, NaiveDate};

use super::{App, Overlay, PromptKind, Screen, write_context};

/// Triage outcomes, counted for the summary line.
pub(super) const TODAY: usize = 0;
pub(super) const TOMORROW: usize = 1;
pub(super) const NEXT_WEEK: usize = 2;
pub(super) const SOMEDAY: usize = 3;
pub(super) const DONE: usize = 4;
pub(super) const REFILED: usize = 5;
pub(super) const OTHER: usize = 6;

/// Fuzzy refile target picker over every heading, recent targets first.
#[derive(Debug)]
pub(super) struct RefilePicker {
    pub input: String,
    pub selected: usize,
    targets: Vec<(String, String)>,
}

impl RefilePicker {
    /// (item ref, `path › outline`) pairs matching the typed text.
    pub(super) fn matches(&self) -> Vec<&(String, String)> {
        self.targets
            .iter()
            .filter(|(_, label)| fuzzy_match(label, &self.input))
            .collect()
    }
}

impl App {
    /// Applies one edit to every target (marked rows, else the selected
    /// row) through the guarded write path and confirms it on the status
    /// line the way org-agenda does: `work.org:142  TODO → DONE · u undo`.
    fn edit_targets(&mut self, intent: EditIntent, outcome: usize, change: &str) -> Result<()> {
        let targets = self.targets();
        if targets.is_empty() {
            return Ok(());
        }
        let location = self
            .selected_row()
            .map(|row| self.short_location(&row.item))
            .unwrap_or_default();
        let engine = WriteEngine::with_default_journal(self.settings.clone())?;
        let mut journals = Vec::new();
        for item_ref in &targets {
            let mutation = engine.edit(item_ref, &intent, false, &write_context())?;
            self.index.refresh_path(Path::new(&mutation.item.file))?;
            journals.push(mutation.journal_id);
        }
        self.record_triage(journals, outcome);
        self.marks.clear();
        self.set_status(if targets.len() == 1 {
            format!("{location}  {change} · u undo")
        } else {
            format!("{} items  {change} · u undo", targets.len())
        });
        self.reload_rows()
    }

    pub(super) fn set_state(&mut self, state: &str) -> Result<()> {
        let from = self
            .selected_row()
            .and_then(|row| row.item.todo.clone())
            .unwrap_or_else(|| "·".to_string());
        let outcome = if self.is_done_keyword(state) {
            DONE
        } else {
            OTHER
        };
        self.edit_targets(
            EditIntent {
                state: Change::Set(state.to_string()),
                ..EditIntent::default()
            },
            outcome,
            &format!("{from} → {state}"),
        )
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

    /// `X` in the lists, `k` in triage: the file's done keyword whose name
    /// starts with CANCEL (CANCELLED or CANCELED).
    pub(super) fn cancel(&mut self) -> Result<()> {
        let state = self
            .selected_keywords()
            .into_iter()
            .find(|(keyword, done)| *done && keyword.starts_with("CANCEL"));
        match state {
            Some((state, _)) => self.set_state(&state),
            None => bail!("no CANCELLED/CANCELED state in this file's TODO keywords"),
        }
    }

    /// (keyword, done) for the selected row's TODO sequence in its file.
    pub(super) fn selected_keywords(&self) -> Vec<(String, bool)> {
        let Some(row) = self.selected_row() else {
            return Vec::new();
        };
        let Some(document) = self.index.document(Path::new(&row.item.file)) else {
            return Vec::new();
        };
        let sequence = row
            .item
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

    /// Reschedules to an org date (`today`, `+1d`, `fri`, `2026-10-30`).
    pub(super) fn schedule(&mut self, value: &str) -> Result<()> {
        let date = parse_date_input(value, self.today)?;
        let planned = date.date.parse::<NaiveDate>()?;
        let outcome = if planned <= self.today {
            TODAY
        } else if planned == self.today + Duration::days(1) {
            TOMORROW
        } else if planned <= self.today + Duration::days(7) {
            NEXT_WEEK
        } else {
            OTHER
        };
        let label = format!("scheduled {}", planned.format("%a %-d %b"));
        self.edit_targets(
            EditIntent {
                scheduled: Change::Set(date),
                ..EditIntent::default()
            },
            outcome,
            &label,
        )
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
    /// org-agenda's priority-up and priority-down.
    pub(super) fn shift_priority(&mut self, up: bool) -> Result<()> {
        let Some(row) = self.selected_row() else {
            return Ok(());
        };
        let agenda = &self.settings;
        let letter = |value: &str| value.chars().next().unwrap_or('B');
        let (highest, lowest) = (
            letter(&agenda.highest_priority),
            letter(&agenda.lowest_priority),
        );
        let current = row
            .item
            .priority
            .as_deref()
            .map_or(letter(&agenda.default_priority), letter);
        let next = if up {
            (current as u8).saturating_sub(1).max(highest as u8) as char
        } else {
            (current as u8 + 1).min(lowest as u8) as char
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

    pub(super) fn commit_prompt(&mut self) -> Result<()> {
        let Overlay::Prompt { kind, input } = &self.overlay else {
            return Ok(());
        };
        let (kind, input) = (*kind, input.trim().to_string());
        self.overlay = Overlay::None;
        match kind {
            PromptKind::Filter => {
                self.filter = input;
                self.reload_rows()
            }
            PromptKind::Schedule => self.schedule(&input),
            PromptKind::Deadline => {
                let date = parse_date_input(&input, self.today)?;
                let label = format!("deadline {}", date.date);
                self.edit_targets(
                    EditIntent {
                        deadline: Change::Set(date),
                        ..EditIntent::default()
                    },
                    OTHER,
                    &label,
                )
            }
            PromptKind::Priority => self.edit_targets(
                EditIntent {
                    priority: if input.is_empty() {
                        Change::Clear
                    } else {
                        Change::Set(input.to_ascii_uppercase())
                    },
                    ..EditIntent::default()
                },
                OTHER,
                "priority",
            ),
            PromptKind::Tags => {
                let mut intent = EditIntent::default();
                for tag in input.split([',', ' ', ':']).filter(|tag| !tag.is_empty()) {
                    if let Some(tag) = tag.strip_prefix('-') {
                        intent.tags_remove.insert(tag.to_string());
                    } else {
                        intent
                            .tags_add
                            .insert(tag.trim_start_matches('+').to_string());
                    }
                }
                self.edit_targets(intent, OTHER, "tags")
            }
        }
    }

    fn record_triage(&mut self, journals: Vec<String>, outcome: usize) {
        if self.screen == Screen::Triage && !journals.is_empty() {
            self.triage.outcomes[outcome] += journals.len();
            self.triage.undo.push((journals, outcome));
        }
    }

    /// `u` undoes the last write; in a finished triage pass it undoes the
    /// whole pass, replaying the journal backwards.
    pub(super) fn undo(&mut self) -> Result<()> {
        let engine = WriteEngine::with_default_journal(self.settings.clone())?;
        if self.screen == Screen::Triage && !self.triage.undo.is_empty() {
            let undo_all = self.rows.is_empty();
            let groups = if undo_all {
                std::mem::take(&mut self.triage.undo)
            } else {
                self.triage.undo.pop().into_iter().collect()
            };
            for (journals, outcome) in groups.into_iter().rev() {
                for journal in journals.iter().rev() {
                    let result = engine.undo(Some(journal), &write_context())?;
                    self.refresh_files(&result.file)?;
                }
                self.triage.outcomes[outcome] =
                    self.triage.outcomes[outcome].saturating_sub(journals.len());
            }
            self.set_status(if undo_all {
                "undid the whole triage pass"
            } else {
                "undid the last triage step"
            });
            return self.reload_rows();
        }
        let result = engine.undo(None, &write_context())?;
        self.refresh_files(&result.file)?;
        self.set_status(format!("undid {}", result.journal_id));
        self.reload_rows()
    }

    fn refresh_files(&mut self, files: &str) -> Result<()> {
        for file in files.split(',') {
            self.index.refresh_path(Path::new(file))?;
        }
        Ok(())
    }

    /// Accept writes nothing: it only marks the change reviewed.
    pub(super) fn accept_selected(&mut self) -> Result<()> {
        let Some(record) = self.journals.get(self.selected) else {
            return Ok(());
        };
        if record.status != JournalStatus::Committed {
            return Ok(());
        }
        WriteEngine::with_default_journal(self.settings.clone())?.review(&record.id)?;
        self.set_status("accepted · nothing written");
        self.reload_rows()
    }

    pub(super) fn accept_all(&mut self) -> Result<()> {
        let engine = WriteEngine::with_default_journal(self.settings.clone())?;
        let mut accepted = 0;
        for record in &self.journals {
            if record.status == JournalStatus::Committed {
                engine.review(&record.id)?;
                accepted += 1;
            }
        }
        self.set_status(format!("accepted {accepted} agent changes"));
        self.reload_rows()
    }

    /// Undo applies the journal's reverse patch only where those bytes are
    /// unchanged; otherwise the conflict is reported and nothing is written.
    pub(super) fn undo_selected_change(&mut self) -> Result<()> {
        let Some(id) = self
            .journals
            .get(self.selected)
            .map(|record| record.id.clone())
        else {
            return Ok(());
        };
        let result = WriteEngine::with_default_journal(self.settings.clone())?
            .undo(Some(&id), &write_context())?;
        self.refresh_files(&result.file)?;
        self.set_status("undid the agent change");
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
            input: String::new(),
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
        self.record_triage(result.journal_ids, REFILED);
        self.set_status(format!("refiled to {label} · u undo"));
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

/// Inactive timestamp text from a planning timestamp, for display.
pub(super) fn short_date(timestamp: &PlanningTimestamp) -> String {
    timestamp
        .date
        .parse::<NaiveDate>()
        .map(|date| date.format("%-d %b").to_string())
        .unwrap_or_else(|_| timestamp.date.clone())
}
