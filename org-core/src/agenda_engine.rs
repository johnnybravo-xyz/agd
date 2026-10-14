use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::LazyLock;

use chrono::{Datelike, Duration, Local, Months, NaiveDate, NaiveDateTime, NaiveTime};
use regex::Regex;

use serde::Serialize;

use crate::{
    AGENDA_ENTRY_SCHEMA, AgdError, AgendaEntry, HeadlineSpan, Item, Occurrence, OccurrenceType,
    PlanningTimestamp, Settings, ViewBlock, ViewGroup, ViewSettings, WorkspaceIndex,
};

/// Checks a view block's match the way `AgendaEngine::view` will use it.
pub fn validate_view_block(block: &ViewBlock) -> Result<(), AgdError> {
    let Some(expression) = block.match_expression.as_deref() else {
        return Ok(());
    };
    let expression = if block.kind == "todo" {
        format!("/{expression}")
    } else {
        expression.to_string()
    };
    TagTodoMatcher::parse(&expression)?;
    for group in &block.groups {
        if let Some(expression) = &group.match_expression {
            TagTodoMatcher::parse(expression)?;
        }
    }
    Ok(())
}

/// One block of a saved view: agenda rows for an agenda block, otherwise a
/// list of items. `from`/`to` are set for agenda blocks only. A block with
/// groups has its rows in `groups` and none in `entries` or `items`.
#[derive(Clone, Debug, Serialize)]
pub struct ViewSection {
    pub header: String,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub entries: Vec<AgendaEntry>,
    pub items: Vec<Item>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<ViewSectionGroup>,
}

/// A grouped block's rows under one group name, in display order.
#[derive(Clone, Debug, Serialize)]
pub struct ViewSectionGroup {
    pub name: String,
    pub entries: Vec<AgendaEntry>,
    pub items: Vec<Item>,
}

/// Agenda bounds for an org-agenda span: day, week, month, or a number of
/// days starting at `start`.
pub fn span_days(
    span: &str,
    start: NaiveDate,
    week_start: &str,
) -> Result<(NaiveDate, NaiveDate), AgdError> {
    match span {
        "day" => Ok(AgendaSpan::Day.bounds(start, week_start)),
        "week" => Ok(AgendaSpan::Week.bounds(start, week_start)),
        "month" => Ok(AgendaSpan::Month.bounds(start, week_start)),
        days => {
            let days = days
                .parse::<i64>()
                .ok()
                .filter(|days| *days > 0)
                .ok_or_else(|| {
                    AgdError::new(
                        "invalid_span",
                        format!("agenda span must be day, week, month or a day count: {span}"),
                    )
                })?;
            Ok((start, start + Duration::days(days - 1)))
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgendaSpan {
    Day,
    Week,
    Month,
}

impl AgendaSpan {
    pub fn bounds(self, from: NaiveDate, week_start: &str) -> (NaiveDate, NaiveDate) {
        match self {
            Self::Day => (from, from),
            Self::Week => {
                let desired = if week_start.eq_ignore_ascii_case("sunday") {
                    6
                } else {
                    0
                };
                let current = from.weekday().num_days_from_monday() as i64;
                let start = from - Duration::days((current - desired).rem_euclid(7));
                (start, start + Duration::days(6))
            }
            Self::Month => {
                let start = NaiveDate::from_ymd_opt(from.year(), from.month(), 1)
                    .expect("valid first of month");
                let next = start
                    .checked_add_months(Months::new(1))
                    .expect("next month");
                (start, next - Duration::days(1))
            }
        }
    }
}

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
    index: &'a WorkspaceIndex,
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
            .map(|entry| (urgency(&entry, query.today, settings), entry))
            .collect();
        let todo_order: Vec<String> = self
            .index
            .settings()
            .todo_keywords
            .iter()
            .flatten()
            .filter(|keyword| keyword.as_str() != "|")
            .map(|keyword| keyword_name(keyword).to_string())
            .collect();
        keyed.sort_by(|left, right| {
            left.1
                .occurrence
                .date
                .cmp(&right.1.occurrence.date)
                .then_with(|| compare_by_strategy(left, right, settings, &todo_order))
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
        items.sort_by(compare_items);
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
        items.sort_by(compare_items);
        Ok(items)
    }

    fn headlines(&self) -> impl Iterator<Item = &HeadlineSpan> {
        self.index
            .documents()
            .flat_map(|document| document.headlines().iter())
    }

    /// Runs a saved view block by block. `start` is the first agenda day
    /// before span alignment, as for `agd agenda --from`.
    pub fn view(
        &self,
        view: &ViewSettings,
        start: NaiveDate,
        today: NaiveDate,
        log_mode: bool,
    ) -> Result<Vec<ViewSection>, AgdError> {
        let settings = self.index.settings();
        let mut sections = Vec::new();
        for block in &view.blocks {
            let match_expression = block.match_expression.clone();
            let section = match block.kind.as_str() {
                "agenda" => {
                    let span = block.span.as_deref().unwrap_or(&settings.agenda_span);
                    let (from, to) = span_days(span, start, &settings.week_start)?;
                    ViewSection {
                        header: block.header.clone().unwrap_or_default(),
                        from: Some(from),
                        to: Some(to),
                        entries: self.agenda(&AgendaQuery {
                            from,
                            to,
                            today,
                            match_expression,
                            log_mode,
                        })?,
                        items: Vec::new(),
                        groups: Vec::new(),
                    }
                }
                kind => {
                    let keywords = match_expression.as_deref().unwrap_or("");
                    let (items, default_header) = match kind {
                        "alltodo" => (
                            self.todo(None, today)?,
                            "Global list of TODO items of type: ALL".to_string(),
                        ),
                        // `(todo "DELEGATED")` lists that keyword even when it is
                        // a done state; only alltodo and tags-todo skip done.
                        "todo" if !keywords.is_empty() => (
                            self.tags(&format!("/{keywords}"), today)?,
                            format!("Global list of TODO items of type: {keywords}"),
                        ),
                        "todo" => (
                            self.todo(None, today)?,
                            "Global list of TODO items of type: ALL".to_string(),
                        ),
                        "tags" => (
                            self.tags(keywords, today)?,
                            format!("Headlines with TAGS match: {keywords}"),
                        ),
                        _ => (
                            self.todo(Some(keywords), today)?,
                            format!("Headlines with TAGS match: {keywords}"),
                        ),
                    };
                    ViewSection {
                        header: block.header.clone().unwrap_or(default_header),
                        from: None,
                        to: None,
                        entries: Vec::new(),
                        items,
                        groups: Vec::new(),
                    }
                }
            };
            sections.push(self.group_section(section, &block.groups, today)?);
        }
        Ok(sections)
    }

    /// Moves a block's rows into its groups, if it has any.
    fn group_section(
        &self,
        mut section: ViewSection,
        groups: &[ViewGroup],
        today: NaiveDate,
    ) -> Result<ViewSection, AgdError> {
        if groups.is_empty() {
            return Ok(section);
        }
        let headlines: HashMap<(&str, usize), &HeadlineSpan> = self
            .headlines()
            .map(|headline| ((headline.item.file.as_str(), headline.item.line), headline))
            .collect();
        let context = GroupContext {
            headlines: &headlines,
            now: matcher_now(today),
            settings: self.index.settings(),
        };
        let entries = group_rows(
            std::mem::take(&mut section.entries),
            |entry| &entry.item,
            groups,
            &context,
        )?;
        let items = group_rows(
            std::mem::take(&mut section.items),
            |item| item,
            groups,
            &context,
        )?;
        section.groups = entries
            .into_iter()
            .map(|(name, entries)| ViewSectionGroup {
                name,
                entries,
                items: Vec::new(),
            })
            .chain(items.into_iter().map(|(name, items)| ViewSectionGroup {
                name,
                entries: Vec::new(),
                items,
            }))
            .collect();
        Ok(section)
    }

    pub fn query(&self, expression: &str, today: NaiveDate) -> Result<Vec<Item>, AgdError> {
        let expression = QlExpr::parse(expression)?;
        let mut items: Vec<Item> = self
            .index
            .items()
            .filter(|item| expression.matches(item, today))
            .cloned()
            .collect();
        items.sort_by(compare_items);
        Ok(items)
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
                    scheduled.time.clone(),
                    "Scheduled:".to_string(),
                ));
            }
            if !item.done && base < today && today >= from && today <= to {
                entries.push(entry(
                    item,
                    OccurrenceType::PastScheduled,
                    today,
                    None,
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
                    deadline.time.clone(),
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
                        None,
                        format!("{} d. ago:", (today - base).num_days()),
                    ));
                } else if base > today && (base - today).num_days() <= warning_days {
                    entries.push(entry(
                        item,
                        OccurrenceType::UpcomingDeadline,
                        today,
                        None,
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
                    closed.time.clone(),
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
                                time,
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
                        timestamp.timestamp.time.clone(),
                        String::new(),
                    ));
                }
                index += 1;
            }
        }
        Ok(entries)
    }
}

