use std::cmp::Ordering;
use std::collections::BTreeSet;

use chrono::{Datelike, Duration, Months, NaiveDate};

use crate::{
    AGENDA_ENTRY_SCHEMA, AgendaEntry, AgendumError, HeadlineSpan, Item, Occurrence,
    OccurrenceType, PlanningTimestamp, WorkspaceIndex,
};

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
}

#[derive(Clone, Debug)]
pub struct AgendaEngine<'a> {
    index: &'a WorkspaceIndex,
}

impl<'a> AgendaEngine<'a> {
    pub fn new(index: &'a WorkspaceIndex) -> Self {
        Self { index }
    }

    pub fn agenda(&self, query: &AgendaQuery) -> Result<Vec<AgendaEntry>, AgendumError> {
        if query.to < query.from {
            return Err(AgendumError::new(
                "invalid_range",
                "agenda end date is before its start date",
            ));
        }
        let matcher = query
            .match_expression
            .as_deref()
            .map(TagTodoMatcher::parse)
            .transpose()?;
        let mut entries = Vec::new();
        for document in self.index.documents() {
            for headline in document.headlines() {
                if matcher
                    .as_ref()
                    .is_some_and(|matcher| !matcher.matches(&headline.item))
                {
                    continue;
                }
                entries.extend(self.occurrences(headline, query.from, query.to, query.today)?);
            }
        }
        entries.sort_by(compare_entries);
        Ok(entries)
    }

    pub fn todo(&self, match_expression: Option<&str>) -> Result<Vec<Item>, AgendumError> {
        let matcher = match_expression.map(TagTodoMatcher::parse).transpose()?;
        let mut items: Vec<Item> = self
            .index
            .items()
            .filter(|item| item.todo.is_some() && !item.done)
            .filter(|item| matcher.as_ref().is_none_or(|matcher| matcher.matches(item)))
            .cloned()
            .collect();
        items.sort_by(compare_items);
        Ok(items)
    }

