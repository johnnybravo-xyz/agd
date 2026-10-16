//! What each screen lists: Today's sections, Week's days, the inbox, saved
//! views, Triage's frozen queue and the Agents tab's journal records, read
//! from the agenda engine and the journal. Also the cursor (kept on the
//! same heading across reloads), marks, screen switching and the `<` and
//! `/` filters.

use std::collections::BTreeSet;
use std::time::Instant;

use agd_core::{
    AgendaEngine, AgendaEntry, AgendaQuery, AgendaSpan, Item, JournalOperation, JournalRecord,
    JournalStatus, OccurrenceType, WorkspaceIndex, WriteEngine, overdue_entries,
};
use anyhow::Result;
use chrono::{Datelike, Duration, NaiveDate};

use super::{
    App, DOING, DUE, DisplayRow, Header, INBOX, OVERDUE, SCHEDULE, Screen, SelectionAnchor,
    TriageState,
};

/// Rows shown under OVERDUE on Today; the rest wait in Triage.
const OVERDUE_ON_TODAY: usize = 3;
/// The Agents tab shows this many recent changes.
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
        if self.screen == Screen::Triage {
            // Triage's cursor is the pass's position in its frozen queue.
            self.selected = self.triage.position.min(self.rows.len().saturating_sub(1));
            return Ok(());
        }
        self.selected = anchor
            .and_then(|anchor| anchor.find(&self.rows))
            .unwrap_or(previous)
            .min(self.row_count().saturating_sub(1));
        Ok(())
    }

    /// `T`: overdue tasks in the core's order (overdue_entries), then the
    /// inbox, frozen as the pass's queue.
    pub(super) fn start_triage(&mut self) -> Result<()> {
        let engine = AgendaEngine::new(&self.index);
        let entries = engine.agenda(&self.query(self.today, self.today))?;
        let someday = &self.settings.someday_tag;
        let mut queue = triage_rows(&entries, engine.inbox());
        // Demoted tasks are done being triaged.
        queue.retain(|row| !row.item.tags.contains(someday) && self.visible(&row.item));
        self.triage = TriageState {
            started: Some(Instant::now()),
            queue,
            ..TriageState::default()
        };
        self.switch_screen(Screen::Triage)
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

    /// The tabs as the header shows them, left to right, for tab, S-tab
    /// and 1–9: the tabs, then a saved view open that is not one, then
    /// Triage while it runs.
    pub(super) fn shown_tabs(&self) -> Vec<Screen> {
        let mut shown = self.tabs();
        if matches!(self.screen, Screen::View(_) | Screen::Triage) && !shown.contains(&self.screen)
        {
            shown.push(self.screen.clone());
        }
        shown
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
        self.overdue_expanded = false;
        self.reload_rows()?;
        self.cursor_to_today();
        Ok(())
    }

    /// Week opens on its first day, with the cursor on today's first row,
    /// or the first row after today when today has none.
    pub(super) fn cursor_to_today(&mut self) {
        if self.screen != Screen::Week {
            return;
        }
        let today = self.today.format("%Y-%m-%d").to_string();
        if let Some(index) = self.rows.iter().position(|row| {
            row.occurrence
                .as_ref()
                .is_some_and(|occurrence| occurrence.date >= today)
        }) {
            self.selected = index;
        }
    }

    /// The task under the cursor; None on Today's "+N more" row, so no key
    /// acts on a task it hides.
    pub(super) fn selected_row(&self) -> Option<&DisplayRow> {
        self.rows.get(self.selected).filter(|row| row.more == 0)
    }

    /// ⏎ on Today's "+N more" row shows every overdue row; returns false
    /// when the cursor is on a task.
    pub(super) fn expand_overdue(&mut self) -> Result<bool> {
        let on_more_row = self.rows.get(self.selected).is_some_and(|row| row.more > 0);
        if !on_more_row {
            return Ok(false);
        }
        self.overdue_expanded = true;
        self.reload_rows()?;
        Ok(true)
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

    /// `m`: marks or unmarks the row (on the Agents tab, the change) under
    /// the cursor and moves on.
    pub(super) fn toggle_mark(&mut self) {
        let mark = if self.screen == Screen::Review {
            self.journals
                .get(self.selected)
                .map(|record| record.id.clone())
        } else {
            self.selected_row().map(|row| row.item.item_ref.clone())
        };
        let Some(mark) = mark else {
            return;
        };
        if !self.marks.remove(&mark) {
            self.marks.insert(mark);
        }
        self.move_selection(1);
    }

    /// `*`: marks every row (on the Agents tab, every change) shown, as
    /// org-agenda-bulk-mark-all; overdue rows folded under "+N more" stay
    /// unmarked, being out of view.
    pub(super) fn mark_all(&mut self) {
        self.marks = if self.screen == Screen::Review {
            self.journals
                .iter()
                .map(|record| record.id.clone())
                .collect()
        } else {
            self.rows
                .iter()
                .filter(|row| row.more == 0)
                .map(|row| row.item.item_ref.clone())
                .collect()
        };
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
            if record.operation == Some(JournalOperation::Capture)
                && let Some(item_ref) = &record.item_ref
            {
                self.provenance.insert(
                    super::stable_ref(item_ref),
                    (record.actor_name().to_string(), record.timestamp.clone()),
                );
            }
        }
        let engine = AgendaEngine::new(&self.index);
        self.counts = engine.summary(self.today, &journals)?;
        let inbox = engine.inbox();

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
            // Filtered before OVERDUE folds, so "+N more" counts what the
            // filters let through.
            Screen::Today => today_rows(
                engine
                    .agenda(&self.query(self.day, self.day))?
                    .into_iter()
                    .filter(|entry| self.visible(&entry.item))
                    .collect(),
                &engine.todo(None, self.today)?,
                &inbox,
                &self.index,
                self.overdue_expanded,
            ),
            Screen::Week => {
                let (from, to) = AgendaSpan::Week.bounds(self.day, self.settings.week_start);
                headers.extend(day_labels(from, to));
                day_grouped(engine.agenda(&self.query(from, to))?)
            }
            Screen::Inbox => inbox
                .into_iter()
                .map(|item| DisplayRow {
                    section: INBOX.to_string(),
                    item,
                    occurrence: None,
                    more: 0,
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
                                    more: 0,
                                }));
                                rows.extend(group.items.into_iter().map(|item| DisplayRow {
                                    section: group.name.clone(),
                                    item,
                                    occurrence: None,
                                    more: 0,
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
                            headers.push(Header::agenda_block(title, Some(section.color.clone())));
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
                            more: 0,
                        }));
                    }
                }
                rows
            }
            Screen::Triage => {
                self.reschedules = reschedule_counts(&journals, &self.index);
                // Cards show each task as the files read now, in the order
                // the pass began with; a finished pass has no rows.
                for card in &mut self.triage.queue {
                    if let Some(item) = current_item(&self.index, &card.item) {
                        card.item = item;
                    }
                }
                self.rows = if self.triage.finished() {
                    Vec::new()
                } else {
                    self.triage.queue.clone()
                };
                self.headers.clear();
                return Ok(());
            }
        };
        rows.retain(|row| self.visible(&row.item));
        // Sections that only exist through their rows (Today, Inbox).
        for row in &rows {
            if !headers.iter().any(|header| header.text == row.section) {
                headers.push(Header::section(row.section.clone(), None));
            }
        }
        // An empty Today says what tomorrow holds.
        self.tomorrow = (0, 0);
        if self.screen == Screen::Today && !self.pane && rows.is_empty() {
            let tomorrow = self.today + Duration::days(1);
            let open: Vec<AgendaEntry> = engine
                .agenda(&self.query(tomorrow, tomorrow))?
                .into_iter()
                .filter(|entry| !entry.item.done)
                .collect();
            let count = |kind: OccurrenceType| {
                open.iter()
                    .filter(|entry| entry.occurrence.kind == kind)
                    .count()
            };
            self.tomorrow = (
                count(OccurrenceType::Scheduled),
                count(OccurrenceType::Deadline),
            );
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
/// first few unless expanded; DOING lists in-progress TODOs without a date
/// today.
fn today_rows(
    entries: Vec<AgendaEntry>,
    todos: &[Item],
    inbox: &[Item],
    index: &WorkspaceIndex,
    overdue_expanded: bool,
) -> Vec<DisplayRow> {
    let mut sections: [(&str, Vec<DisplayRow>); 5] = [
        (OVERDUE, Vec::new()),
        (SCHEDULE, Vec::new()),
        (DUE, Vec::new()),
        (DOING, Vec::new()),
        (INBOX, Vec::new()),
    ];
    let mut shown = BTreeSet::new();
    // OVERDUE is the core's overdue list: one row per item, in its order.
    for entry in overdue_entries(&entries) {
        shown.insert(entry.item.item_ref.clone());
        sections[0].1.push(DisplayRow {
            section: OVERDUE.to_string(),
            item: entry.item,
            occurrence: Some(entry.occurrence),
            more: 0,
        });
    }
    for entry in entries.into_iter().filter(|entry| !entry.occurrence.late) {
        let slot = if matches!(
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
            more: 0,
        });
    }
    let overdue = std::mem::take(&mut sections[0].1);
    sections[0].1 = collapse_overdue(overdue, overdue_expanded);
    // Deadlines soonest first: `days` is minus the days left, 0 on the day.
    sections[2].1.sort_by_key(|row| {
        std::cmp::Reverse(
            row.occurrence
                .as_ref()
                .map_or(0, |occurrence| occurrence.days),
        )
    });
    for item in todos {
        if !shown.contains(&item.item_ref) && is_in_progress(item, index) {
            shown.insert(item.item_ref.clone());
            sections[3].1.push(DisplayRow {
                section: DOING.to_string(),
                item: item.clone(),
                occurrence: None,
                more: 0,
            });
        }
    }
    for item in inbox {
        if shown.insert(item.item_ref.clone()) {
            sections[4].1.push(DisplayRow {
                section: INBOX.to_string(),
                item: item.clone(),
                occurrence: None,
                more: 0,
            });
        }
    }
    // Done rows stay visible, last in their section.
    for (_, rows) in &mut sections[1..] {
        rows.sort_by_key(|row| row.item.done);
    }
    sections.into_iter().flat_map(|(_, rows)| rows).collect()
}