struct GroupContext<'a> {
    headlines: &'a HashMap<(&'a str, usize), &'a HeadlineSpan>,
    now: NaiveDateTime,
    settings: &'a Settings,
}

const OTHER_ITEMS: &str = "Other items";

/// org-super-agenda's grouping: each group takes the remaining rows it
/// matches, in list order; groups show by order (equal non-zero orders by
/// name), the rest last as "Other items" (order 99), empty groups not at
/// all. Rows keep their order inside a group.
fn group_rows<T>(
    rows: Vec<T>,
    item_of: impl Fn(&T) -> &Item,
    groups: &[ViewGroup],
    context: &GroupContext<'_>,
) -> Result<Vec<(String, Vec<T>)>, AgdError> {
    let mut remaining = rows;
    let mut sections: Vec<(String, i64, Vec<T>)> = Vec::new();
    for group in groups {
        if let Some(by) = &group.by {
            let mut keyed: BTreeMap<String, Vec<T>> = BTreeMap::new();
            let mut rest = Vec::new();
            for row in remaining {
                match group_key(by, item_of(&row)) {
                    Some(key) => keyed.entry(key).or_default().push(row),
                    None => rest.push(row),
                }
            }
            sections.extend(
                keyed
                    .into_iter()
                    .map(|(name, rows)| (name, group.order, rows)),
            );
            remaining = rest;
            continue;
        }
        let matcher = group
            .match_expression
            .as_deref()
            .map(TagTodoMatcher::parse)
            .transpose()?;
        let effort_bound = |bound: &Option<String>| -> Result<Option<f64>, AgdError> {
            bound
                .as_deref()
                .map(|text| {
                    duration_minutes(text).ok_or_else(|| {
                        AgdError::new("invalid_view", format!("effort '{text}' is not a duration"))
                    })
                })
                .transpose()
        };
        let effort_max = effort_bound(&group.effort_max)?;
        let effort_min = effort_bound(&group.effort_min)?;
        let (matching, rest): (Vec<T>, Vec<T>) = remaining.into_iter().partition(|row| {
            let item = item_of(row);
            let Some(headline) = context.headlines.get(&(item.file.as_str(), item.line)) else {
                return matcher.is_none() && effort_max.is_none() && effort_min.is_none();
            };
            let effort = || {
                headline
                    .properties
                    .iter()
                    .find(|property| property.key.eq_ignore_ascii_case("Effort"))
                    .and_then(|property| duration_minutes(&property.value))
            };
            matcher
                .as_ref()
                .is_none_or(|matcher| matcher.matches(headline, context.now, context.settings))
                && effort_max.is_none_or(|max| effort().is_some_and(|effort| effort <= max))
                && effort_min.is_none_or(|min| effort().is_some_and(|effort| effort >= min))
        });
        remaining = rest;
        if !group.discard {
            let name = group
                .name
                .clone()
                .or_else(|| group.match_expression.clone())
                .unwrap_or_else(|| OTHER_ITEMS.to_string());
            sections.push((name, group.order, matching));
        }
    }
    sections.insert(0, (OTHER_ITEMS.to_string(), 99, remaining));
    sections.sort_by(|left, right| {
        left.1.cmp(&right.1).then_with(|| {
            if left.1 == 0 {
                Ordering::Equal
            } else {
                left.0.cmp(&right.0)
            }
        })
    });
    Ok(sections
        .into_iter()
        .filter(|(_, _, rows)| !rows.is_empty())
        .map(|(name, _, rows)| (name, rows))
        .collect())
}

static DURATION_NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9]+(?:\.[0-9]*)?$").unwrap());
static DURATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^((?:[0-9]+(?:\.[0-9]*)?[ \t]*(?:min|h|d|w|m|y)[ \t]*)*)(?:([0-9]+):([0-9]{2})(?::([0-9]{2}))?)?$",
    )
    .unwrap()
});
static DURATION_UNIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([0-9]+(?:\.[0-9]*)?)[ \t]*(min|h|d|w|m|y)").unwrap());

