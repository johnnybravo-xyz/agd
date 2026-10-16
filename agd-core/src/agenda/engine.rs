use chrono::{Duration, NaiveDate};

use crate::{
    AGENDA_ENTRY_SCHEMA, AgdError, AgendaEntry, HeadlineSpan, Item, Occurrence, OccurrenceType,
    WorkspaceIndex,
};

use super::matcher::{TagTodoMatcher, matcher_now};
use super::repeat::{planning_dates, repeated_dates};
use super::sort::{compare_by_strategy, compare_items, keyword_name, urgency};

#[derive(Clone, Debug)]
pub struct AgendaQuery {
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub today: NaiveDate,
    pub match_expression: Option<String>,
    /// org-agenda log mode (`l`): also list CLOSED entries.
    pub log_mode: bool,
}

#[derive(Clone, Debug)]
pub struct AgendaEngine<'a> {
    pub(super) index: &'a WorkspaceIndex,
}

impl<'a> AgendaEngine<'a> {
    pub fn new(index: &'a WorkspaceIndex) -> Self {
        Self { index }
    }

    pub fn agenda(&self, query: &AgendaQuery) -> Result<Vec<AgendaEntry>, AgdError> {
        if query.to < query.from {
            return Err(AgdError::new(
                "invalid_range",
                "agenda end date is before its start date",
            ));
        }
        let matcher = query
            .match_expression
            .as_deref()
            .map(TagTodoMatcher::parse)
            .transpose()?;
        let now = matcher_now(query.today);
        let settings = self.index.settings();
        // org-agenda collects each file's deadlines, then scheduled entries,
        // then blocks and timestamps, each in buffer order, and stable-sorts
        // the day with its default strategy (time-up urgency-down
        // category-keep, where category-keep keeps collection order).
        let mut entries = Vec::new();
        for document in self.index.documents() {
            let mut file_entries = Vec::new();
            for headline in document.headlines() {
                if matcher
                    .as_ref()
                    .is_some_and(|matcher| !matcher.matches(headline, now, settings))
                {
                    continue;
                }
                let occurrences = self.occurrences(headline, query).map_err(|error| {
                    AgdError::new(
                        error.code().to_string(),
                        format!("{}:{}: {error}", headline.item.file, headline.item.line),
                    )
                })?;
                file_entries.extend(occurrences);
            }
            file_entries.sort_by_key(|entry| collection_rank(entry.occurrence.kind));
            entries.extend(file_entries);
        }
        let mut keyed: Vec<(i64, AgendaEntry)> = entries
            .into_iter()
            .map(|entry| (urgency(&entry, query.today, self.index), entry))
            .collect();
        let todo_order: Vec<String> = self
            .index
            .settings()
            .todo_keywords
            .iter()
            .flat_map(|line| line.split_whitespace())
            .filter(|keyword| *keyword != "|")
            .map(|keyword| keyword_name(keyword).to_string())
            .collect();
        keyed.sort_by(|left, right| {
            left.1
                .occurrence
                .date
                .cmp(&right.1.occurrence.date)
                .then_with(|| compare_by_strategy(left, right, self.index, &todo_order))
        });
        Ok(keyed.into_iter().map(|(_, entry)| entry).collect())
    }

    pub fn todo(
        &self,
        match_expression: Option<&str>,
        today: NaiveDate,
    ) -> Result<Vec<Item>, AgdError> {
        let matcher = match_expression.map(TagTodoMatcher::parse).transpose()?;
        let now = matcher_now(today);
        let settings = self.index.settings();
        let mut items: Vec<Item> = self
            .headlines()
            .filter(|headline| headline.item.todo.is_some() && !headline.item.done)
            .filter(|headline| {
                matcher
                    .as_ref()
                    .is_none_or(|matcher| matcher.matches(headline, now, settings))
            })
            .map(|headline| headline.item.clone())
            .collect();
        items.sort_by(|left, right| compare_items(left, right, self.index));
        Ok(items)
    }