/// Unless expanded, the first few overdue rows and a "+N more" row
/// standing for the rest. One hidden row is simply shown.
fn collapse_overdue(mut rows: Vec<DisplayRow>, expanded: bool) -> Vec<DisplayRow> {
    if expanded || rows.len() <= OVERDUE_ON_TODAY + 1 {
        return rows;
    }
    let hidden = rows.len() - OVERDUE_ON_TODAY;
    rows.truncate(OVERDUE_ON_TODAY + 1);
    if let Some(more) = rows.last_mut() {
        more.more = hidden;
    }
    rows
}

/// The pane: today's dated rows, timed ones by clock time, then untimed,
/// with done rows last; overdue rows are only counted.
fn pane_rows(entries: Vec<AgendaEntry>) -> Vec<DisplayRow> {
    let mut rows: Vec<DisplayRow> = entries
        .into_iter()
        .filter(|entry| !entry.occurrence.late)
        .map(|entry| DisplayRow {
            section: SCHEDULE.to_string(),
            item: entry.item,
            occurrence: Some(entry.occurrence),
            more: 0,
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
            *counts.entry(super::stable_ref(item_ref)).or_default() += 1;
        }
    }
    for document in index.documents() {
        for headline in document.headlines() {
            let section =
                &document.source()[headline.section_span.start..headline.section_span.end];
            let logged = section.matches("- Rescheduled from").count();
            if logged > 0 {
                counts.insert(super::stable_ref(&headline.item.item_ref), logged);
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
            counted: false,
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
                more: 0,
            }
        })
        .collect()
}

