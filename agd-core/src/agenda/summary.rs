//! The day at a glance, one answer for every surface: the overdue list that
//! Today, Triage and both panes show ([`overdue_entries`]), and the counts of the
//! status band (`agd agenda --status`, the TUI's header and pane), each
//! counting an item once ([`AgendaSummary`]).

use std::cmp::Reverse;
use std::collections::BTreeSet;

use chrono::NaiveDate;
use serde::Serialize;

use crate::{AgdError, AgendaEntry, AgendaQuery, JournalRecord, JournalStatus, OccurrenceType};

use super::engine::AgendaEngine;

/// The overdue rows among `entries` (a day's agenda), one per item: missed
/// deadlines first, most days late first, then past-scheduled reminders,
/// oldest first. Rows of equal lateness keep the agenda's order. Done
/// items are never overdue.
pub fn overdue_entries(entries: &[AgendaEntry]) -> Vec<AgendaEntry> {
    let mut late: Vec<&AgendaEntry> = entries
        .iter()
        .filter(|entry| entry.occurrence.late && !entry.item.done)
        .collect();
    late.sort_by_key(|entry| {
        (
            entry.occurrence.kind != OccurrenceType::Deadline,
            Reverse(entry.occurrence.days),
        )
    });
    let mut seen = BTreeSet::new();
    late.into_iter()
        .filter(|entry| seen.insert(entry.item.item_ref.as_str()))
        .cloned()
        .collect()
}

/// What the status band counts, each item once.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct AgendaSummary {
    /// Items in the overdue list.
    pub overdue: usize,
    /// Other open items scheduled, due or timestamped today; a deadline's
    /// warning (`In 3 d.:`) is not today's.
    pub today: usize,
    /// Items in the inbox, where captures land.
    pub inbox: usize,
    /// Agent changes not yet reviewed.
    pub unreviewed: usize,
}

impl AgendaSummary {
    /// `1 overdue · 3 today · 9 inbox · 2 unreviewed`, leaving out zero
    /// counts; empty when every count is zero.
    pub fn line(&self) -> String {
        [
            (self.overdue, "overdue"),
            (self.today, "today"),
            (self.inbox, "inbox"),
            (self.unreviewed, "unreviewed"),
        ]
        .iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, label)| format!("{count} {label}"))
        .collect::<Vec<_>>()
        .join(" · ")
    }
}

impl AgendaEngine<'_> {
    /// The status band's counts for `today`, with unreviewed agent changes
    /// read from `journal` (`WriteEngine::journal`).
    pub fn summary(
        &self,
        today: NaiveDate,
        journal: &[JournalRecord],
    ) -> Result<AgendaSummary, AgdError> {
        let entries = self.agenda(&AgendaQuery {
            from: today,
            to: today,
            today,
            match_expression: None,
            log_mode: false,
        })?;
        let overdue = overdue_entries(&entries);
        let mut counted: BTreeSet<&str> = overdue
            .iter()
            .map(|entry| entry.item.item_ref.as_str())
            .collect();
        let today_items = entries
            .iter()
            .filter(|entry| {
                !entry.item.done && entry.occurrence.kind != OccurrenceType::UpcomingDeadline
            })
            .filter(|entry| counted.insert(entry.item.item_ref.as_str()))
            .count();
        let unreviewed = journal
            .iter()
            .filter(|record| record.agent().is_some() && record.status == JournalStatus::Committed)
            .count();
        Ok(AgendaSummary {
            overdue: overdue.len(),
            today: today_items,
            inbox: self.inbox().len(),
            unreviewed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agenda::test_support::fixture;

    #[test]
    fn overdue_lists_missed_deadlines_then_old_schedules_each_item_once() {
        let (_temp, index) = fixture(
            "* TODO Scheduled long ago\nSCHEDULED: <2026-09-01 Tue>\n\
* TODO Deadline missed by two\nDEADLINE: <2026-10-11 Sun>\n\
* TODO Both, deadline missed by five\nSCHEDULED: <2026-10-01 Thu> DEADLINE: <2026-10-08 Thu>\n\
* TODO Scheduled yesterday\nSCHEDULED: <2026-10-12 Mon>\n\
* DONE Finished late\nSCHEDULED: <2026-10-01 Thu>\n\
* TODO Due soon\nDEADLINE: <2026-10-16 Fri>\n\
* TODO Today\nSCHEDULED: <2026-10-13 Tue>\n",
        );
        let today = NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        let engine = AgendaEngine::new(&index);
        let entries = engine
            .agenda(&AgendaQuery {
                from: today,
                to: today,
                today,
                match_expression: None,
                log_mode: false,
            })
            .unwrap();
        let rows: Vec<(String, i64)> = overdue_entries(&entries)
            .into_iter()
            .map(|entry| (entry.item.title, entry.occurrence.days))
            .collect();
        let expected = [
            ("Both, deadline missed by five", 5),
            ("Deadline missed by two", 2),
            ("Scheduled long ago", 42),
            ("Scheduled yesterday", 1),
        ]
        .map(|(title, days)| (title.to_string(), days));
        assert_eq!(rows, expected);
        let due_soon = entries
            .iter()
            .find(|entry| entry.item.title == "Due soon")
            .unwrap();
        assert_eq!(
            (due_soon.occurrence.days, due_soon.occurrence.late),
            (-3, false)
        );

        let record = |actor: &str, status: JournalStatus| JournalRecord {
            schema: crate::JOURNAL_SCHEMA.to_string(),
            id: format!("{actor}-{status:?}"),
            status,
            timestamp: "2026-10-13T09:00:00+00:00".to_string(),
            actor: actor.to_string(),
            project: "test".to_string(),
            file: "agenda.org".to_string(),
            item_ref: None,
            transaction: None,
            operation: Some(crate::JournalOperation::Capture),
            pre_hash: String::new(),
            post_hash: String::new(),
            patches: Vec::new(),
            diff: String::new(),
        };
        let journal = [
            record("mcp:codex", JournalStatus::Committed),
            record("mcp:codex", JournalStatus::Reviewed),
            record("cli", JournalStatus::Committed),
        ];
        let summary = engine.summary(today, &journal).unwrap();
        assert_eq!(
            summary,
            AgendaSummary {
                overdue: 4,
                today: 1,
                inbox: 0,
                unreviewed: 1,
            }
        );
        assert_eq!(summary.line(), "4 overdue · 1 today · 1 unreviewed");
        assert_eq!(AgendaSummary::default().line(), "");
    }
}
