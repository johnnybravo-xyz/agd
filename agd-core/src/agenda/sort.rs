//! Agenda order: the org-agenda-sorting-strategy keys, org-agenda urgency, and
//! priority values from each file's own `#+PRIORITIES` range.
//!
//! Every sort is stable, so rows that compare equal keep collection order
//! (file, then buffer position) and the output is deterministic.

use std::cmp::Ordering;

use chrono::NaiveDate;

use crate::{AgendaEntry, Item, OccurrenceType, PriorityRange, WorkspaceIndex};

use super::engine::parse_date;
use super::export::agenda_title;

/// One comparison per org-agenda-sorting-strategy key, first difference
/// wins; the sort is stable, so category-keep and habit-* keep collection
/// order.
pub(super) fn compare_by_strategy(
    (left_urgency, left): &(i64, AgendaEntry),
    (right_urgency, right): &(i64, AgendaEntry),
    index: &WorkspaceIndex,
    todo_order: &[String],
) -> Ordering {
    let settings = index.settings();
    let todo_rank = |entry: &AgendaEntry| {
        entry.item.todo.as_deref().map_or(-1, |todo| {
            todo_order
                .iter()
                .position(|keyword| keyword == todo)
                .map_or(-1, |index| index as i64)
        })
    };
    for key in &settings.sorting_strategy {
        let (name, direction) = key.rsplit_once('-').unwrap_or((key, "up"));
        let ordering = match name {
            "time" => compare_time_of_day(&left.occurrence.time, &right.occurrence.time),
            "urgency" => left_urgency.cmp(right_urgency),
            "priority" => {
                priority_value(&left.item, index).cmp(&priority_value(&right.item, index))
            }
            "category" if direction != "keep" => left.item.category.cmp(&right.item.category),
            "alpha" => agenda_title(&left.item).cmp(&agenda_title(&right.item)),
            "todo-state" => todo_rank(left).cmp(&todo_rank(right)),
            _ => Ordering::Equal,
        };
        let ordering = if direction == "down" {
            ordering.reverse()
        } else {
            ordering
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

pub(super) fn keyword_name(keyword: &str) -> &str {
    keyword.split(['(', '!', '@']).next().unwrap_or(keyword)
}

/// org-agenda urgency: priority value (A=2000, B=1000, C=0 by default),
/// raised by days overdue for today's deadline reminders, lowered by days
/// left for upcoming ones, and raised by 99 plus days late for scheduled.
pub(super) fn urgency(entry: &AgendaEntry, today: NaiveDate, index: &WorkspaceIndex) -> i64 {
    let priority = priority_value(&entry.item, index);
    let date = parse_date(&entry.occurrence.date).ok();
    match entry.occurrence.kind {
        OccurrenceType::Deadline | OccurrenceType::UpcomingDeadline => {
            let base = entry
                .item
                .planning
                .deadline
                .as_ref()
                .and_then(|deadline| parse_date(&deadline.date).ok());
            match (date, base) {
                (Some(date), Some(base)) if date == today => priority + (today - base).num_days(),
                _ => priority,
            }
        }
        OccurrenceType::Scheduled | OccurrenceType::PastScheduled => {
            let base = entry
                .item
                .planning
                .scheduled
                .as_ref()
                .and_then(|scheduled| parse_date(&scheduled.date).ok());
            match (date, base) {
                (Some(date), Some(base)) => priority + 99 + (date - base).num_days(),
                _ => priority + 99,
            }
        }
        _ => priority,
    }
}

/// The item's priority value in its own file's range (`#+PRIORITIES`).
pub(super) fn priority_value(item: &Item, index: &WorkspaceIndex) -> i64 {
    index
        .document(std::path::Path::new(&item.file))
        .map_or_else(
            || PriorityRange::from_settings(index.settings()),
            |document| document.priorities(),
        )
        .value(item.priority.as_deref())
}

// time-up with org-agenda-sort-notime-is-late: timed entries first.
fn compare_time_of_day(left: &Option<String>, right: &Option<String>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => time_minutes(left).cmp(&time_minutes(right)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn time_minutes(time: &str) -> u32 {
    let (hours, minutes) = time.split_once(':').unwrap_or((time, "0"));
    hours.parse::<u32>().unwrap_or(0) * 60 + minutes.parse::<u32>().unwrap_or(0)
}

// org-agenda TODO lists: urgency-down category-keep, i.e. priority first and
// otherwise file and buffer order.
pub(super) fn compare_items(left: &Item, right: &Item, index: &WorkspaceIndex) -> Ordering {
    priority_value(right, index)
        .cmp(&priority_value(left, index))
        .then_with(|| left.file.cmp(&right.file))
        .then_with(|| left.line.cmp(&right.line))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agenda::test_support::fixture;
    use crate::{AgendaEngine, Settings};

    #[test]
    fn todo_lists_sort_by_each_files_own_priority_range() {
        // agenda.org: no cookie means its default, E, the lowest; later.org
        // keeps the configured default B.
        let (temp, _) = fixture("#+PRIORITIES: A E E\n* TODO Default lowest\n* TODO [#D] Fourth\n");
        std::fs::write(temp.path().join("later.org"), "* TODO Default middle\n").unwrap();
        let index = WorkspaceIndex::load(Settings {
            roots: vec![temp.path().to_string_lossy().into_owned()],
            agenda_files: vec!["**/*.org".to_string()],
            ..Settings::default()
        })
        .unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        let titles: Vec<String> = AgendaEngine::new(&index)
            .todo(None, today)
            .unwrap()
            .into_iter()
            .map(|item| item.title)
            .collect();
        // D of A-E and B of A-C are both one step above their lowest.
        assert_eq!(titles, ["Fourth", "Default middle", "Default lowest"]);
    }
}
