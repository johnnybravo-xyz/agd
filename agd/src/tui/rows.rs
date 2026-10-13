use std::collections::BTreeSet;
use std::time::Instant;

use super::{AgendaView, App, DisplayRow, Screen, SelectionAnchor};
use anyhow::Result;
use chrono::{Duration, Local};
use org_core::{
    AgendaEngine, AgendaEntry, AgendaQuery, AgendaSpan, Item, JournalStatus, Occurrence,
    OccurrenceType, WriteEngine,
};

pub(super) fn period_duration(view: AgendaView) -> Duration {
    match view {
        AgendaView::Day | AgendaView::Todo => Duration::days(1),
        AgendaView::Week => Duration::weeks(1),
    }
}

pub(super) fn section_for(entry: &AgendaEntry) -> String {
    if matches!(entry.occurrence.kind, OccurrenceType::PastScheduled) {
        "SLIPPED"
    } else if entry.occurrence.time.is_some() {
        "SCHEDULE"
    } else if matches!(
        entry.occurrence.kind,
        OccurrenceType::Deadline | OccurrenceType::UpcomingDeadline
    ) {
        "DUE SOON"
    } else if matches!(entry.item.todo.as_deref(), Some("NEXT" | "WAIT")) {
        "IN PROGRESS"
    } else if is_inbox(&entry.item) {
        "INBOX"
    } else {
        "TODAY"
    }
    .to_string()
}

pub(super) fn section_order(section: &str) -> u8 {
    match section {
        "SLIPPED" => 0,
        "SCHEDULE" => 1,
        "DUE SOON" => 2,
        "IN PROGRESS" => 3,
        "INBOX" => 4,
        _ => 5,
    }
}

pub(super) fn is_inbox(item: &Item) -> bool {
    item.olpath
        .iter()
        .any(|heading| heading.eq_ignore_ascii_case("inbox"))
        || item.category.eq_ignore_ascii_case("inbox")
}

pub(super) fn occurrence_prefix(occurrence: Option<&Occurrence>) -> String {
    let Some(occurrence) = occurrence else {
        return String::new();
    };
    if !occurrence.extra.is_empty() {
        return occurrence.extra.clone();
    }
    match occurrence.kind {
        OccurrenceType::PastScheduled => "Sched.:",
        OccurrenceType::Scheduled => "Scheduled:",
        OccurrenceType::Deadline => "Deadline:",
        OccurrenceType::UpcomingDeadline => "In:",
        OccurrenceType::Timestamp => "",
        OccurrenceType::Block => "Block:",
        OccurrenceType::Closed => "Closed:",
        OccurrenceType::Todo => "",
    }
    .to_string()
}

