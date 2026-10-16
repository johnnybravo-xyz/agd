use std::collections::BTreeSet;
use std::sync::LazyLock;

use chrono::{Datelike, NaiveDate};
use regex::Regex;
use unicode_width::UnicodeWidthStr;

use crate::{AgendaEntry, Item, Occurrence, OccurrenceType, WeekStart, WorkspaceIndex};

use super::engine::{date_range, parse_date};
use super::sort::priority_value;
use super::span::AgendaSpan;

static ACTIVE_TIMESTAMP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[ \t]*<\d{4}-\d{2}-\d{2}[^>\n]*>").expect("active timestamp regex")
});

/// Headline text as org-agenda prints it: active timestamps in the title,
/// such as `Birthday <1993-10-16 Sat +1y>`, are left out.
pub fn agenda_title(item: &Item) -> String {
    ACTIVE_TIMESTAMP_RE
        .replace_all(&item.title, "")
        .trim()
        .to_string()
}

fn occurrence_name(kind: OccurrenceType) -> &'static str {
    match kind {
        OccurrenceType::Scheduled => "scheduled",
        OccurrenceType::PastScheduled => "past-scheduled",
        OccurrenceType::Deadline => "deadline",
        OccurrenceType::UpcomingDeadline => "upcoming-deadline",
        OccurrenceType::Timestamp => "timestamp",
        OccurrenceType::Block => "block",
        OccurrenceType::Closed => "closed",
        OccurrenceType::Todo => "todo",
    }
}

// Emacs's export layouts, checked against pinned Emacs in conformance/agenda:
// the rows of `org-batch-agenda-csv` and the text `org-batch-agenda` prints.

/// An active timestamp in a headline, which a timestamp row leaves out.
static HEADLINE_TIMESTAMP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<\d{4}-\d{2}-\d{2}[^>\n]*>").expect("headline timestamp regex"));

/// Agenda rows in Emacs's `org-batch-agenda-csv` layout: all eleven columns
/// (category, head, type, todo, tags, date, time, extra, priority-l,
/// priority-n, agenda-day) with Emacs's values. As in Emacs, a field's
/// commas become semicolons and nothing is quoted. `today` decides which
/// scheduled rows Emacs calls past-scheduled.
pub fn agenda_csv(entries: &[AgendaEntry], today: NaiveDate, index: &WorkspaceIndex) -> String {
    let mut output = String::new();
    for entry in entries {
        let item = &entry.item;
        let (kind, date) = csv_type_and_date(entry, today);
        // Org gives every closed row the same priority-n.
        let priority = match entry.occurrence.kind {
            OccurrenceType::Closed => 100_000,
            _ => priority_value(item, index),
        };
        push_csv_row(
            &mut output,
            [
                &item.category,
                &row_head(entry),
                kind,
                item.todo.as_deref().unwrap_or_default(),
                &all_tags(item).join(":"),
                &date,
                &agenda_time(&entry.occurrence),
                &entry.occurrence.extra,
                item.priority.as_deref().unwrap_or_default(),
                &priority.to_string(),
                &entry.occurrence.date,
            ],
        );
    }
    output
}

/// A TODO-list or tags block's rows in the same layout: type `tagsmatch`
/// for a `tags` or `tags-todo` block and `todo` for the others, with no
/// date, time, extra or agenda day.
pub fn items_csv(items: &[Item], block_kind: &str, index: &WorkspaceIndex) -> String {
    let kind = match block_kind {
        "tags" | "tags-todo" => "tagsmatch",
        _ => "todo",
    };
    let mut output = String::new();
    for item in items {
        push_csv_row(
            &mut output,
            [
                &item.category,
                &item.title,
                kind,
                item.todo.as_deref().unwrap_or_default(),
                &all_tags(item).join(":"),
                "",
                "",
                "",
                item.priority.as_deref().unwrap_or_default(),
                &priority_value(item, index).to_string(),
                "",
            ],
        );
    }
    output
}