    /// Unfinished headings in an inbox: under a heading named Inbox, or in a
    /// file or category named inbox. Containers without a TODO keyword that have
    /// children are left out.
    pub fn inbox(&self) -> Vec<Item> {
        let mut items = Vec::new();
        for document in self.index.documents() {
            let in_inbox_file = document
                .path()
                .file_stem()
                .is_some_and(|stem| stem.eq_ignore_ascii_case("inbox"));
            for headline in document.headlines() {
                let item = &headline.item;
                let ancestors = &item.olpath[..item.olpath.len().saturating_sub(1)];
                let under_inbox = ancestors
                    .iter()
                    .any(|heading| heading.eq_ignore_ascii_case("inbox"));
                let leaf = headline.subtree_span.end == headline.section_span.end;
                // Top-level headings without a keyword in an inbox file are
                // its sections (Tasks, Notes, Bookmarks), not items.
                let container = in_inbox_file && headline.level == 1 && item.todo.is_none();
                if !item.done
                    && !container
                    && (under_inbox || in_inbox_file || item.category.eq_ignore_ascii_case("inbox"))
                    && (item.todo.is_some() || leaf)
                    && !item.title.eq_ignore_ascii_case("inbox")
                {
                    items.push(item.clone());
                }
            }
        }
        items
    }

    /// Every heading matching an Emacs tags match, done or not, like an
    /// org-agenda `tags` block.
    pub fn tags(&self, match_expression: &str, today: NaiveDate) -> Result<Vec<Item>, AgdError> {
        let matcher = TagTodoMatcher::parse(match_expression)?;
        let now = matcher_now(today);
        let settings = self.index.settings();
        let mut items: Vec<Item> = self
            .headlines()
            .filter(|headline| matcher.matches(headline, now, settings))
            .map(|headline| headline.item.clone())
            .collect();
        items.sort_by(|left, right| compare_items(left, right, self.index));
        Ok(items)
    }

    pub(super) fn headlines(&self) -> impl Iterator<Item = &HeadlineSpan> {
        self.index
            .documents()
            .flat_map(|document| document.headlines().iter())
    }

    fn occurrences(
        &self,
        headline: &HeadlineSpan,
        query: &AgendaQuery,
    ) -> Result<Vec<AgendaEntry>, AgdError> {
        let (from, to, today) = (query.from, query.to, query.today);
        let item = &headline.item;
        let settings = self.index.settings();
        let mut entries = Vec::new();

        // Scheduled and deadline expansion mirrors org-agenda-get-scheduled
        // and org-agenda-get-deadlines with Org's defaults: an entry shows on
        // its base date and on future repeats, and today carries the
        // past-scheduled or overdue reminder counted from the base date.
        if !(item.done && settings.skip_scheduled_if_done)
            && let Some(scheduled) = &item.planning.scheduled
        {
            let base = parse_date(&scheduled.date)?;
            for date in planning_dates(scheduled, from, to, today, item.done)? {
                entries.push(entry(
                    item,
                    OccurrenceType::Scheduled,
                    date,
                    (scheduled.time.clone(), scheduled.end_time.clone()),
                    "Scheduled:".to_string(),
                ));
            }
            if !item.done && base < today && today >= from && today <= to {
                entries.push(entry(
                    item,
                    OccurrenceType::PastScheduled,
                    today,
                    (None, None),
                    format!("Sched.{:>2}x:", (today - base).num_days()),
                ));
            }
        }

        if !(item.done && settings.skip_deadline_if_done)
            && let Some(deadline) = &item.planning.deadline
        {
            let base = parse_date(&deadline.date)?;
            for date in planning_dates(deadline, from, to, today, item.done)? {
                entries.push(entry(
                    item,
                    OccurrenceType::Deadline,
                    date,
                    (deadline.time.clone(), deadline.end_time.clone()),
                    "Deadline:".to_string(),
                ));
            }
            let warning_days = deadline
                .warning
                .as_deref()
                .and_then(parse_day_interval)
                .unwrap_or(settings.deadline_warning_days)
                .max(0);
            if !item.done && today >= from && today <= to {
                if base < today {
                    entries.push(entry(
                        item,
                        OccurrenceType::Deadline,
                        today,
                        (None, None),
                        format!("{} d. ago:", (today - base).num_days()),
                    ));
                } else if base > today && (base - today).num_days() <= warning_days {
                    entries.push(entry(
                        item,
                        OccurrenceType::UpcomingDeadline,
                        today,
                        (None, None),
                        format!("In {:>3} d.:", (base - today).num_days()),
                    ));
                }
            }
        }

        if query.log_mode
            && let Some(closed) = &item.planning.closed
        {
            let date = parse_date(&closed.date)?;
            if date >= from && date <= to {
                entries.push(entry(
                    item,
                    OccurrenceType::Closed,
                    date,
                    (closed.time.clone(), None),
                    "Closed:".to_string(),
                ));
            }
        }

        if !(item.done && settings.skip_timestamp_if_done) {
            let mut index = 0;
            while index < headline.timestamps.len() {
                let timestamp = &headline.timestamps[index];
                if !timestamp.active {
                    index += 1;
                    continue;
                }
                if let Some(end) = headline.timestamps.get(index + 1)
                    && end.active
                    && headline.timestamps[index].span.end <= end.span.start
                {
                    let document = self
                        .index
                        .document(std::path::Path::new(&item.file))
                        .expect("headline document");
                    if document.source()[timestamp.span.end..end.span.start].trim() == "--" {
                        let start_date = parse_date(&timestamp.timestamp.date)?;
                        let end_date = parse_date(&end.timestamp.date)?;
                        let total = (end_date - start_date).num_days() + 1;
                        for day in date_range(start_date.max(from), end_date.min(to)) {
                            let ordinal = (day - start_date).num_days() + 1;
                            let time = if day == start_date {
                                timestamp.timestamp.time.clone()
                            } else if day == end_date {
                                end.timestamp.time.clone()
                            } else {
                                None
                            };
                            entries.push(entry(
                                item,
                                OccurrenceType::Block,
                                day,
                                (time, None),
                                format!("({ordinal}/{total}):"),
                            ));
                        }
                        index += 2;
                        continue;
                    }
                }
                for date in repeated_dates(&timestamp.timestamp, from, to, false)? {
                    entries.push(entry(
                        item,
                        OccurrenceType::Timestamp,
                        date,
                        (
                            timestamp.timestamp.time.clone(),
                            timestamp.timestamp.end_time.clone(),
                        ),
                        String::new(),
                    ));
                }
                index += 1;
            }
        }
        Ok(entries)
    }
}