    pub fn query(&self, expression: &str, today: NaiveDate) -> Result<Vec<Item>, AgendumError> {
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
        from: NaiveDate,
        to: NaiveDate,
        today: NaiveDate,
    ) -> Result<Vec<AgendaEntry>, AgendumError> {
        let item = &headline.item;
        let settings = &self.index.settings().agenda;
        let mut entries = Vec::new();

        if !(item.done && settings.skip_scheduled_if_done)
            && let Some(scheduled) = &item.planning.scheduled
        {
            let dates = repeated_dates(scheduled, from, to, item.done)?;
            for date in dates {
                entries.push(entry(
                    item,
                    OccurrenceType::Scheduled,
                    date,
                    scheduled.time.clone(),
                    "Scheduled:".to_string(),
                ));
            }
            let previous = previous_occurrence(scheduled, today)?;
            if !item.done
                && previous < today
                && today >= from
                && today <= to
                && !entries
                    .iter()
                    .any(|entry| entry.occurrence.date == today.format("%Y-%m-%d").to_string())
            {
                entries.push(entry(
                    item,
                    OccurrenceType::PastScheduled,
                    today,
                    scheduled.time.clone(),
                    format!("Sched. {}x:", (today - previous).num_days()),
                ));
            }
        }

        if !(item.done && settings.skip_deadline_if_done)
            && let Some(deadline) = &item.planning.deadline
        {
            let due_dates = repeated_dates(deadline, from, to, item.done)?;
            let base_due = parse_date(&deadline.date)?;
            if base_due < today
                && deadline.repeater.is_none()
                && !item.done
                && today >= from
                && today <= to
            {
                entries.push(entry(
                    item,
                    OccurrenceType::Deadline,
                    today,
                    deadline.time.clone(),
                    format!("{} d. ago:", (today - base_due).num_days()),
                ));
            }
            for due in due_dates {
                let warning_days = deadline
                    .warning
                    .as_deref()
                    .and_then(parse_day_interval)
                    .unwrap_or(settings.deadline_warning_days)
                    .max(0);
                let warning_start = due - Duration::days(warning_days);
                if today >= from
                    && today <= to
                    && today >= warning_start
                    && today < due
                    && !item.done
                {
                    entries.push(entry(
                        item,
                        OccurrenceType::UpcomingDeadline,
                        today,
                        deadline.time.clone(),
                        format!("In {:>3} d.:", (due - today).num_days()),
                    ));
                }
                if due >= from && due <= to {
                    entries.push(entry(
                        item,
                        OccurrenceType::Deadline,
                        due,
                        deadline.time.clone(),
                        "Deadline:".to_string(),
                    ));
                }
            }
        }

        if let Some(closed) = &item.planning.closed {
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
                            entries.push(entry(
                                item,
                                OccurrenceType::Block,
                                day,
                                timestamp.timestamp.time.clone(),
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

#[derive(Clone, Debug)]
struct TagTodoMatcher {
    include_tags: BTreeSet<String>,
    exclude_tags: BTreeSet<String>,
    todo: BTreeSet<String>,
}

impl TagTodoMatcher {
    fn parse(input: &str) -> Result<Self, AgendumError> {
        let (tags, todo) = input.split_once('/').unwrap_or((input, ""));
        let mut include_tags = BTreeSet::new();
        let mut exclude_tags = BTreeSet::new();
        let mut cursor = 0;
        let bytes = tags.as_bytes();
        while cursor < bytes.len() {
            let sign = bytes[cursor] as char;
            if !matches!(sign, '+' | '-') {
                return Err(AgendumError::new(
                    "invalid_match",
                    format!("expected '+' or '-' at byte {cursor} in '{input}'"),
                ));
            }
            cursor += 1;
            let start = cursor;
            while cursor < bytes.len() && !matches!(bytes[cursor], b'+' | b'-') {
                cursor += 1;
            }
            if start == cursor {
                return Err(AgendumError::new("invalid_match", format!("empty tag in '{input}'")));
            }
            let tag = &tags[start..cursor];
            if sign == '+' {
                include_tags.insert(tag.to_string());
            } else {
                exclude_tags.insert(tag.to_string());
            }
        }
        let todo = todo
            .split('|')
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect();
        Ok(Self {
            include_tags,
            exclude_tags,
            todo,
        })
    }

    fn matches(&self, item: &Item) -> bool {
        let tags: BTreeSet<&str> = item
            .tags
            .iter()
            .chain(&item.inherited_tags)
            .map(String::as_str)
            .collect();
        self.include_tags.iter().all(|tag| tags.contains(tag.as_str()))
            && self.exclude_tags.iter().all(|tag| !tags.contains(tag.as_str()))
            && (self.todo.is_empty()
                || item
                    .todo
                    .as_ref()
                    .is_some_and(|todo| self.todo.contains(todo)))
    }
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
    fn parse(input: &str) -> Result<Self, AgendumError> {
        let tokens = tokenize(input)?;
        let mut cursor = 0;
        let expression = parse_ql_expression(&tokens, &mut cursor)?;
        if cursor != tokens.len() {
            return Err(AgendumError::new("invalid_query", "unexpected tokens after query"));
        }
        Ok(expression)
    }

    fn matches(&self, item: &Item, today: NaiveDate) -> bool {
        match self {
            Self::And(expressions) => expressions.iter().all(|expression| expression.matches(item, today)),
            Self::Or(expressions) => expressions.iter().any(|expression| expression.matches(item, today)),
            Self::Not(expression) => !expression.matches(item, today),
            Self::Todo(todo) => item.todo.as_ref() == Some(todo),
            Self::Tags(tag) => item.tags.iter().chain(&item.inherited_tags).any(|value| value == tag),
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

fn tokenize(input: &str) -> Result<Vec<Token>, AgendumError> {
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
                    return Err(AgendumError::new("invalid_query", "unterminated string"));
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

fn parse_ql_expression(tokens: &[Token], cursor: &mut usize) -> Result<QlExpr, AgendumError> {
    expect(tokens, cursor, Token::Open)?;
    let operator = word(tokens, cursor)?;
    let expression = match operator.as_str() {
        "and" | "or" => {
            let mut children = Vec::new();
            while tokens.get(*cursor) != Some(&Token::Close) {
                children.push(parse_ql_expression(tokens, cursor)?);
            }
            if children.is_empty() {
                return Err(AgendumError::new("invalid_query", format!("{operator} needs operands")));
            }
            if operator == "and" { QlExpr::And(children) } else { QlExpr::Or(children) }
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
                return Err(AgendumError::new("invalid_query", format!("{operator} supports only :to")));
            }
            let days = word(tokens, cursor)?.parse::<i64>().map_err(|_| {
                AgendumError::new("invalid_query", format!("{operator} :to needs an integer"))
            })?;
            if operator == "deadline" { QlExpr::Deadline(days) } else { QlExpr::Scheduled(days) }
        }
        _ => return Err(AgendumError::new("invalid_query", format!("unsupported selector: {operator}"))),
    };
    expect(tokens, cursor, Token::Close)?;
    Ok(expression)
}

fn expect(tokens: &[Token], cursor: &mut usize, expected: Token) -> Result<(), AgendumError> {
    if tokens.get(*cursor) == Some(&expected) {
        *cursor += 1;
        Ok(())
    } else {
        Err(AgendumError::new("invalid_query", format!("expected {expected:?}")))
    }
}

fn word(tokens: &[Token], cursor: &mut usize) -> Result<String, AgendumError> {
    match tokens.get(*cursor) {
        Some(Token::Word(value)) => {
            *cursor += 1;
            Ok(value.clone())
        }
        _ => Err(AgendumError::new("invalid_query", "expected a word")),
    }
}

fn string_value(tokens: &[Token], cursor: &mut usize) -> Result<String, AgendumError> {
    match tokens.get(*cursor) {
        Some(Token::String(value)) => {
            *cursor += 1;
            Ok(value.clone())
        }
        _ => Err(AgendumError::new("invalid_query", "expected a quoted string")),
    }
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
            entry.item.title.clone(),
            occurrence_name(entry.occurrence.kind).to_string(),
            entry.item.todo.clone().unwrap_or_default(),
            tags,
            entry.occurrence.date.clone(),
            entry.occurrence.time.clone().unwrap_or_default(),
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
) -> Result<Vec<NaiveDate>, AgendumError> {
    let mut date = parse_date(&timestamp.date)?;
    if done {
        return Ok((date >= from && date <= to).then_some(date).into_iter().collect());
    }
    let Some(repeater) = timestamp.repeater.as_deref() else {
        return Ok((date >= from && date <= to).then_some(date).into_iter().collect());
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

fn previous_occurrence(
    timestamp: &PlanningTimestamp,
    today: NaiveDate,
) -> Result<NaiveDate, AgendumError> {
    let mut date = parse_date(&timestamp.date)?;
    let Some(repeater) = timestamp.repeater.as_deref() else {
        return Ok(date);
    };
    let (_, amount, unit) = parse_repeater(repeater)?;
    while date < today {
        let next = add_period(date, amount, unit)?;
        if next > today {
            break;
        }
        date = next;
    }
    Ok(date)
}

fn parse_repeater(input: &str) -> Result<(&str, u64, char), AgendumError> {
    let (kind, value) = if let Some(value) = input.strip_prefix("++") {
        ("++", value)
    } else if let Some(value) = input.strip_prefix(".+") {
        (".+", value)
    } else if let Some(value) = input.strip_prefix('+') {
        ("+", value)
    } else {
        return Err(AgendumError::new("invalid_repeater", format!("invalid repeater: {input}")));
    };
    let unit = value.chars().next_back().ok_or_else(|| {
        AgendumError::new("invalid_repeater", format!("invalid repeater: {input}"))
    })?;
    let amount = value[..value.len() - unit.len_utf8()]
        .parse::<u64>()
        .map_err(|_| AgendumError::new("invalid_repeater", format!("invalid repeater: {input}")))?;
    if amount == 0 {
        return Err(AgendumError::new("invalid_repeater", "repeater interval must be positive"));
    }
    Ok((kind, amount, unit))
}

fn add_period(date: NaiveDate, amount: u64, unit: char) -> Result<NaiveDate, AgendumError> {
    let next = match unit {
        'd' => date.checked_add_signed(Duration::days(amount as i64)),
        'w' => date.checked_add_signed(Duration::weeks(amount as i64)),
        'm' => date.checked_add_months(Months::new(amount as u32)),
        'y' => date.checked_add_months(Months::new((amount * 12) as u32)),
        _ => None,
    };
    next.ok_or_else(|| AgendumError::new("invalid_repeater", "repeater date is out of range"))
}

fn parse_day_interval(input: &str) -> Option<i64> {
    let value = input.strip_prefix('-')?;
    let days = value.strip_suffix('d')?;
    days.parse().ok()
}

fn parse_date(input: &str) -> Result<NaiveDate, AgendumError> {
    NaiveDate::parse_from_str(input, "%Y-%m-%d")
        .map_err(|_| AgendumError::new("invalid_date", format!("invalid Org date: {input}")))
}

fn date_range(start: NaiveDate, end: NaiveDate) -> impl Iterator<Item = NaiveDate> {
    (0..=(end - start).num_days().max(-1)).map(move |days| start + Duration::days(days))
}

fn compare_entries(left: &AgendaEntry, right: &AgendaEntry) -> Ordering {
    left.occurrence
        .date
        .cmp(&right.occurrence.date)
        .then_with(|| compare_optional_time(&left.occurrence.time, &right.occurrence.time))
        .then_with(|| compare_priority(&left.item.priority, &right.item.priority))
        .then_with(|| occurrence_rank(left.occurrence.kind).cmp(&occurrence_rank(right.occurrence.kind)))
        .then_with(|| left.item.category.cmp(&right.item.category))
        .then_with(|| left.item.title.cmp(&right.item.title))
        .then_with(|| left.item.file.cmp(&right.item.file))
        .then_with(|| left.item.line.cmp(&right.item.line))
}

fn compare_items(left: &Item, right: &Item) -> Ordering {
    compare_priority(&left.priority, &right.priority)
        .then_with(|| left.category.cmp(&right.category))
        .then_with(|| left.file.cmp(&right.file))
        .then_with(|| left.line.cmp(&right.line))
}

fn compare_optional_time(left: &Option<String>, right: &Option<String>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn compare_priority(left: &Option<String>, right: &Option<String>) -> Ordering {
    left.as_deref()
        .unwrap_or("B")
        .cmp(right.as_deref().unwrap_or("B"))
}

fn occurrence_rank(kind: OccurrenceType) -> u8 {
    match kind {
        OccurrenceType::PastScheduled | OccurrenceType::Scheduled => 0,
        OccurrenceType::Deadline => 1,
        OccurrenceType::Block => 2,
        OccurrenceType::Timestamp => 3,
        OccurrenceType::UpcomingDeadline => 4,
        OccurrenceType::Closed => 5,
        OccurrenceType::Todo => 6,
    }
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
        let mut settings = Settings::default();
        settings.files.roots = vec![temp.path().to_string_lossy().into_owned()];
        settings.files.agenda = vec!["**/*.org".to_string()];
        let index = WorkspaceIndex::load(settings).unwrap();
        (temp, index)
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
            })
            .unwrap();
        assert!(entries.iter().any(|entry| {
            entry.occurrence.kind == OccurrenceType::PastScheduled
        }));
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
        let (_temp, index) =
            fixture("* TODO Call, bank\nSCHEDULED: <2026-10-13 Tue 10:00>\n");
        let entries = AgendaEngine::new(&index)
            .agenda(&AgendaQuery {
                from: NaiveDate::from_ymd_opt(2026, 10, 13).unwrap(),
                to: NaiveDate::from_ymd_opt(2026, 10, 13).unwrap(),
                today: NaiveDate::from_ymd_opt(2026, 10, 13).unwrap(),
                match_expression: None,
            })
            .unwrap();
        assert!(agenda_csv(&entries).contains("\"Call, bank\",scheduled,TODO"));
    }
}
