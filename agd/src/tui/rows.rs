use std::collections::BTreeSet;

use anyhow::Result;
use chrono::{Datelike, Duration, NaiveDate};
use org_core::{
    AgendaEngine, AgendaEntry, AgendaQuery, AgendaSpan, Item, JournalRecord, JournalStatus,
    OccurrenceType, WorkspaceIndex, WriteEngine,
};

use super::{
    App, DOING, DUE, DisplayRow, Header, INBOX, OVERDUE, SCHEDULE, Screen, SelectionAnchor,
};

/// Rows shown under OVERDUE on Today; the rest wait in Triage.
const OVERDUE_ON_TODAY: usize = 3;
/// Agent review shows this many recent changes.
const REVIEW_LIMIT: usize = 200;

impl App {
    /// Rebuilds the visible rows and keeps the cursor on the same heading
    /// (by ID or file, line and outline path); if it left the list, the
    /// cursor stays at the same position, which is the next row.
    pub(super) fn reload_rows(&mut self) -> Result<()> {
        let previous = self.selected;
        let anchor = self
            .selected_row()
            .map(|row| SelectionAnchor::from_item(&row.item));
        self.rebuild_rows()?;
        self.selected = anchor
            .and_then(|anchor| anchor.find(&self.rows))
            .unwrap_or(previous)
            .min(self.row_count().saturating_sub(1));
        Ok(())
    }

    /// The header tabs in order: the start view first when one is set.
    pub(super) fn tabs(&self) -> Vec<Screen> {
        let mut tabs = Vec::new();
        if !self.settings.start_view.is_empty() {
            tabs.push(Screen::View(self.settings.start_view.clone()));
        }
        tabs.extend([Screen::Today, Screen::Week, Screen::Inbox, Screen::Review]);
        tabs
    }

    /// Changes screen with the cursor on the first row; esc comes back from
    /// Triage, Agents and saved views other than the start view, which is a
    /// tab.
    pub(super) fn switch_screen(&mut self, screen: Screen) -> Result<()> {
        let tab_view = matches!(&screen, Screen::View(key) if *key == self.settings.start_view);
        if matches!(screen, Screen::Triage | Screen::Review | Screen::View(_))
            && !tab_view
            && !matches!(self.screen, Screen::Triage | Screen::Review)
        {
            self.back = self.screen.clone();
        }
        self.screen = screen;
        self.rows.clear();
        self.selected = 0;
        self.marks.clear();
        self.reload_rows()
    }

    pub(super) fn selected_row(&self) -> Option<&DisplayRow> {
        self.rows.get(self.selected)
    }

    pub(super) fn row_count(&self) -> usize {
        if self.screen == Screen::Review {
            self.journals.len()
        } else {
            self.rows.len()
        }
    }

    pub(super) fn move_selection(&mut self, delta: isize) {
        let count = self.row_count();
        self.selected = if count == 0 {
            0
        } else {
            (self.selected as isize + delta).clamp(0, count as isize - 1) as usize
        };
    }

    pub(super) fn toggle_mark(&mut self) {
        let Some(item_ref) = self.selected_row().map(|row| row.item.item_ref.clone()) else {
            return;
        };
        if !self.marks.remove(&item_ref) {
            self.marks.insert(item_ref);
        }
        self.move_selection(1);
    }

    /// Marked rows when any are marked (bulk mode), else the selected row.
    pub(super) fn targets(&self) -> Vec<String> {
        if self.marks.is_empty() {
            self.selected_row()
                .map(|row| vec![row.item.item_ref.clone()])
                .unwrap_or_default()
        } else {
            self.marks.iter().cloned().collect()
        }
    }

    /// Categories in the order they first appear, for the `<` context filter.
    pub(super) fn categories(&self) -> Vec<String> {
        let mut seen = BTreeSet::new();
        self.index
            .items()
            .filter(|item| item.todo.is_some())
            .map(|item| item.category.clone())
            .filter(|category| seen.insert(category.clone()))
            .collect()
    }

