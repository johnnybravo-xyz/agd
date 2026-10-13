use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::write_context;
use super::{App, Overlay, PromptKind, RefileTarget, Screen, SelectionAnchor};
use anyhow::Result;
use chrono::{Duration, Local, NaiveDate};
use org_core::{Change, EditIntent, WriteEngine, parse_capture_line, parse_date_input};

pub(super) fn fuzzy_match(value: &str, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let mut query = query.chars().flat_map(char::to_lowercase);
    let mut wanted = query.next();
    for character in value.chars().flat_map(char::to_lowercase) {
        if Some(character) == wanted {
            wanted = query.next();
            if wanted.is_none() {
                return true;
            }
        }
    }
    false
}

pub(super) fn added_text(diff: &str) -> String {
    let mut text = diff
        .lines()
        .filter_map(|line| {
            if line.starts_with("+++") {
                None
            } else {
                line.strip_prefix('+')
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    text
}

impl App {
    pub(super) fn apply_state(&mut self, state: &str) -> Result<()> {
        let selected = self.selected;
        let anchor = self
            .selected_row()
            .map(|row| SelectionAnchor::from_item(&row.item));
        let targets = self.targets();
        let mut journals = Vec::new();
        let is_done = self.settings.todo.sequences.iter().any(|sequence| {
            sequence
                .iter()
                .skip_while(|keyword| keyword.as_str() != "|")
                .skip(1)
                .any(|keyword| keyword.split(['(', '!', '@']).next().unwrap_or(keyword) == state)
        });
        for item_ref in &targets {
            journals.push(self.edit_ref(
                item_ref,
                EditIntent {
                    state: Change::Set(state.to_string()),
                    ..EditIntent::default()
                },
            )?);
        }
        if is_done {
            self.record_triage(journals, 4);
        }
        self.marks.clear();
        self.set_status(format!("{} item(s) → {state} · u undo", targets.len()));
        self.reload_rows()?;
        self.selected = anchor
            .and_then(|anchor| anchor.find(&self.rows))
            .unwrap_or_else(|| selected.min(self.row_count().saturating_sub(1)));
        Ok(())
    }

    pub(super) fn cycle_state(&mut self) -> Result<()> {
        let Some(row) = self.selected_row() else {
            return Ok(());
        };
        let active: Vec<&str> = self
            .settings
            .todo
            .sequences
            .iter()
            .flat_map(|sequence| sequence.iter())
            .take_while(|keyword| keyword.as_str() != "|")
            .map(|keyword| keyword.split(['(', '!', '@']).next().unwrap_or(keyword))
            .collect();
        if active.is_empty() {
            return Ok(());
        }
        let current = row.item.todo.as_deref();
        let index = active
            .iter()
            .position(|keyword| Some(*keyword) == current)
            .unwrap_or(0);
        let next = active[(index + 1) % active.len()].to_string();
        self.apply_state(&next)
    }

    pub(super) fn cancel_selected(&mut self) -> Result<()> {
        let state = self
            .settings
            .todo
            .sequences
            .iter()
            .flat_map(|sequence| {
                sequence
                    .iter()
                    .skip_while(|keyword| keyword.as_str() != "|")
                    .skip(1)
            })
            .map(|keyword| keyword.split(['(', '!', '@']).next().unwrap_or(keyword))
            .find(|keyword| keyword.contains("CANCEL"))
            .map(str::to_string);
        if let Some(state) = state {
            self.apply_state(&state)
        } else {
            self.set_status("no cancellation state is configured".to_string());
            Ok(())
        }
    }

    pub(super) fn apply_date_to_targets(&mut self, value: &str) -> Result<()> {
        let date = parse_date_input(value, Local::now().date_naive())?;
        let targets = self.targets();
        let mut journals = Vec::new();
        for item_ref in &targets {
            journals.push(self.edit_ref(
                item_ref,
                EditIntent {
                    scheduled: Change::Set(date.clone()),
                    ..EditIntent::default()
                },
            )?);
        }
        let outcome = match value {
            "today" => 0,
            "+1d" => 1,
            _ => 2,
        };
        self.record_triage(journals, outcome);
        self.marks.clear();
        self.set_status(format!(
            "scheduled {} item(s) {value} · u undo",
            targets.len()
        ));
        self.reload_rows()
    }

    pub(super) fn apply_someday(&mut self) -> Result<()> {
        let targets = self.targets();
        let mut journals = Vec::new();
        for item_ref in &targets {
            journals.push(self.edit_ref(
                item_ref,
                EditIntent {
                    scheduled: Change::Clear,
                    tags_add: BTreeSet::from([self.settings.triage.someday_tag.clone()]),
                    ..EditIntent::default()
                },
            )?);
        }
        self.record_triage(journals, 3);
        self.marks.clear();
        self.set_status(format!(
            "moved {} item(s) to someday · u undo",
            targets.len()
        ));
        self.reload_rows()
    }

    pub(super) fn edit_ref(&mut self, item_ref: &str, intent: EditIntent) -> Result<String> {
        let mutation = WriteEngine::with_default_journal(self.settings.clone())?.edit(
            item_ref,
            &intent,
            false,
            &write_context(),
        )?;
        self.index.refresh_path(Path::new(&mutation.item.file))?;
        Ok(mutation.journal_id)
    }

    pub(super) fn record_triage(&mut self, journals: Vec<String>, outcome: usize) {
        if self.screen == Screen::Triage && !journals.is_empty() {
            self.triage_outcomes[outcome] += journals.len();
            self.triage_undo.push((journals, outcome));
        }
    }

    pub(super) fn undo(&mut self) -> Result<()> {
        if self.screen == Screen::Triage && !self.triage_undo.is_empty() {
            let undo_all = self.rows.is_empty();
            let mut groups = if undo_all {
                std::mem::take(&mut self.triage_undo)
            } else {
                vec![self.triage_undo.pop().expect("triage undo group")]
            };
            for (journals, outcome) in groups.drain(..).rev() {
                for journal in journals.iter().rev() {
                    let result = WriteEngine::with_default_journal(self.settings.clone())?
                        .undo(Some(journal), &write_context())?;
                    for file in result.file.split(',') {
                        self.index.refresh_path(Path::new(file))?;
                    }
                }
                self.triage_outcomes[outcome] =
                    self.triage_outcomes[outcome].saturating_sub(journals.len());
            }
            self.set_status(if undo_all {
                "undid all triage changes".to_string()
            } else {
                "undid last triage action".to_string()
            });
            return self.reload_rows();
        }
        let result = WriteEngine::with_default_journal(self.settings.clone())?
            .undo(None, &write_context())?;
        for file in result.file.split(',') {
            self.index.refresh_path(Path::new(file))?;
        }
        self.set_status(format!("undid {}", result.journal_id));
        self.reload_rows()
    }

    pub(super) fn review_selected(&mut self) -> Result<()> {
        let Some(record) = self.journals.get(self.selected) else {
            return Ok(());
        };
        WriteEngine::with_default_journal(self.settings.clone())?.review(&record.id)?;
        self.set_status(format!("reviewed {}", record.id));
        self.reload_rows()
    }

    pub(super) fn review_all(&mut self) -> Result<()> {
        let ids: Vec<String> = self
            .journals
            .iter()
            .map(|record| record.id.clone())
            .collect();
        let engine = WriteEngine::with_default_journal(self.settings.clone())?;
        for id in &ids {
            engine.review(id)?;
        }
        self.set_status(format!("reviewed {} agent changes", ids.len()));
        self.reload_rows()
    }

    pub(super) fn undo_review_selected(&mut self) -> Result<()> {
        let Some(record) = self.journals.get(self.selected) else {
            return Ok(());
        };
        let id = record.id.clone();
        let result = WriteEngine::with_default_journal(self.settings.clone())?
            .undo(Some(&id), &write_context())?;
        for file in result.file.split(',') {
            self.index.refresh_path(Path::new(file))?;
        }
        self.set_status(format!("undid agent change {id}"));
        self.reload_rows()
    }

    pub(super) fn open_refile(&mut self) {
        let Some(source_ref) = self.selected_row().map(|row| row.item.item_ref.clone()) else {
            return;
        };
        let mut targets: Vec<RefileTarget> = self
            .index
            .items()
            .filter(|item| item.item_ref != source_ref)
            .map(|item| RefileTarget {
                item_ref: item.item_ref.clone(),
                label: format!("{}  {}:{}", item.olpath.join(" / "), item.file, item.line),
            })
            .collect();
        targets.sort_by_key(|target| {
            (
                self.recent_targets
                    .iter()
                    .position(|item_ref| item_ref == &target.item_ref)
                    .unwrap_or(usize::MAX),
                target.label.to_lowercase(),
            )
        });
        self.overlay = Overlay::Refile {
            input: String::new(),
            selected: 0,
            targets,
        };
    }

    pub(super) fn commit_refile(&mut self) -> Result<()> {
        let Overlay::Refile {
            input,
            selected,
            targets,
        } = &self.overlay
        else {
            return Ok(());
        };
        let Some(target) = targets
            .iter()
            .filter(|target| fuzzy_match(&target.label, input))
            .nth(*selected)
            .cloned()
        else {
            return Ok(());
        };
        let Some((source_ref, source_file)) = self
            .selected_row()
            .map(|row| (row.item.item_ref.clone(), row.item.file.clone()))
        else {
            return Ok(());
        };
        let result = WriteEngine::with_default_journal(self.settings.clone())?.refile(
            &source_ref,
            &target.item_ref,
            false,
            &write_context(),
        )?;
        self.index.refresh_path(Path::new(&source_file))?;
        self.index.refresh_path(Path::new(&result.item.file))?;
        self.recent_targets
            .retain(|item_ref| item_ref != &target.item_ref);
        self.recent_targets.insert(0, target.item_ref);
        self.recent_targets.truncate(10);
        self.record_triage(result.journal_ids, 5);
        self.overlay = Overlay::None;
        self.set_status(format!("refiled {} · u undo", result.item.item_ref));
        self.reload_rows()
    }

    pub(super) fn refresh_capture_preview(&mut self) {
        let Overlay::Capture {
            template,
            input,
            preview,
            body,
        } = &mut self.overlay
        else {
            return;
        };
        let Some(template) = template.clone() else {
            *preview = "type a capture template key".to_string();
            return;
        };
        let result = parse_capture_line(input, Local::now().date_naive()).and_then(|draft| {
            WriteEngine::with_default_journal(self.settings.clone())?.capture(
                &template,
                &draft,
                body,
                &BTreeMap::new(),
                true,
                &write_context(),
            )
        });
        *preview = result
            .map(|mutation| added_text(&mutation.diff))
            .unwrap_or_else(|error| error.to_string());
    }

    pub(super) fn commit_capture(&mut self) -> Result<()> {
        let Overlay::Capture {
            template,
            input,
            body,
            ..
        } = &self.overlay
        else {
            return Ok(());
        };
        let template = template.clone().expect("capture template selected");
        let draft = parse_capture_line(input, Local::now().date_naive())?;
        let mutation = WriteEngine::with_default_journal(self.settings.clone())?.capture(
            &template,
            &draft,
            body,
            &BTreeMap::new(),
            false,
            &write_context(),
        )?;
        self.index.refresh_path(Path::new(&mutation.item.file))?;
        self.overlay = Overlay::None;
        self.set_status(format!("captured {} · u undo", mutation.item.item_ref));
        if self.capture_only {
            self.quit = true;
            return Ok(());
        }
        self.reload_rows()
    }

    pub(super) fn commit_prompt(&mut self) -> Result<()> {
        let Overlay::Prompt { kind, input } = &self.overlay else {
            return Ok(());
        };
        let kind = *kind;
        let input = input.clone();
        if kind == PromptKind::Filter {
            self.filter = input;
            self.overlay = Overlay::None;
            return self.reload_rows();
        }
        let Some(item_ref) = self.selected_row().map(|row| row.item.item_ref.clone()) else {
            self.overlay = Overlay::None;
            return Ok(());
        };
        let mut triage_outcome = None;
        let mut intent = EditIntent::default();
        match kind {
            PromptKind::Schedule => {
                let date = parse_date_input(&input, Local::now().date_naive())?;
                let planned = date.date.parse::<NaiveDate>()?;
                let today = Local::now().date_naive();
                triage_outcome = Some(if planned <= today {
                    0
                } else if planned == today + Duration::days(1) {
                    1
                } else {
                    2
                });
                intent.scheduled = Change::Set(date);
            }
            PromptKind::Deadline => {
                intent.deadline = Change::Set(parse_date_input(&input, Local::now().date_naive())?)
            }
            PromptKind::Priority => intent.priority = Change::Set(input),
            PromptKind::Tags => {
                for tag in input
                    .split(',')
                    .map(str::trim)
                    .filter(|tag| !tag.is_empty())
                {
                    if let Some(tag) = tag.strip_prefix('-') {
                        intent.tags_remove.insert(tag.to_string());
                    } else {
                        intent
                            .tags_add
                            .insert(tag.trim_start_matches('+').to_string());
                    }
                }
            }
            PromptKind::Filter => unreachable!(),
        }
        let journal = self.edit_ref(&item_ref, intent)?;
        if let Some(outcome) = triage_outcome {
            self.record_triage(vec![journal], outcome);
        }
        self.overlay = Overlay::None;
        self.set_status(format!("updated {item_ref} · u undo"));
        self.reload_rows()
    }
}