/// Minutes in an Org duration, as org-duration-to-minutes reads it: a
/// number of minutes, `H:MM` or `H:MM:SS`, unit sums such as `1h 30min` or
/// `2d` (`m` is a 30-day month, `y` 365.25 days), or units then `H:MM`.
pub(crate) fn duration_minutes(text: &str) -> Option<f64> {
    let text = text.trim();
    if DURATION_NUMBER.is_match(text) {
        return text.parse().ok();
    }
    let captures = DURATION.captures(text)?;
    let units = captures.get(1).map_or("", |units| units.as_str());
    if units.is_empty() && captures.get(2).is_none() {
        return None;
    }
    let mut minutes = 0.0;
    for unit in DURATION_UNIT.captures_iter(units) {
        let count: f64 = unit[1].parse().ok()?;
        minutes += count
            * match &unit[2] {
                "min" => 1.0,
                "h" => 60.0,
                "d" => 1_440.0,
                "w" => 10_080.0,
                "m" => 43_200.0,
                _ => 525_960.0,
            };
    }
    let clock = |index: usize| -> f64 {
        captures
            .get(index)
            .and_then(|part| part.as_str().parse::<f64>().ok())
            .unwrap_or(0.0)
    };
    Some(minutes + clock(2) * 60.0 + clock(3) + clock(4) / 60.0)
}

/// The group header for a `by` group, as org-super-agenda's auto groups
/// name them; None leaves the row for later groups.
fn group_key(by: &str, item: &Item) -> Option<String> {
    match by {
        "category" => Some(format!("Category: {}", item.category)),
        "todo" => item.todo.as_ref().map(|todo| format!("To-do: {todo}")),
        "priority" => item
            .priority
            .as_ref()
            .map(|priority| format!("Priority: {priority}")),
        _ => item
            .olpath
            .len()
            .checked_sub(2)
            .map(|parent| item.olpath[parent].clone()),
    }
}

#[derive(Clone, Debug)]
// Emacs tags/todo match syntax: `|`-separated alternatives of terms joined by
// `&`, `+`, `-` or nothing, then an optional `/` TODO part (`NEXT|WAIT`,
// `-DONE`, a leading `!` for not-done states). A term is a tag (`work`) or a
// property comparison (`TODO="NEXT"`, `Effort<15`, `CLOSED>="<-3w>"`) with
// Org's semantics; regexp terms (`{...}`) are rejected explicitly.
struct TagTodoMatcher {
    alternatives: Vec<TagTerms>,
    todo_include: BTreeSet<String>,
    todo_exclude: BTreeSet<String>,
    todo_not_done: bool,
}

#[derive(Clone, Debug, Default)]
struct TagTerms {
    include: BTreeSet<String>,
    exclude: BTreeSet<String>,
    properties: Vec<PropertyTerm>,
}

#[derive(Clone, Debug)]
struct PropertyTerm {
    negated: bool,
    /// Upcased, as Org compares property names.
    name: String,
    op: Ordering,
    /// `=`/`<>` hold on equal/unequal; `<=`/`>=` also hold on equal.
    or_equal: bool,
    not_equal: bool,
    value: PropertyValue,
    /// `OP*`: the property must exist.
    require_present: bool,
}

#[derive(Clone, Debug)]
enum PropertyValue {
    Number(f64),
    Text(String),
    /// The text inside `"<...>"`, resolved against the match's now.
    Time(String),
}

// org-make-tags-matcher treats a quoted value as a time when it starts with
// a digit, now, today, tomorrow or a [+-]N[dmwy] offset inside <> or [].
static MATCHER_TIME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[\[<](?:[0-9]+|now|today|tomorrow|[+-][0-9]+[dmwy]).*[\]>]$").unwrap()
});
static TIME_OFFSET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([+-][0-9]+)([hdwmy])$").unwrap());
static TIMESTAMP_DATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\d{4})-(\d{2})-(\d{2})(?:[^\]>\d]*?(\d{1,2}):(\d{2}))?").unwrap()
});
static LEADING_NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?").unwrap());

/// The match's "now" for `<now>`: the agenda day at the current clock time.
fn matcher_now(today: NaiveDate) -> NaiveDateTime {
    today.and_time(Local::now().time())
}

impl TagTodoMatcher {
    fn parse(input: &str) -> Result<Self, AgdError> {
        let (tags, todo) = input.split_once('/').unwrap_or((input, ""));
        let mut alternatives = Vec::new();
        if !tags.trim().is_empty() {
            for alternative in tags.split('|') {
                alternatives.push(parse_tag_terms(alternative, input)?);
            }
        }
        let todo_not_done = todo.starts_with('!');
        let mut todo_include = BTreeSet::new();
        let mut todo_exclude = BTreeSet::new();
        for term in todo.trim_start_matches('!').split('|') {
            let term = term.trim();
            if let Some(keyword) = term.strip_prefix('-') {
                todo_exclude.insert(keyword.to_string());
            } else if !term.is_empty() {
                todo_include.insert(term.trim_start_matches('+').to_string());
            }
        }
        Ok(Self {
            alternatives,
            todo_include,
            todo_exclude,
            todo_not_done,
        })
    }

    fn matches(&self, headline: &HeadlineSpan, now: NaiveDateTime, settings: &Settings) -> bool {
        let item = &headline.item;
        let tags: BTreeSet<&str> = item
            .tags
            .iter()
            .chain(&item.inherited_tags)
            .map(String::as_str)
            .collect();
        let tags_match = self.alternatives.is_empty()
            || self.alternatives.iter().any(|terms| {
                terms.include.iter().all(|tag| tags.contains(tag.as_str()))
                    && terms.exclude.iter().all(|tag| !tags.contains(tag.as_str()))
                    && terms
                        .properties
                        .iter()
                        .all(|term| term.negated != term.holds(headline, now, settings))
            });
        let todo = item.todo.as_deref();
        tags_match
            && (!self.todo_not_done || (todo.is_some() && !item.done))
            && (self.todo_include.is_empty()
                || todo.is_some_and(|todo| self.todo_include.contains(todo)))
            && todo.is_none_or(|todo| !self.todo_exclude.contains(todo))
    }
}