/// Emacs's type and date columns for a row. Org calls every row of an entry
/// scheduled before today `past-scheduled` and dates it on its schedule; a
/// deadline row is dated on its deadline; other rows on their agenda day.
fn csv_type_and_date(entry: &AgendaEntry, today: NaiveDate) -> (&'static str, String) {
    let planning = &entry.item.planning;
    let day = entry.occurrence.date.clone();
    match entry.occurrence.kind {
        OccurrenceType::Scheduled | OccurrenceType::PastScheduled => {
            match planning.scheduled.as_ref().map(|scheduled| &scheduled.date) {
                Some(base) if parse_date(base).is_ok_and(|base| base < today) => {
                    ("past-scheduled", base.clone())
                }
                _ => ("scheduled", day),
            }
        }
        OccurrenceType::Deadline => (
            "deadline",
            planning
                .deadline
                .as_ref()
                .map_or(day, |deadline| deadline.date.clone()),
        ),
        kind => (occurrence_name(kind), day),
    }
}

/// The headline text Emacs shows on a row: a timestamp row leaves out the
/// headline's active timestamps (`Birthday <1993-10-16 Sat +1y>`) and keeps
/// the text around them; every other row shows the headline as written.
fn row_head(entry: &AgendaEntry) -> String {
    match entry.occurrence.kind {
        OccurrenceType::Timestamp => HEADLINE_TIMESTAMP_RE
            .replace_all(&entry.item.title, "")
            .into_owned(),
        _ => entry.item.title.clone(),
    }
}

/// A row's time as org-agenda prints it with org-agenda-time-leading-zero
/// nil: ` 9:00......`, or ` 9:00-10:30` for a range; empty without a time.
fn agenda_time(occurrence: &Occurrence) -> String {
    let Some(start) = occurrence.time.as_deref() else {
        return String::new();
    };
    match occurrence.end_time.as_deref() {
        Some(end) => format!("{:>5}-{}", clock(start), clock(end)),
        None => format!("{:>5}......", clock(start)),
    }
}

/// `09:30` as Org writes it in the agenda: `9:30`.
fn clock(time: &str) -> &str {
    match time.strip_prefix('0') {
        Some(rest) if rest.starts_with(|c: char| c.is_ascii_digit()) => rest,
        _ => time,
    }
}

/// Inherited tags, then the headline's own, each once.
fn all_tags(item: &Item) -> Vec<&str> {
    let mut seen = BTreeSet::new();
    item.inherited_tags
        .iter()
        .chain(&item.tags)
        .filter(|tag| seen.insert(tag.as_str()))
        .map(String::as_str)
        .collect()
}

/// org-agenda-export-csv-mapper: commas become semicolons, ends trimmed.
fn push_csv_row(output: &mut String, fields: [&str; 11]) {
    let fields = fields.map(|field| field.replace(',', ";").trim().to_string());
    output.push_str(&fields.join(","));
    output.push('\n');
}

/// The column tags end at: org-agenda-tags-column `auto` right-aligns them
/// to the window, and `org-batch-agenda`'s window is 79 columns wide.
const TAGS_COLUMN: usize = 79;

/// The agenda from `from` to `to` as `org-batch-agenda` prints it with the
/// time grid off: `header`, or Org's span line such as `Week-agenda (W42):`,
/// then every day with its rows (org-agenda-show-all-dates).
pub fn agenda_text(
    entries: &[AgendaEntry],
    from: NaiveDate,
    to: NaiveDate,
    header: Option<&str>,
) -> String {
    let mut output = match header {
        Some(header) => format!("{header}\n"),
        None => span_header(from, to),
    };
    for day in date_range(from, to) {
        output.push_str(&day_line(day));
        let date = day.format("%Y-%m-%d").to_string();
        for entry in entries.iter().filter(|entry| entry.occurrence.date == date) {
            output.push_str(&agenda_row(entry));
            output.push('\n');
        }
    }
    output
}

/// One agenda row as org-agenda prints it: `  category:`, the time, the
/// leader (`Scheduled:`, `In   3 d.:`), TODO, priority, headline and tags.
pub fn agenda_row(entry: &AgendaEntry) -> String {
    let mut line = format!("  {:<12}", format!("{}:", entry.item.category));
    let time = agenda_time(&entry.occurrence);
    if !time.is_empty() {
        line.push_str(&format!("{time:<12}"));
    }
    let leader = agenda_leader(&entry.occurrence.extra);
    if !leader.is_empty() {
        line.push_str(&leader);
        line.push(' ');
    }
    push_headline(&mut line, &entry.item, &row_head(entry));
    with_tags(line, &entry.item)
}

/// One row of a TODO list or tags match: `  category:`, TODO, priority,
/// headline and tags.
pub fn item_row(item: &Item) -> String {
    let mut line = format!("  {:<12}", format!("{}:", item.category));
    push_headline(&mut line, item, &item.title);
    with_tags(line, item)
}

