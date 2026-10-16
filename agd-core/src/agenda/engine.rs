//! `AgendaEngine` and what the agenda collects: the rows each heading gives on
//! each day (scheduled, deadline and its warning, past-scheduled reminders,
//! timestamps, date ranges, and closed entries in log mode), as
//! org-agenda-get-scheduled and org-agenda-get-deadlines collect them, and the
//! TODO, inbox and tags lists. Also the date helpers the agenda modules share.
//!
//! Ordering is `sort`'s, repeaters `repeat`'s, matching `matcher`'s, views
//! `view`'s and org-ql `query`'s. The engine only reads the index it is given.

use std::collections::BTreeSet;
use std::path::Path;

use chrono::{Duration, NaiveDate};

use crate::date::parse_org_date;
use crate::write::{CaptureTarget, find_olpath, heading_titled};
use crate::{
    AGENDA_ENTRY_SCHEMA, AgdError, AgendaEntry, CaptureTemplate, ErrorCode, HeadlineSpan,
    IndexedDocument, Item, Occurrence, OccurrenceType, WorkspaceIndex,
};

use super::matcher::{TagTodoMatcher, matcher_now};
use super::repeat::{planning_dates, repeated_dates};
use super::sort::{compare_by_strategy, compare_items, urgency};

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
                ErrorCode::InvalidRange,
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
                        error.code(),
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
        // The configured keywords in order, as a file without #+TODO reads them.
        let todo_order: Vec<String> = crate::document::todo_keywords("", self.index.settings())
            .into_iter()
            .map(|keyword| keyword.name)
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

    /// The inbox: unfinished entries where captures land, in file and
    /// buffer order. For each `[[capture]]` template (`agent_inbox` is one
    /// of them) that is the direct children of its target heading
    /// (`heading`, `olpath` or `id`), or the top-level entries of a
    /// file it targets as a whole. A `datetree` target files entries by
    /// date, a log rather than an inbox, so it is left out.
    pub fn inbox(&self) -> Vec<Item> {
        let mut landed: BTreeSet<(&Path, usize)> = BTreeSet::new();
        for template in &self.index.settings().captures {
            if template.datetree {
                continue;
            }
            if let Some((document, parent)) = self.capture_target(template) {
                let level = parent.map_or(1, |parent| parent.level + 1);
                let region = parent.map_or(0..document.source().len(), |parent| {
                    parent.subtree_span.start..parent.subtree_span.end
                });
                for headline in document.headlines() {
                    if headline.level == level && region.contains(&headline.line_span.start) {
                        landed.insert((document.path(), headline.item.line));
                    }
                }
            }
        }
        self.headlines()
            .filter(|headline| {
                !headline.item.done
                    && landed.contains(&(Path::new(&headline.item.file), headline.item.line))
            })
            .map(|headline| headline.item.clone())
            .collect()
    }

    /// The document a template captures into and the heading its entries go
    /// under (None for a whole-file target), as the index reads them now;
    /// None when the target is missing, as a capture would find it.
    fn capture_target(
        &self,
        template: &CaptureTemplate,
    ) -> Option<(&IndexedDocument, Option<&HeadlineSpan>)> {
        if let CaptureTarget::Id(id) = CaptureTarget::of(template) {
            let (document, headline) = self.index.resolve(&id).ok()?;
            return Some((document, Some(headline)));
        }
        let file = shellexpand::tilde(template.file.as_deref()?).into_owned();
        let path = self.index.roots().resolve_read(Path::new(&file)).ok()?;
        let document = self.index.document(&path)?;
        let parent = match CaptureTarget::of(template) {
            CaptureTarget::File | CaptureTarget::Id(_) => None,
            CaptureTarget::Heading(title) => Some(heading_titled(document, &title)?),
            CaptureTarget::OutlinePath(outline) => {
                let start = find_olpath(document, &outline).ok()?;
                Some(
                    document
                        .headlines()
                        .iter()
                        .find(|headline| headline.line_span.start == start)?,
                )
            }
        };
        Some((document, parent))
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
            let base = parse_org_date(&scheduled.date)?;
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
                let late = (today - base).num_days();
                entries.push(reminder(
                    item,
                    OccurrenceType::PastScheduled,
                    today,
                    format!("Sched.{late:>2}x:"),
                    late,
                ));
            }
        }

        if !(item.done && settings.skip_deadline_if_done)
            && let Some(deadline) = &item.planning.deadline
        {
            let base = parse_org_date(&deadline.date)?;
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
                .and_then(warning_days)
                .unwrap_or(settings.deadline_warning_days)
                .max(0);
            if !item.done && today >= from && today <= to {
                let late = (today - base).num_days();
                if late > 0 {
                    entries.push(reminder(
                        item,
                        OccurrenceType::Deadline,
                        today,
                        format!("{late} d. ago:"),
                        late,
                    ));
                } else if late < 0 && -late <= warning_days {
                    entries.push(reminder(
                        item,
                        OccurrenceType::UpcomingDeadline,
                        today,
                        format!("In {:>3} d.:", -late),
                        late,
                    ));
                }
            }
        }

        if query.log_mode
            && let Some(closed) = &item.planning.closed
        {
            let date = parse_org_date(&closed.date)?;
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
                        let start_date = parse_org_date(&timestamp.timestamp.date)?;
                        let end_date = parse_org_date(&end.timestamp.date)?;
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

/// A reminder row on today: a past-scheduled or missed deadline `days`
/// late (a late row), or an upcoming deadline `-days` away.
fn reminder(
    item: &Item,
    kind: OccurrenceType,
    today: NaiveDate,
    extra: String,
    days: i64,
) -> AgendaEntry {
    let mut row = entry(item, kind, today, (None, None), extra);
    row.occurrence.days = days;
    row.occurrence.late = days > 0;
    row
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
            days: 0,
            late: false,
        },
    }
}