/// One agenda row on `date`; the time pair is the start time and, for a
/// range such as `10:00-11:30`, the end time.
fn entry(
    item: &Item,
    kind: OccurrenceType,
    date: NaiveDate,
    (time, end_time): (Option<String>, Option<String>),
    extra: String,
) -> AgendaEntry {
    AgendaEntry {
        schema: AGENDA_ENTRY_SCHEMA.to_string(),
        item: item.clone(),
        occurrence: Occurrence {
            kind,
            date: date.format("%Y-%m-%d").to_string(),
            time,
            end_time,
            extra,
        },
    }
}

fn parse_day_interval(input: &str) -> Option<i64> {
    let value = input.strip_prefix('-')?;
    let days = value.strip_suffix('d')?;
    days.parse().ok()
}

pub(super) fn parse_date(input: &str) -> Result<NaiveDate, AgdError> {
    NaiveDate::parse_from_str(input, "%Y-%m-%d")
        .map_err(|_| AgdError::new("invalid_date", format!("invalid Org date: {input}")))
}

pub(super) fn date_range(start: NaiveDate, end: NaiveDate) -> impl Iterator<Item = NaiveDate> {
    (0..=(end - start).num_days().max(-1)).map(move |days| start + Duration::days(days))
}

fn collection_rank(kind: OccurrenceType) -> u8 {
    match kind {
        OccurrenceType::Deadline | OccurrenceType::UpcomingDeadline => 0,
        OccurrenceType::Scheduled | OccurrenceType::PastScheduled => 1,
        OccurrenceType::Block => 2,
        OccurrenceType::Timestamp => 3,
        OccurrenceType::Closed | OccurrenceType::Todo => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agenda::test_support::fixture;

    #[test]
    fn expands_warnings_repeaters_ranges_and_past_scheduled() {
        let (_temp, index) = fixture(
            "#+TODO: TODO | DONE\n#+FILETAGS: :work:\n* TODO Late\nSCHEDULED: <2026-10-01 Thu>\n* TODO Due\nDEADLINE: <2026-10-16 Fri -2d>\n* TODO Repeat\nSCHEDULED: <2026-10-12 Mon +2d>\n* TODO Block\n<2026-10-14 Wed>--<2026-10-15 Thu>\n",
        );
        let entries = AgendaEngine::new(&index)
            .agenda(&AgendaQuery {
                from: NaiveDate::from_ymd_opt(2026, 10, 13).unwrap(),
                to: NaiveDate::from_ymd_opt(2026, 10, 16).unwrap(),
                today: NaiveDate::from_ymd_opt(2026, 10, 14).unwrap(),
                match_expression: Some("+work/TODO".to_string()),
                log_mode: false,
            })
            .unwrap();
        assert!(
            entries
                .iter()
                .any(|entry| { entry.occurrence.kind == OccurrenceType::PastScheduled })
        );
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.occurrence.kind == OccurrenceType::UpcomingDeadline)
                .count(),
            1
        );
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.item.title == "Repeat")
                .count(),
            2
        );
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.occurrence.kind == OccurrenceType::Block)
                .count(),
            2
        );
    }
}