/// The task a card shows, as the files read now: by ID, else by file and
/// outline path, which state, date and tag changes leave alone.
fn current_item(index: &WorkspaceIndex, item: &Item) -> Option<Item> {
    index
        .items()
        .find(|candidate| match &item.id {
            Some(id) => candidate.id.as_ref() == Some(id),
            None => candidate.file == item.file && candidate.olpath == item.olpath,
        })
        .cloned()
}

/// The core's overdue list, in its order, then the inbox; each item once.
fn triage_rows(entries: &[AgendaEntry], inbox: Vec<Item>) -> Vec<DisplayRow> {
    let mut rows: Vec<DisplayRow> = overdue_entries(entries)
        .into_iter()
        .map(|entry| DisplayRow {
            section: OVERDUE.to_string(),
            item: entry.item,
            occurrence: Some(entry.occurrence),
            more: 0,
        })
        .collect();
    let mut seen: BTreeSet<String> = rows.iter().map(|row| row.item.item_ref.clone()).collect();
    rows.extend(
        inbox
            .into_iter()
            .filter(|item| seen.insert(item.item_ref.clone()))
            .map(|item| DisplayRow {
                section: INBOX.to_string(),
                item,
                occurrence: None,
                more: 0,
            }),
    );
    rows
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

/// Recent agent changes, unreviewed first, newest first within each group.
fn review_records(journals: Vec<JournalRecord>, agent: Option<&str>) -> Vec<JournalRecord> {
    let mut records: Vec<JournalRecord> = journals
        .into_iter()
        .filter(|record| {
            record.agent().is_some()
                && matches!(
                    record.status,
                    JournalStatus::Committed | JournalStatus::Reviewed
                )
                && agent.is_none_or(|agent| record.agent() == Some(agent))
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