/// A deadline's warning period in days, as org-get-wdays reads `-3d`,
/// `-1w`, `-1m`, `-1y` or `-48h`: the count times the unit's days (a month
/// is 30.4, a year 365.25, an hour 0.041667), rounded down.
fn warning_days(warning: &str) -> Option<i64> {
    let value = warning.trim_start_matches('-');
    let unit = value.chars().last()?;
    let factor = match unit {
        'h' => 0.041667,
        'd' => 1.0,
        'w' => 7.0,
        'm' => 30.4,
        'y' => 365.25,
        _ => return None,
    };
    let count: u32 = value[..value.len() - 1].parse().ok()?;
    Some((f64::from(count) * factor).floor() as i64)
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
    use crate::{CaptureTemplate, Settings};

    #[test]
    fn the_inbox_is_where_capture_templates_land() {
        let temp = tempfile::tempdir().unwrap();
        let write = |name: &str, text: &str| std::fs::write(temp.path().join(name), text).unwrap();
        write(
            "refile.org",
            "* Refile\n** TODO Renew passport\n*** Notes under it\n** DONE Filed already\n",
        );
        write("work.org", "* Inbox\n** TODO Not a capture target\n");
        write(
            "projects.org",
            "* Agd\n** Ideas\n*** Faster index\n* Other\n** Ideas\n*** Elsewhere\n",
        );
        write("notes.org", "* Loose note\n** Its detail\n");
        write(
            "journal.org",
            "* 2026\n** 2026-10 October\n*** 2026-10-16 Friday\n**** Wrote\n",
        );
        let template = |key: &str, file: &str| CaptureTemplate {
            key: key.to_string(),
            file: Some(file.to_string()),
            heading: None,
            ..CaptureTemplate::default()
        };
        let settings = Settings {
            roots: vec![temp.path().to_string_lossy().into_owned()],
            agenda_files: vec!["*.org".to_string()],
            captures: vec![
                CaptureTemplate {
                    heading: Some("Refile".to_string()),
                    ..template("t", "refile.org")
                },
                CaptureTemplate {
                    olpath: Some(vec!["Agd".to_string(), "Ideas".to_string()]),
                    ..template("i", "projects.org")
                },
                template("n", "notes.org"),
                CaptureTemplate {
                    datetree: true,
                    ..template("j", "journal.org")
                },
            ],
            ..Settings::default()
        };
        let index = WorkspaceIndex::load(settings).unwrap();
        let titles: Vec<String> = AgendaEngine::new(&index)
            .inbox()
            .into_iter()
            .map(|item| item.title)
            .collect();
        assert_eq!(titles, ["Loose note", "Faster index", "Renew passport"]);
    }

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