    fn rebuild_rows(&mut self) -> Result<()> {
        let journals = WriteEngine::with_default_journal(self.settings.clone())?.journal()?;
        self.provenance.clear();
        for record in &journals {
            if record.diff.lines().any(|line| line.starts_with("+*"))
                && let Some(item_ref) = &record.item_ref
            {
                let actor = record
                    .actor
                    .strip_prefix("mcp:")
                    .unwrap_or("you")
                    .to_string();
                let time = record.timestamp.get(11..16).unwrap_or_default().to_string();
                self.provenance.insert(item_ref.clone(), (actor, time));
            }
        }
        self.counts.agents = journals
            .iter()
            .filter(|record| is_agent(record) && record.status == JournalStatus::Committed)
            .count();
        let engine = AgendaEngine::new(&self.index);
        let today_entries = engine.agenda(&self.query(self.today, self.today))?;
        self.counts.overdue = today_entries
            .iter()
            .filter(|entry| is_overdue(entry))
            .count();
        let inbox = engine.inbox();
        self.counts.inbox = inbox.len();

        let mut headers: Vec<Header> = Vec::new();
        let mut rows = match &self.screen {
            Screen::Review => {
                self.journals = review_records(journals, self.agent_filter.as_deref());
                self.rows.clear();
                return Ok(());
            }
            Screen::Today if self.pane => {
                pane_rows(engine.agenda(&self.query(self.today, self.today))?)
            }
            Screen::Today if self.classic => {
                headers.extend(day_labels(self.day, self.day));
                self.day_rows(&engine)?
            }
            Screen::Today => today_rows(
                engine.agenda(&self.query(self.day, self.day))?,
                &engine.todo(None, self.today)?,
                &inbox,
                &self.index,
            ),
            Screen::Week => {
                let (from, to) = AgendaSpan::Week.bounds(self.day, &self.settings.week_start);
                headers.extend(day_labels(from, to));
                day_grouped(engine.agenda(&self.query(from, to))?)
            }
            Screen::Inbox => inbox
                .into_iter()
                .map(|item| DisplayRow {
                    section: INBOX.to_string(),
                    item,
                    occurrence: None,
                })
                .collect(),
            Screen::View(key) => {
                let mut rows = Vec::new();
                if let Some(view) = self.settings.view(key) {
                    for section in engine.view(view, self.day, self.today, self.log_mode)? {
                        // A grouped block shows its groups instead of days,
                        // as org-super-agenda does.
                        if !section.groups.is_empty() {
                            if !section.header.is_empty() {
                                headers.push(Header::section(
                                    section.header.clone(),
                                    Some(section.color.clone()),
                                ));
                            }
                            for group in section.groups {
                                headers
                                    .push(Header::section(group.name.clone(), Some(group.color)));
                                rows.extend(group.entries.into_iter().map(|entry| DisplayRow {
                                    section: group.name.clone(),
                                    item: entry.item,
                                    occurrence: Some(entry.occurrence),
                                }));
                                rows.extend(group.items.into_iter().map(|item| DisplayRow {
                                    section: group.name.clone(),
                                    item,
                                    occurrence: None,
                                }));
                            }
                            continue;
                        }
                        // Agenda blocks list every day under their title, as
                        // an Emacs block agenda does; list blocks use theirs.
                        if let (Some(from), Some(to)) = (section.from, section.to) {
                            let title = if section.header.is_empty() {
                                agenda_block_title(from, to)
                            } else {
                                section.header.clone()
                            };
                            headers.push(Header::section(title, Some(section.color.clone())));
                            headers.extend(day_labels(from, to));
                            rows.extend(day_grouped(section.entries));
                        }
                        if section.from.is_none() && !section.header.is_empty() {
                            headers.push(Header::section(
                                section.header.clone(),
                                Some(section.color.clone()),
                            ));
                        }
                        rows.extend(section.items.into_iter().map(|item| DisplayRow {
                            section: section.header.clone(),
                            item,
                            occurrence: None,
                        }));
                    }
                }
                rows
            }
            Screen::Triage => {
                self.reschedules = reschedule_counts(&journals, &self.index);
                let someday = self.settings.someday_tag.clone();
                let mut rows = triage_rows(today_entries, inbox);
                // Demoted tasks are done being triaged.
                rows.retain(|row| !row.item.tags.contains(&someday));
                rows
            }
        };
        rows.retain(|row| self.visible(&row.item));
        // Sections that only exist through their rows (Today, Inbox).
        for row in &rows {
            if !headers.iter().any(|header| header.text == row.section) {
                headers.push(Header::section(row.section.clone(), None));
            }
        }
        self.headers = headers;
        self.rows = rows;
        Ok(())
    }