pub(super) fn view_title(app: &App) -> String {
    match app.screen {
        Screen::Triage => format!("Triage · {} remaining", app.rows.len()),
        Screen::Review => "Agent review".to_string(),
        Screen::Today => match app.view {
            AgendaView::Day => format!("Today · {}", app.day.format("%a %d %b %Y")),
            AgendaView::Week => format!("Week · {}", app.day.format("%d %b %Y")),
            AgendaView::Todo => "Todo".to_string(),
        },
    }
}

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

    /// Changes screen with the cursor on the first row.
    pub(super) fn switch_screen(&mut self, screen: Screen) -> Result<()> {
        self.screen = screen;
        self.rows.clear();
        self.selected = 0;
        self.reload_rows()
    }

    fn rebuild_rows(&mut self) -> Result<()> {
        self.inbox_count = self
            .index
            .items()
            .filter(|item| item.todo.is_some() && !item.done && is_inbox(item))
            .count();
        let journals = WriteEngine::with_default_journal(self.settings.clone())?.journal()?;
        self.provenance.clear();
        for record in &journals {
            if matches!(record.status, JournalStatus::Committed)
                && let Some(item_ref) = &record.item_ref
            {
                self.provenance
                    .insert(item_ref.clone(), record.actor.clone());
            }
        }
        self.agent_count = journals
            .iter()
            .filter(|record| {
                matches!(record.status, JournalStatus::Committed)
                    && record.actor.starts_with("mcp:")
            })
            .count();
        self.reschedule_counts.clear();
        for record in &journals {
            if matches!(record.status, JournalStatus::Committed)
                && record.diff.contains("SCHEDULED:")
                && let Some(item_ref) = &record.item_ref
            {
                *self.reschedule_counts.entry(item_ref.clone()).or_default() += 1;
            }
        }
        if self.screen == Screen::Review {
            self.journals = journals
                .into_iter()
                .filter(|record| {
                    matches!(record.status, JournalStatus::Committed)
                        && record.actor.starts_with("mcp:")
                })
                .collect();
            return Ok(());
        }
        let today = Local::now().date_naive();
        let engine = AgendaEngine::new(&self.index);
        let mut rows = if self.screen == Screen::Triage {
            let entries = engine.agenda(&AgendaQuery {
                from: today - Duration::days(3650),
                to: today,
                today,
                match_expression: None,
                log_mode: false,
            })?;
            let mut seen = BTreeSet::new();
            let mut rows: Vec<DisplayRow> = entries
                .into_iter()
                .filter(|entry| {
                    matches!(entry.occurrence.kind, OccurrenceType::PastScheduled)
                        || is_inbox(&entry.item)
                })
                .filter(|entry| !entry.item.done && seen.insert(entry.item.item_ref.clone()))
                .map(|entry| DisplayRow {
                    section: if is_inbox(&entry.item) {
                        "INBOX"
                    } else {
                        "SLIPPED"
                    }
                    .to_string(),
                    item: entry.item,
                    occurrence: Some(entry.occurrence),
                })
                .collect();
            for item in engine.todo(None)?.into_iter().filter(is_inbox) {
                if seen.insert(item.item_ref.clone()) {
                    rows.push(DisplayRow {
                        section: "INBOX".to_string(),
                        item,
                        occurrence: None,
                    });
                }
            }
            rows
        } else if self.view == AgendaView::Todo {
            engine
                .todo(None)?
                .into_iter()
                .map(|item| DisplayRow {
                    section: "TODO".to_string(),
                    item,
                    occurrence: None,
                })
                .collect()
        } else {
            let span = if self.view == AgendaView::Week {
                AgendaSpan::Week
            } else {
                AgendaSpan::Day
            };
            let (from, to) = span.bounds(self.day, &self.settings.agenda.week_start);
            let mut seen = BTreeSet::new();
            let mut rows: Vec<DisplayRow> = engine
                .agenda(&AgendaQuery {
                    from,
                    to,
                    today,
                    match_expression: None,
                    log_mode: false,
                })?
                .into_iter()
                .map(|entry| DisplayRow {
                    section: section_for(&entry),
                    item: entry.item,
                    occurrence: Some(entry.occurrence),
                })
                .inspect(|row| {
                    seen.insert(row.item.item_ref.clone());
                })
                .collect();
            if self.view == AgendaView::Day {
                for item in engine.todo(None)?.into_iter().filter(is_inbox) {
                    if seen.insert(item.item_ref.clone()) {
                        rows.push(DisplayRow {
                            section: "INBOX".to_string(),
                            item,
                            occurrence: None,
                        });
                    }
                }
            }
            rows
        };
        let context = ["all", "work", "personal"][self.context];
        rows.retain(|row| {
            (context == "all" || row.item.category.eq_ignore_ascii_case(context))
                && (self.filter.is_empty()
                    || row
                        .item
                        .title
                        .to_lowercase()
                        .contains(&self.filter.to_lowercase())
                    || row
                        .item
                        .tags
                        .iter()
                        .any(|tag| tag.eq_ignore_ascii_case(&self.filter)))
        });
        if self.screen == Screen::Today {
            self.slipped_count = rows.iter().filter(|row| row.section == "SLIPPED").count();
        }
        if !self.classic && self.screen == Screen::Today {
            let mut slipped = 0;
            rows.retain(|row| {
                if row.section == "SLIPPED" {
                    slipped += 1;
                    slipped <= 3
                } else {
                    true
                }
            });
            rows.sort_by_key(|row| section_order(&row.section));
        }
        self.rows = rows;
        Ok(())
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
        if count == 0 {
            self.selected = 0;
        } else {
            self.selected = (self.selected as isize + delta).rem_euclid(count as isize) as usize;
        }
    }

    pub(super) fn set_status(&mut self, status: String) {
        self.status = status;
        self.status_at = Instant::now();
    }

    pub(super) fn toggle_mark(&mut self) {
        let Some(item_ref) = self.selected_row().map(|row| row.item.item_ref.clone()) else {
            return;
        };
        if !self.marks.remove(&item_ref) {
            self.marks.insert(item_ref);
        }
    }

    pub(super) fn targets(&self) -> Vec<String> {
        if self.marks.is_empty() {
            self.selected_row()
                .map(|row| vec![row.item.item_ref.clone()])
                .unwrap_or_default()
        } else {
            self.marks.iter().cloned().collect()
        }
    }
}