impl PropertyTerm {
    fn holds(&self, headline: &HeadlineSpan, now: NaiveDateTime, settings: &Settings) -> bool {
        let actual = property_value(headline, &self.name, settings);
        if self.require_present && actual.is_none() {
            return false;
        }
        let actual = actual.unwrap_or_default();
        let ordering = match &self.value {
            PropertyValue::Number(value) => leading_number(&actual).partial_cmp(value),
            PropertyValue::Text(value) => Some(actual.as_str().cmp(value)),
            // org-time* compare float times and fail when either side is
            // missing or unparseable, whatever the operator.
            PropertyValue::Time(value) => match (timestamp_time(&actual), matcher_time(value, now))
            {
                (Some(actual), Some(value)) => Some(actual.cmp(&value)),
                _ => return false,
            },
        };
        let Some(ordering) = ordering else {
            return false;
        };
        if self.not_equal {
            ordering != Ordering::Equal
        } else {
            ordering == self.op || (self.or_equal && ordering == Ordering::Equal)
        }
    }
}

/// The value Org's matcher reads for NAME: LEVEL, CATEGORY and TODO are the
/// heading's; PRIORITY falls back to the default priority; planning lines
/// give their timestamps; anything else is the heading's own property
/// drawer entry (Org's matcher does not inherit properties by default).
fn property_value(headline: &HeadlineSpan, name: &str, settings: &Settings) -> Option<String> {
    let item = &headline.item;
    let planning = |timestamp: &Option<PlanningTimestamp>, open: char, close: char| {
        timestamp.as_ref().map(|timestamp| {
            timestamp
                .raw
                .clone()
                .unwrap_or_else(|| format!("{open}{}{close}", timestamp.date))
        })
    };
    match name {
        "LEVEL" => Some(headline.level.to_string()),
        "CATEGORY" => Some(item.category.clone()),
        "TODO" => item.todo.clone(),
        "PRIORITY" => Some(
            item.priority
                .clone()
                .unwrap_or_else(|| settings.default_priority.clone()),
        ),
        "SCHEDULED" => planning(&item.planning.scheduled, '<', '>'),
        "DEADLINE" => planning(&item.planning.deadline, '<', '>'),
        "CLOSED" => planning(&item.planning.closed, '[', ']'),
        _ => headline
            .properties
            .iter()
            .find(|property| property.key.eq_ignore_ascii_case(name))
            .map(|property| property.value.trim().to_string()),
    }
}

/// Emacs's string-to-number: the leading number, or 0.
fn leading_number(value: &str) -> f64 {
    LEADING_NUMBER
        .find(value)
        .and_then(|number| number.as_str().trim().parse().ok())
        .unwrap_or(0.0)
}

/// The first Org timestamp in VALUE, at midnight when it has no time.
fn timestamp_time(value: &str) -> Option<NaiveDateTime> {
    let captures = TIMESTAMP_DATE.captures(value)?;
    let number = |index: usize| -> Option<u32> { captures.get(index)?.as_str().parse().ok() };
    let date = NaiveDate::from_ymd_opt(captures[1].parse().ok()?, number(2)?, number(3)?)?;
    let time = NaiveTime::from_hms_opt(number(4).unwrap_or(0), number(5).unwrap_or(0), 0)?;
    Some(date.and_time(time))
}

/// org-matcher-time: now, today, tomorrow, an offset from today's midnight
/// (h from now; a month is 31 days, a year 365.25), or a timestamp.
fn matcher_time(value: &str, now: NaiveDateTime) -> Option<NaiveDateTime> {
    let inner = &value[1..value.len() - 1];
    let midnight = now.date().and_time(NaiveTime::MIN);
    match inner {
        "now" => return Some(now),
        "today" => return Some(midnight),
        "tomorrow" => return Some(midnight + Duration::days(1)),
        _ => {}
    }
    if let Some(captures) = TIME_OFFSET.captures(inner) {
        let count: f64 = captures[1].parse().ok()?;
        let (base, seconds) = match &captures[2] {
            "h" => (now, 3_600.0),
            "d" => (midnight, 86_400.0),
            "w" => (midnight, 7.0 * 86_400.0),
            "m" => (midnight, 31.0 * 86_400.0),
            _ => (midnight, 365.25 * 86_400.0),
        };
        return Some(base + Duration::seconds((count * seconds) as i64));
    }
    timestamp_time(inner)
}

fn parse_tag_terms(alternative: &str, input: &str) -> Result<TagTerms, AgdError> {
    let invalid =
        |message: String| AgdError::new("invalid_match", format!("{message} in '{input}'"));
    let mut terms = TagTerms::default();
    let mut chars = alternative.trim().chars().peekable();
    while let Some(&next) = chars.peek() {
        let include = match next {
            '+' | '&' => {
                chars.next();
                if next == '&' {
                    continue;
                }
                true
            }
            '-' => {
                chars.next();
                false
            }
            _ => true,
        };
        if chars.peek() == Some(&'{') {
            return Err(invalid(
                "regexp matches ({...}) are not supported".to_string(),
            ));
        }
        let mut tag = String::new();
        while let Some(&character) = chars.peek() {
            if character.is_alphanumeric() || matches!(character, '_' | '@' | '#' | '%') {
                tag.push(character);
                chars.next();
            } else {
                break;
            }
        }
        if tag.is_empty() {
            let found = chars.peek().map_or("end".to_string(), |c| format!("'{c}'"));
            return Err(invalid(format!(
                "expected a tag or property before {found}"
            )));
        }
        if chars
            .peek()
            .is_some_and(|c| matches!(c, '=' | '<' | '>' | '!'))
        {
            let term = parse_property_term(&tag, !include, &mut chars).map_err(invalid)?;
            terms.properties.push(term);
        } else if include {
            terms.include.insert(tag);
        } else {
            terms.exclude.insert(tag);
        }
    }
    Ok(terms)
}