    fn query(&self, from: NaiveDate, to: NaiveDate) -> AgendaQuery {
        AgendaQuery {
            from,
            to,
            today: self.today,
            match_expression: None,
            log_mode: self.log_mode,
        }
    }

    /// Classic view: the plain org-agenda day list for `day`.
    fn day_rows(&self, engine: &AgendaEngine<'_>) -> Result<Vec<DisplayRow>> {
        Ok(day_grouped(engine.agenda(&self.query(self.day, self.day))?))
    }

    /// Context (`<`) and text/tag (`/`) filters.
    fn visible(&self, item: &Item) -> bool {
        let context = self
            .context
            .as_ref()
            .is_none_or(|context| item.category.eq_ignore_ascii_case(context));
        let filter = self.filter.to_lowercase();
        context
            && (filter.is_empty()
                || item.title.to_lowercase().contains(&filter)
                || item
                    .tags
                    .iter()
                    .chain(&item.inherited_tags)
                    .any(|tag| tag.to_lowercase() == filter.trim_start_matches('+')))
    }
}

/// Today's sections in the order you act on them. OVERDUE shows only the
/// first few; DOING lists in-progress TODOs without a date today.
fn today_rows(
    entries: Vec<AgendaEntry>,
    todos: &[Item],
    inbox: &[Item],
    index: &WorkspaceIndex,
) -> Vec<DisplayRow> {
    let mut sections: [(&str, Vec<DisplayRow>); 5] = [
        (OVERDUE, Vec::new()),
        (SCHEDULE, Vec::new()),
        (DUE, Vec::new()),
        (DOING, Vec::new()),
        (INBOX, Vec::new()),
    ];
    let mut shown = BTreeSet::new();
    for entry in entries {
        let slot = if is_overdue(&entry) {
            0
        } else if matches!(
            entry.occurrence.kind,
            OccurrenceType::Deadline | OccurrenceType::UpcomingDeadline
        ) {
            2
        } else {
            1
        };
        shown.insert(entry.item.item_ref.clone());
        sections[slot].1.push(DisplayRow {
            section: sections[slot].0.to_string(),
            item: entry.item,
            occurrence: Some(entry.occurrence),
        });
    }
    sections[0].1.truncate(OVERDUE_ON_TODAY);
    for item in todos {
        if !shown.contains(&item.item_ref) && is_in_progress(item, index) {
            shown.insert(item.item_ref.clone());
            sections[3].1.push(DisplayRow {
                section: DOING.to_string(),
                item: item.clone(),
                occurrence: None,
            });
        }
    }
    for item in inbox {
        if shown.insert(item.item_ref.clone()) {
            sections[4].1.push(DisplayRow {
                section: INBOX.to_string(),
                item: item.clone(),
                occurrence: None,
            });
        }
    }
    sections.into_iter().flat_map(|(_, rows)| rows).collect()
}