/// `Day-agenda (W42):`, `Week-agenda (W42):`, `Month-agenda (W40-W44):` or
/// `12 days-agenda (W41-W43):`: Org names spans of 1, 7 and 14 days and the
/// month span, and counts the days of any other.
fn span_header(from: NaiveDate, to: NaiveDate) -> String {
    let days = (to - from).num_days() + 1;
    let name = match days {
        1 => "Day".to_string(),
        7 => "Week".to_string(),
        14 => "Fortnight".to_string(),
        _ if AgendaSpan::Month.bounds(from, WeekStart::Monday) == (from, to) => "Month".to_string(),
        _ => format!("{days} days"),
    };
    let (first, last) = (from.iso_week().week(), to.iso_week().week());
    let weeks = if first == last {
        format!("W{first:02}")
    } else {
        format!("W{first:02}-W{last:02}")
    };
    format!("{name}-agenda ({weeks}):\n")
}

/// org-agenda-format-date-aligned: `Monday      5 October 2026 W41`, the
/// ISO week on Mondays.
fn day_line(day: NaiveDate) -> String {
    let week = if day.weekday() == chrono::Weekday::Mon {
        format!(" W{:02}", day.iso_week().week())
    } else {
        String::new()
    };
    format!(
        "{:<10} {:>2} {} {}{week}\n",
        day.format("%A"),
        day.day(),
        day.format("%B"),
        day.year()
    )
}

/// Org's default leader for a row's `extra` (org-agenda-scheduled-leaders,
/// org-agenda-deadline-leaders and the log mode's `Closed:`), padded as
/// Emacs prints it; `extra` itself is trimmed, as in Emacs's CSV.
fn agenda_leader(extra: &str) -> String {
    match extra {
        "" => String::new(),
        "Deadline:" => "Deadline:  ".to_string(),
        "Closed:" => "Closed:    ".to_string(),
        _ if extra.ends_with(" d. ago:") => format!("{extra:>10} "),
        _ => format!("{extra} "),
    }
}

fn push_headline(line: &mut String, item: &Item, head: &str) {
    if let Some(todo) = &item.todo {
        line.push_str(todo);
        line.push(' ');
    }
    if let Some(priority) = &item.priority {
        line.push_str(&format!("[#{priority}] "));
    }
    line.push_str(head);
}

/// Appends the row's tags ending at TAGS_COLUMN, at least one space after
/// the text, as org-agenda-align-tags does; display width counts wide
/// characters twice.
fn with_tags(line: String, item: &Item) -> String {
    let Some(tags) = agenda_tags(item) else {
        return line;
    };
    let text = line.trim_end();
    let used = text.width() + tags.width();
    let padding = TAGS_COLUMN.saturating_sub(used).max(1);
    format!("{text}{}{tags}", " ".repeat(padding))
}

/// Tags as org-agenda shows them, inherited ones first: `:own:`,
/// `:file:parent::own:`, or `:file::` when every tag is inherited.
fn agenda_tags(item: &Item) -> Option<String> {
    let inherited = item.inherited_tags.join(":");
    let own = item.tags.join(":");
    match (inherited.is_empty(), own.is_empty()) {
        (true, true) => None,
        (true, false) => Some(format!(":{own}:")),
        (false, true) => Some(format!(":{inherited}::")),
        (false, false) => Some(format!(":{inherited}::{own}:")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agenda::test_support::fixture;
    use crate::{AgendaEngine, AgendaQuery};

    #[test]
    fn csv_writes_emacs_columns_and_values() {
        let (_temp, index) = fixture(
            "* TODO Call, bank\nSCHEDULED: <2026-10-13 Tue 09:00-10:30>\n* TODO Old\nSCHEDULED: <2026-10-10 Sat>\n",
        );
        let today = NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        let entries = AgendaEngine::new(&index)
            .agenda(&AgendaQuery {
                from: today,
                to: today,
                today,
                match_expression: None,
                log_mode: false,
            })
            .unwrap();
        let rows = agenda_csv(&entries, today, &index);
        let category = &entries[0].item.category;
        assert_eq!(
            rows,
            format!(
                "{category},Call; bank,scheduled,TODO,,2026-10-13,9:00-10:30,Scheduled:,,1000,2026-10-13\n\
                 {category},Old,past-scheduled,TODO,,2026-10-10,,Sched. 3x:,,1000,2026-10-13\n"
            )
        );
    }
}