/// Reads `OP[*]VALUE` after a property NAME.
fn parse_property_term(
    name: &str,
    negated: bool,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<PropertyTerm, String> {
    let mut op = String::new();
    while let Some(&character) = chars.peek() {
        if matches!(character, '=' | '<' | '>' | '!') {
            op.push(character);
            chars.next();
        } else {
            break;
        }
    }
    let (op_ordering, or_equal, not_equal) = match op.as_str() {
        "=" | "==" => (Ordering::Equal, false, false),
        "<>" | "!=" => (Ordering::Equal, false, true),
        "<" => (Ordering::Less, false, false),
        "<=" | "=<" => (Ordering::Less, true, false),
        ">" => (Ordering::Greater, false, false),
        ">=" | "=>" => (Ordering::Greater, true, false),
        _ => return Err(format!("unknown operator '{op}' after {name}")),
    };
    let require_present = chars.next_if_eq(&'*').is_some();
    let value = match chars.peek() {
        Some('"') => {
            chars.next();
            let mut text = String::new();
            loop {
                match chars.next() {
                    Some('"') => break,
                    Some(character) => text.push(character),
                    None => return Err(format!("unterminated string after {name}{op}")),
                }
            }
            if MATCHER_TIME.is_match(&text) {
                PropertyValue::Time(text)
            } else {
                PropertyValue::Text(text)
            }
        }
        Some('{') => return Err("regexp matches ({...}) are not supported".to_string()),
        _ => {
            let mut number = String::new();
            while let Some(&character) = chars.peek() {
                if character.is_ascii_digit()
                    || matches!(character, '.' | 'e' | 'E')
                    || (character == '-' && (number.is_empty() || number.ends_with(['e', 'E'])))
                {
                    number.push(character);
                    chars.next();
                } else {
                    break;
                }
            }
            if number.is_empty() {
                return Err(format!("expected a \"string\" or number after {name}{op}"));
            }
            PropertyValue::Number(leading_number(&number))
        }
    };
    Ok(PropertyTerm {
        negated,
        name: name.to_uppercase(),
        op: op_ordering,
        or_equal,
        not_equal,
        value,
        require_present,
    })
}

#[derive(Clone, Debug)]
enum QlExpr {
    And(Vec<QlExpr>),
    Or(Vec<QlExpr>),
    Not(Box<QlExpr>),
    Todo(String),
    Tags(String),
    Priority(String),
    Category(String),
    Text(String),
    Deadline(i64),
    Scheduled(i64),
    Done,
}

impl QlExpr {
    fn parse(input: &str) -> Result<Self, AgdError> {
        let tokens = tokenize(input)?;
        let mut cursor = 0;
        let expression = parse_ql_expression(&tokens, &mut cursor)?;
        if cursor != tokens.len() {
            return Err(AgdError::new(
                "invalid_query",
                "unexpected tokens after query",
            ));
        }
        Ok(expression)
    }

    fn matches(&self, item: &Item, today: NaiveDate) -> bool {
        match self {
            Self::And(expressions) => expressions
                .iter()
                .all(|expression| expression.matches(item, today)),
            Self::Or(expressions) => expressions
                .iter()
                .any(|expression| expression.matches(item, today)),
            Self::Not(expression) => !expression.matches(item, today),
            Self::Todo(todo) => item.todo.as_ref() == Some(todo),
            Self::Tags(tag) => item
                .tags
                .iter()
                .chain(&item.inherited_tags)
                .any(|value| value == tag),
            Self::Priority(priority) => item.priority.as_ref() == Some(priority),
            Self::Category(category) => item.category == *category,
            Self::Text(text) => item.title.to_lowercase().contains(&text.to_lowercase()),
            Self::Deadline(days) => item
                .planning
                .deadline
                .as_ref()
                .and_then(|timestamp| parse_date(&timestamp.date).ok())
                .is_some_and(|date| date >= today && date <= today + Duration::days(*days)),
            Self::Scheduled(days) => item
                .planning
                .scheduled
                .as_ref()
                .and_then(|timestamp| parse_date(&timestamp.date).ok())
                .is_some_and(|date| date >= today && date <= today + Duration::days(*days)),
            Self::Done => item.done,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Token {
    Open,
    Close,
    Word(String),
    String(String),
}

fn tokenize(input: &str) -> Result<Vec<Token>, AgdError> {
    let mut tokens = Vec::new();
    let mut chars = input.char_indices().peekable();
    while let Some((_, character)) = chars.next() {
        match character {
            '(' => tokens.push(Token::Open),
            ')' => tokens.push(Token::Close),
            '"' => {
                let mut value = String::new();
                let mut closed = false;
                while let Some((_, character)) = chars.next() {
                    if character == '"' {
                        closed = true;
                        break;
                    }
                    if character == '\\' {
                        let Some((_, escaped)) = chars.next() else {
                            break;
                        };
                        value.push(escaped);
                    } else {
                        value.push(character);
                    }
                }
                if !closed {
                    return Err(AgdError::new("invalid_query", "unterminated string"));
                }
                tokens.push(Token::String(value));
            }
            character if character.is_whitespace() => {}
            _ => {
                let mut value = String::from(character);
                while let Some((_, next)) = chars.peek() {
                    if next.is_whitespace() || matches!(next, '(' | ')') {
                        break;
                    }
                    value.push(*next);
                    chars.next();
                }
                tokens.push(Token::Word(value));
            }
        }
    }
    Ok(tokens)
}

fn parse_ql_expression(tokens: &[Token], cursor: &mut usize) -> Result<QlExpr, AgdError> {
    expect(tokens, cursor, Token::Open)?;
    let operator = word(tokens, cursor)?;
    let expression = match operator.as_str() {
        "and" | "or" => {
            let mut children = Vec::new();
            while tokens.get(*cursor) != Some(&Token::Close) {
                children.push(parse_ql_expression(tokens, cursor)?);
            }
            if children.is_empty() {
                return Err(AgdError::new(
                    "invalid_query",
                    format!("{operator} needs operands"),
                ));
            }
            if operator == "and" {
                QlExpr::And(children)
            } else {
                QlExpr::Or(children)
            }
        }
        "not" => QlExpr::Not(Box::new(parse_ql_expression(tokens, cursor)?)),
        "todo" => QlExpr::Todo(string_value(tokens, cursor)?),
        "tags" => QlExpr::Tags(string_value(tokens, cursor)?),
        "priority" => QlExpr::Priority(string_value(tokens, cursor)?),
        "category" => QlExpr::Category(string_value(tokens, cursor)?),
        "text" => QlExpr::Text(string_value(tokens, cursor)?),
        "done" => QlExpr::Done,
        "deadline" | "scheduled" => {
            let keyword = word(tokens, cursor)?;
            if keyword != ":to" {
                return Err(AgdError::new(
                    "invalid_query",
                    format!("{operator} supports only :to"),
                ));
            }
            let days = word(tokens, cursor)?.parse::<i64>().map_err(|_| {
                AgdError::new("invalid_query", format!("{operator} :to needs an integer"))
            })?;
            if operator == "deadline" {
                QlExpr::Deadline(days)
            } else {
                QlExpr::Scheduled(days)
            }
        }
        _ => {
            return Err(AgdError::new(
                "invalid_query",
                format!("unsupported selector: {operator}"),
            ));
        }
    };
    expect(tokens, cursor, Token::Close)?;
    Ok(expression)
}

fn expect(tokens: &[Token], cursor: &mut usize, expected: Token) -> Result<(), AgdError> {
    if tokens.get(*cursor) == Some(&expected) {
        *cursor += 1;
        Ok(())
    } else {
        Err(AgdError::new(
            "invalid_query",
            format!("expected {expected:?}"),
        ))
    }
}

fn word(tokens: &[Token], cursor: &mut usize) -> Result<String, AgdError> {
    match tokens.get(*cursor) {
        Some(Token::Word(value)) => {
            *cursor += 1;
            Ok(value.clone())
        }
        _ => Err(AgdError::new("invalid_query", "expected a word")),
    }
}

fn string_value(tokens: &[Token], cursor: &mut usize) -> Result<String, AgdError> {
    match tokens.get(*cursor) {
        Some(Token::String(value)) => {
            *cursor += 1;
            Ok(value.clone())
        }
        _ => Err(AgdError::new("invalid_query", "expected a quoted string")),
    }
}

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

pub fn agenda_csv(entries: &[AgendaEntry]) -> String {
    let mut output = String::new();
    for entry in entries {
        let mut seen_tags = BTreeSet::new();
        let tags = entry
            .item
            .inherited_tags
            .iter()
            .chain(&entry.item.tags)
            .filter(|tag| seen_tags.insert((*tag).clone()))
            .cloned()
            .collect::<Vec<_>>()
            .join(":");
        let fields = [
            entry.item.category.clone(),
            agenda_title(&entry.item),
            occurrence_name(entry.occurrence.kind).to_string(),
            entry.item.todo.clone().unwrap_or_default(),
            tags,
            entry.occurrence.date.clone(),
            // Emacs CSV writes hours without a leading zero (9:00, 0:30).
            entry
                .occurrence
                .time
                .as_deref()
                .map(|time| match time.strip_prefix('0') {
                    Some(rest) if rest.starts_with(|c: char| c.is_ascii_digit()) => rest,
                    _ => time,
                })
                .unwrap_or_default()
                .to_string(),
            entry.occurrence.extra.clone(),
            entry.item.priority.clone().unwrap_or_default(),
        ];
        output.push_str(&fields.map(|field| csv_field(&field)).join(","));
        output.push('\n');
    }
    output
}

fn entry(
    item: &Item,
    kind: OccurrenceType,
    date: NaiveDate,
    time: Option<String>,
    extra: String,
) -> AgendaEntry {
    AgendaEntry {
        schema: AGENDA_ENTRY_SCHEMA.to_string(),
        item: item.clone(),
        occurrence: Occurrence {
            kind,
            date: date.format("%Y-%m-%d").to_string(),
            time,
            extra,
        },
    }
}

fn repeated_dates(
    timestamp: &PlanningTimestamp,
    from: NaiveDate,
    to: NaiveDate,
    done: bool,
) -> Result<Vec<NaiveDate>, AgdError> {
    let mut date = parse_date(&timestamp.date)?;
    if done {
        return Ok((date >= from && date <= to)
            .then_some(date)
            .into_iter()
            .collect());
    }
    let Some(repeater) = timestamp.repeater.as_deref() else {
        return Ok((date >= from && date <= to)
            .then_some(date)
            .into_iter()
            .collect());
    };
    let (_, amount, unit) = parse_repeater(repeater)?;
    while date < from {
        date = add_period(date, amount, unit)?;
    }
    let mut dates = Vec::new();
    while date <= to {
        dates.push(date);
        date = add_period(date, amount, unit)?;
    }
    Ok(dates)
}

/// Days in [from, to] where a SCHEDULED or DEADLINE timestamp is listed
/// itself: its base date, plus repeat dates after today while not done.
fn planning_dates(
    timestamp: &PlanningTimestamp,
    from: NaiveDate,
    to: NaiveDate,
    today: NaiveDate,
    done: bool,
) -> Result<Vec<NaiveDate>, AgdError> {
    let base = parse_date(&timestamp.date)?;
    let mut dates = Vec::new();
    if base >= from && base <= to {
        dates.push(base);
    }
    if done {
        return Ok(dates);
    }
    for date in repeated_dates(timestamp, from, to, false)? {
        if date > today && date != base {
            dates.push(date);
        }
    }
    Ok(dates)
}

fn parse_repeater(input: &str) -> Result<(&str, u64, char), AgdError> {
    // Habit repeaters carry a maximum interval, e.g. `.+2d/3d`; only the
    // repeat interval matters for agenda dates.
    let input = input.split('/').next().unwrap_or(input);
    let (kind, value) = if let Some(value) = input.strip_prefix("++") {
        ("++", value)
    } else if let Some(value) = input.strip_prefix(".+") {
        (".+", value)
    } else if let Some(value) = input.strip_prefix('+') {
        ("+", value)
    } else {
        return Err(AgdError::new(
            "invalid_repeater",
            format!("invalid repeater: {input}"),
        ));
    };
    let unit = value
        .chars()
        .next_back()
        .ok_or_else(|| AgdError::new("invalid_repeater", format!("invalid repeater: {input}")))?;
    let amount = value[..value.len() - unit.len_utf8()]
        .parse::<u64>()
        .map_err(|_| AgdError::new("invalid_repeater", format!("invalid repeater: {input}")))?;
    if amount == 0 {
        return Err(AgdError::new(
            "invalid_repeater",
            "repeater interval must be positive",
        ));
    }
    Ok((kind, amount, unit))
}

fn add_period(date: NaiveDate, amount: u64, unit: char) -> Result<NaiveDate, AgdError> {
    let next = match unit {
        'd' => date.checked_add_signed(Duration::days(amount as i64)),
        'w' => date.checked_add_signed(Duration::weeks(amount as i64)),
        'm' => date.checked_add_months(Months::new(amount as u32)),
        'y' => date.checked_add_months(Months::new((amount * 12) as u32)),
        _ => None,
    };
    next.ok_or_else(|| AgdError::new("invalid_repeater", "repeater date is out of range"))
}

fn parse_day_interval(input: &str) -> Option<i64> {
    let value = input.strip_prefix('-')?;
    let days = value.strip_suffix('d')?;
    days.parse().ok()
}

fn parse_date(input: &str) -> Result<NaiveDate, AgdError> {
    NaiveDate::parse_from_str(input, "%Y-%m-%d")
        .map_err(|_| AgdError::new("invalid_date", format!("invalid Org date: {input}")))
}

fn date_range(start: NaiveDate, end: NaiveDate) -> impl Iterator<Item = NaiveDate> {
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

/// One comparison per org-agenda-sorting-strategy key, first difference
/// wins; the sort is stable, so category-keep and habit-* keep collection
/// order.
fn compare_by_strategy(
    (left_urgency, left): &(i64, AgendaEntry),
    (right_urgency, right): &(i64, AgendaEntry),
    settings: &Settings,
    todo_order: &[String],
) -> Ordering {
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
            "priority" => priority_value(left.item.priority.as_deref(), settings)
                .cmp(&priority_value(right.item.priority.as_deref(), settings)),
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

fn keyword_name(keyword: &str) -> &str {
    keyword.split(['(', '!', '@']).next().unwrap_or(keyword)
}

/// org-agenda urgency: priority value (A=2000, B=1000, C=0 by default),
/// raised by days overdue for today's deadline reminders, lowered by days
/// left for upcoming ones, and raised by 99 plus days late for scheduled.
fn urgency(entry: &AgendaEntry, today: NaiveDate, settings: &Settings) -> i64 {
    let priority = priority_value(entry.item.priority.as_deref(), settings);
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

fn priority_value(priority: Option<&str>, settings: &Settings) -> i64 {
    let letter = |value: &str| value.chars().next().map_or(0, |c| c as i64);
    let lowest = letter(&settings.lowest_priority);
    let value = priority.map_or_else(|| letter(&settings.default_priority), letter);
    1000 * (lowest - value)
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
fn compare_items(left: &Item, right: &Item) -> Ordering {
    compare_priority(&left.priority, &right.priority)
        .then_with(|| left.file.cmp(&right.file))
        .then_with(|| left.line.cmp(&right.line))
}

fn compare_priority(left: &Option<String>, right: &Option<String>) -> Ordering {
    left.as_deref()
        .unwrap_or("B")
        .cmp(right.as_deref().unwrap_or("B"))
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

/// TODO-list rows in the org-batch-agenda-csv layout (type `todo`, no date).
pub fn items_csv(items: &[Item]) -> String {
    let mut output = String::new();
    for item in items {
        let mut seen_tags = BTreeSet::new();
        let tags = item
            .inherited_tags
            .iter()
            .chain(&item.tags)
            .filter(|tag| seen_tags.insert((*tag).clone()))
            .cloned()
            .collect::<Vec<_>>()
            .join(":");
        let fields = [
            item.category.clone(),
            agenda_title(item),
            "todo".to_string(),
            item.todo.clone().unwrap_or_default(),
            tags,
            String::new(),
            String::new(),
            String::new(),
            item.priority.clone().unwrap_or_default(),
        ];
        output.push_str(&fields.map(|field| csv_field(&field)).join(","));
        output.push('\n');
    }
    output
}

fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Settings;

    fn fixture(source: &str) -> (tempfile::TempDir, WorkspaceIndex) {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("agenda.org"), source).unwrap();
        let settings = Settings {
            roots: vec![temp.path().to_string_lossy().into_owned()],
            agenda_files: vec!["**/*.org".to_string()],
            ..Settings::default()
        };
        let index = WorkspaceIndex::load(settings).unwrap();
        (temp, index)
    }

    #[test]
    fn views_run_blocks_in_order_like_a_block_agenda() {
        let (_temp, index) = fixture(
            "#+TODO: TODO NEXT | DONE\n* TODO Today :work:\nSCHEDULED: <2026-10-13 Tue>\n* NEXT Later :home:\n* DONE Old :work:\n* Note :work:\n",
        );
        let view = ViewSettings {
            key: "m".to_string(),
            desc: "Mixed".to_string(),
            blocks: vec![
                ViewBlock {
                    kind: "agenda".to_string(),
                    match_expression: None,
                    span: Some("day".to_string()),
                    header: Some("Today".to_string()),
                    groups: Vec::new(),
                },
                ViewBlock {
                    kind: "todo".to_string(),
                    match_expression: Some("NEXT|DONE".to_string()),
                    span: None,
                    header: None,
                    groups: Vec::new(),
                },
                ViewBlock {
                    kind: "tags".to_string(),
                    match_expression: Some("work".to_string()),
                    span: None,
                    header: None,
                    groups: Vec::new(),
                },
            ],
        };
        let today = NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        let sections = AgendaEngine::new(&index)
            .view(&view, today, today, false)
            .unwrap();
        assert_eq!(sections[0].header, "Today");
        assert_eq!(sections[0].entries.len(), 1);
        let titles = |section: &ViewSection| -> Vec<String> {
            section
                .items
                .iter()
                .map(|item| item.title.clone())
                .collect()
        };
        assert_eq!(
            sections[1].header,
            "Global list of TODO items of type: NEXT|DONE"
        );
        assert_eq!(titles(&sections[1]), ["Later", "Old"]);
        assert_eq!(titles(&sections[2]), ["Today", "Old", "Note"]);
    }

    #[test]
    fn durations_read_as_org_duration_to_minutes_does() {
        // Expected values printed by org-duration-to-minutes in Emacs 30.2.
        for (text, minutes) in [
            ("0:10", 10.0),
            ("1:30", 90.0),
            ("10", 10.0),
            ("90", 90.0),
            ("1h", 60.0),
            ("1h 30min", 90.0),
            ("2d", 2880.0),
            ("1:05:30", 65.5),
            (" 0:10 ", 10.0),
            ("0.5", 0.5),
            ("1.5h", 90.0),
        ] {
            assert_eq!(duration_minutes(text), Some(minutes), "{text}");
        }
        assert_eq!(duration_minutes("abc"), None);
        assert_eq!(duration_minutes(""), None);
    }

    #[test]
    fn effort_groups_compare_durations_inclusively() {
        let (_temp, index) = fixture(
            "* TODO Ten as a clock\n:PROPERTIES:\n:Effort:   0:10\n:END:\n* TODO Fifteen as minutes\n:PROPERTIES:\n:Effort:   15\n:END:\n* TODO Ninety as a clock\n:PROPERTIES:\n:Effort:   1:30\n:END:\n* TODO No estimate\n",
        );
        let view = ViewSettings {
            key: "e".to_string(),
            desc: "Effort".to_string(),
            blocks: vec![ViewBlock {
                kind: "alltodo".to_string(),
                match_expression: None,
                span: None,
                header: None,
                groups: vec![
                    ViewGroup {
                        name: Some("Quick".to_string()),
                        effort_max: Some("0:15".to_string()),
                        ..ViewGroup::default()
                    },
                    ViewGroup {
                        name: Some("Long".to_string()),
                        effort_min: Some("1h".to_string()),
                        ..ViewGroup::default()
                    },
                ],
            }],
        };
        let today = NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        let sections = AgendaEngine::new(&index)
            .view(&view, today, today, false)
            .unwrap();
        let groups: Vec<(String, Vec<String>)> = sections[0]
            .groups
            .iter()
            .map(|group| {
                let titles = group.items.iter().map(|item| item.title.clone()).collect();
                (group.name.clone(), titles)
            })
            .collect();
        let expected = [
            ("Quick", vec!["Ten as a clock", "Fifteen as minutes"]),
            ("Long", vec!["Ninety as a clock"]),
            ("Other items", vec!["No estimate"]),
        ]
        .map(|(name, titles)| {
            (
                name.to_string(),
                titles.into_iter().map(str::to_string).collect::<Vec<_>>(),
            )
        });
        assert_eq!(groups, expected);
    }

    #[test]
    fn view_groups_take_rows_first_match_first_and_show_by_order() {
        let (_temp, index) = fixture(
            "#+TODO: TODO NEXT WAIT HOLD | DONE\n* TODO Alone :work:\n* NEXT Decided :home:\n* HOLD Parked\n* WAIT Waiting\n* Project\n** TODO Child\n** TODO Hot child :urgent:\n",
        );
        let group = |name: &str, expression: &str, order: i64| ViewGroup {
            name: Some(name.to_string()),
            match_expression: Some(expression.to_string()),
            order,
            ..ViewGroup::default()
        };
        let view = ViewSettings {
            key: "g".to_string(),
            desc: "Grouped".to_string(),
            blocks: vec![ViewBlock {
                kind: "alltodo".to_string(),
                match_expression: None,
                span: None,
                header: Some("Backlog".to_string()),
                groups: vec![
                    ViewGroup {
                        match_expression: Some("/HOLD".to_string()),
                        discard: true,
                        ..ViewGroup::default()
                    },
                    // Matched before the others, shown after them.
                    group("Waiting", "/WAIT", 9),
                    group("Next", "/NEXT", -1),
                    group("Urgent", "urgent", 0),
                    group("Nothing", "absent", 0),
                    ViewGroup {
                        by: Some("parent".to_string()),
                        ..ViewGroup::default()
                    },
                ],
            }],
        };
        let today = NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        let sections = AgendaEngine::new(&index)
            .view(&view, today, today, false)
            .unwrap();
        let groups: Vec<(String, Vec<String>)> = sections[0]
            .groups
            .iter()
            .map(|group| {
                let titles = group.items.iter().map(|item| item.title.clone()).collect();
                (group.name.clone(), titles)
            })
            .collect();
        let expected = [
            ("Next", vec!["Decided"]),
            ("Urgent", vec!["Hot child"]),
            ("Project", vec!["Child"]),
            ("Waiting", vec!["Waiting"]),
            ("Other items", vec!["Alone"]),
        ]
        .map(|(name, titles)| {
            (
                name.to_string(),
                titles.into_iter().map(str::to_string).collect::<Vec<_>>(),
            )
        });
        assert_eq!(groups, expected);
        assert!(sections[0].items.is_empty());
    }

    #[test]
    fn tags_todo_match_follows_emacs_syntax() {
        let (_temp, index) = fixture(
            "#+TODO: TODO NEXT WAIT | DONE\n* TODO A :work:\n* NEXT B :work:urgent:\n* WAIT C :home:\n* DONE D :home:\n* TODO E :work:someday:\n",
        );
        let titles = |expression: &str| -> Vec<String> {
            let matcher = TagTodoMatcher::parse(expression).unwrap();
            let now = NaiveDate::from_ymd_opt(2026, 10, 13)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap();
            index
                .documents()
                .flat_map(|document| document.headlines())
                .filter(|headline| matcher.matches(headline, now, index.settings()))
                .map(|headline| headline.item.title.clone())
                .collect()
        };
        assert_eq!(titles("work"), ["A", "B", "E"]);
        assert_eq!(titles("work-someday"), ["A", "B"]);
        assert_eq!(titles("work&urgent|home"), ["B", "C", "D"]);
        assert_eq!(titles("+work/NEXT|TODO"), ["A", "B", "E"]);
        assert_eq!(titles("home/!"), ["C"]);
        assert_eq!(titles("/-WAIT"), ["A", "B", "D", "E"]);
        assert_eq!(titles("work+TODO<>\"NEXT\""), ["A", "E"]);
        assert_eq!(titles("PRIORITY=\"B\"+home"), ["C", "D"]);
        assert_eq!(
            TagTodoMatcher::parse("TODO={NE}").err().unwrap().code(),
            "invalid_match"
        );
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

    #[test]
    fn evaluates_documented_org_ql_subset() {
        let (_temp, index) = fixture(
            "#+TODO: TODO NEXT | DONE\n* NEXT [#A] Report :work:\nDEADLINE: <2026-10-16 Fri>\n* TODO Other :personal:\n",
        );
        let items = AgendaEngine::new(&index)
            .query(
                "(and (todo \"NEXT\") (tags \"work\") (deadline :to 7))",
                NaiveDate::from_ymd_opt(2026, 10, 13).unwrap(),
            )
            .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Report");
    }

    #[test]
    fn csv_uses_emacs_occurrence_vocabulary() {
        let (_temp, index) = fixture("* TODO Call, bank\nSCHEDULED: <2026-10-13 Tue 10:00>\n");
        let entries = AgendaEngine::new(&index)
            .agenda(&AgendaQuery {
                from: NaiveDate::from_ymd_opt(2026, 10, 13).unwrap(),
                to: NaiveDate::from_ymd_opt(2026, 10, 13).unwrap(),
                today: NaiveDate::from_ymd_opt(2026, 10, 13).unwrap(),
                match_expression: None,
                log_mode: false,
            })
            .unwrap();
        assert!(agenda_csv(&entries).contains("\"Call, bank\",scheduled,TODO"));
    }
}