/// The pane: today's dated rows, timed ones by clock time, then untimed,
/// with done rows last; overdue rows are only counted.
fn pane_rows(entries: Vec<AgendaEntry>) -> Vec<DisplayRow> {
    let mut rows: Vec<DisplayRow> = entries
        .into_iter()
        .filter(|entry| !is_overdue(entry))
        .map(|entry| DisplayRow {
            section: SCHEDULE.to_string(),
            item: entry.item,
            occurrence: Some(entry.occurrence),
        })
        .collect();
    rows.sort_by_key(|row| {
        (
            row.item.done,
            row.occurrence
                .as_ref()
                .and_then(|occurrence| occurrence.time.clone())
                .is_none(),
            row.occurrence
                .as_ref()
                .and_then(|occurrence| occurrence.time.clone()),
        )
    });
    rows
}

/// Reschedules per item: Org's own "Rescheduled from" LOGBOOK notes when
/// org-log-reschedule wrote them, otherwise agd journal entries that moved
/// an existing SCHEDULED date.
fn reschedule_counts(
    journals: &[JournalRecord],
    index: &WorkspaceIndex,
) -> std::collections::BTreeMap<String, usize> {
    let mut counts = std::collections::BTreeMap::new();
    for record in journals {
        let moved = record
            .diff
            .lines()
            .any(|line| line.starts_with('-') && line.contains("SCHEDULED:"))
            && record
                .diff
                .lines()
                .any(|line| line.starts_with('+') && line.contains("SCHEDULED:"));
        if moved && let Some(item_ref) = &record.item_ref {
            *counts.entry(item_ref.clone()).or_default() += 1;
        }
    }
    for document in index.documents() {
        for headline in document.headlines() {
            let section =
                &document.source()[headline.section_span.start..headline.section_span.end];
            let logged = section.matches("- Rescheduled from").count();
            if logged > 0 {
                counts.insert(headline.item.item_ref.clone(), logged);
            }
        }
    }
    counts
}

/// org-agenda's day header: `Wednesday  14 October 2026`.
pub(super) fn day_label(date: NaiveDate) -> String {
    format!(
        "{:<10} {:>2} {} {}",
        date.format("%A"),
        date.day(),
        date.format("%B"),
        date.year()
    )
}

/// Emacs's title for an agenda block without an overriding header.
fn agenda_block_title(from: NaiveDate, to: NaiveDate) -> String {
    let days = (to - from).num_days() + 1;
    let span = match days {
        1 => "Day".to_string(),
        7 => "Week".to_string(),
        days => format!("{days}-day"),
    };
    format!("{span}-agenda (W{:02}):", from.iso_week().week())
}

fn day_labels(from: NaiveDate, to: NaiveDate) -> Vec<Header> {
    let mut labels = Vec::new();
    let mut day = from;
    while day <= to {
        labels.push(Header {
            text: day_label(day),
            day: true,
            color: None,
        });
        day += Duration::days(1);
    }
    labels
}

/// org-agenda rows grouped under one header per day, as in Emacs.
fn day_grouped(entries: Vec<AgendaEntry>) -> Vec<DisplayRow> {
    entries
        .into_iter()
        .map(|entry| {
            let section = entry
                .occurrence
                .date
                .parse::<NaiveDate>()
                .map(day_label)
                .unwrap_or_else(|_| entry.occurrence.date.clone());
            DisplayRow {
                section,
                item: entry.item,
                occurrence: Some(entry.occurrence),
            }
        })
        .collect()
}

/// Overdue items first (most days late first), then the inbox.
fn triage_rows(entries: Vec<AgendaEntry>, inbox: Vec<Item>) -> Vec<DisplayRow> {
    let mut seen = BTreeSet::new();
    let mut rows: Vec<DisplayRow> = entries
        .into_iter()
        .filter(|entry| is_overdue(entry) && !entry.item.done)
        .filter(|entry| seen.insert(entry.item.item_ref.clone()))
        .map(|entry| DisplayRow {
            section: OVERDUE.to_string(),
            item: entry.item,
            occurrence: Some(entry.occurrence),
        })
        .collect();
    rows.extend(
        inbox
            .into_iter()
            .filter(|item| seen.insert(item.item_ref.clone()))
            .map(|item| DisplayRow {
                section: INBOX.to_string(),
                item,
                occurrence: None,
            }),
    );
    rows
}

/// Past-scheduled reminders and overdue deadlines.
pub(super) fn is_overdue(entry: &AgendaEntry) -> bool {
    match entry.occurrence.kind {
        OccurrenceType::PastScheduled => true,
        OccurrenceType::Deadline => entry.occurrence.extra.ends_with("ago:"),
        _ => false,
    }
}

/// A TODO in a middle state of its sequence (NEXT, INPROGRESS, WAIT…):
/// not the first keyword and not done.
fn is_in_progress(item: &Item, index: &WorkspaceIndex) -> bool {
    let Some(todo) = item.todo.as_deref() else {
        return false;
    };
    let Some(document) = index.document(std::path::Path::new(&item.file)) else {
        return false;
    };
    let Some(keyword) = document.todo_keyword(todo) else {
        return false;
    };
    let head = document
        .todo_keywords()
        .iter()
        .find(|candidate| candidate.sequence == keyword.sequence);
    !item.done && head.is_some_and(|head| head.name != todo)
}

fn is_agent(record: &JournalRecord) -> bool {
    record.actor.starts_with("mcp:")
}

/// Recent agent changes, unreviewed first, newest first within each group.
fn review_records(journals: Vec<JournalRecord>, agent: Option<&str>) -> Vec<JournalRecord> {
    let mut records: Vec<JournalRecord> = journals
        .into_iter()
        .filter(|record| {
            is_agent(record)
                && matches!(
                    record.status,
                    JournalStatus::Committed | JournalStatus::Reviewed
                )
                && agent.is_none_or(|agent| record.actor.strip_prefix("mcp:") == Some(agent))
        })
        .collect();
    records.sort_by(|left, right| {
        (left.status != JournalStatus::Committed)
            .cmp(&(right.status != JournalStatus::Committed))
            .then_with(|| right.timestamp.cmp(&left.timestamp))
    });
    records.truncate(REVIEW_LIMIT);
    records
}

/// Days since a date, for "waiting 3d" and triage ages.
pub(super) fn days_since(date: &str, today: NaiveDate) -> Option<i64> {
    let date = date.get(..10)?.parse::<NaiveDate>().ok()?;
    Some((today - date).num_days().max(0))
}

/// Next occurrence of a repeating SCHEDULED/DEADLINE after marking it DONE,
/// as org-auto-repeat-maybe computes it, for the peek preview.
pub(super) fn next_repeat(date: &str, repeater: &str, today: NaiveDate) -> Option<NaiveDate> {
    let base = date.parse::<NaiveDate>().ok()?;
    let (kind, value) = if let Some(value) = repeater.strip_prefix("++") {
        ("++", value)
    } else if let Some(value) = repeater.strip_prefix(".+") {
        (".+", value)
    } else {
        ("+", repeater.strip_prefix('+')?)
    };
    let value = value.split('/').next()?;
    let unit = value.chars().last()?;
    let amount: i64 = value[..value.len() - 1].parse().ok()?;
    let step = |date: NaiveDate| -> Option<NaiveDate> {
        match unit {
            'd' => Some(date + Duration::days(amount)),
            'w' => Some(date + Duration::weeks(amount)),
            'm' => date.checked_add_months(chrono::Months::new(amount as u32)),
            'y' => date.checked_add_months(chrono::Months::new(12 * amount as u32)),
            _ => None,
        }
    };
    match kind {
        ".+" => step(today),
        "++" => {
            let mut next = step(base)?;
            while next <= today {
                next = step(next)?;
            }
            Some(next)
        }
        _ => step(base),
    }
}
