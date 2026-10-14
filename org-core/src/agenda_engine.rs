use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::sync::LazyLock;

use chrono::{Datelike, Duration, Months, NaiveDate};
use regex::Regex;

use serde::Serialize;

use crate::{
    AGENDA_ENTRY_SCHEMA, AgdError, AgendaEntry, HeadlineSpan, Item, Occurrence, OccurrenceType,
    PlanningTimestamp, Settings, ViewBlock, ViewSettings, WorkspaceIndex,
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
    TagTodoMatcher::parse(&expression).map(|_| ())
}

/// One block of a saved view: agenda rows for an agenda block, otherwise a
/// list of items. `from`/`to` are set for agenda blocks only.
#[derive(Clone, Debug, Serialize)]
pub struct ViewSection {
    pub header: String,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
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
                    .is_some_and(|matcher| !matcher.matches(&headline.item))
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
        let settings = self.index.settings();
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

    pub fn todo(&self, match_expression: Option<&str>) -> Result<Vec<Item>, AgdError> {
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
    pub fn tags(&self, match_expression: &str) -> Result<Vec<Item>, AgdError> {
        let matcher = TagTodoMatcher::parse(match_expression)?;
        let mut items: Vec<Item> = self
            .index
            .items()
            .filter(|item| matcher.matches(item))
            .cloned()
            .collect();
        items.sort_by(compare_items);
        Ok(items)
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
                    }
                }
                kind => {
                    let keywords = match_expression.as_deref().unwrap_or("");
                    let (items, default_header) = match kind {
                        "alltodo" => (
                            self.todo(None)?,
                            "Global list of TODO items of type: ALL".to_string(),
                        ),
                        // `(todo "DELEGATED")` lists that keyword even when it is
                        // a done state; only alltodo and tags-todo skip done.
                        "todo" if !keywords.is_empty() => (
                            self.tags(&format!("/{keywords}"))?,
                            format!("Global list of TODO items of type: {keywords}"),
                        ),
                        "todo" => (
                            self.todo(None)?,
                            "Global list of TODO items of type: ALL".to_string(),
                        ),
                        "tags" => (
                            self.tags(keywords)?,
                            format!("Headlines with TAGS match: {keywords}"),
                        ),
                        _ => (
                            self.todo(Some(keywords))?,
                            format!("Headlines with TAGS match: {keywords}"),
                        ),
                    };
                    ViewSection {
                        header: block.header.clone().unwrap_or(default_header),
                        from: None,
                        to: None,
                        entries: Vec::new(),
                        items,
                    }
                }
            };
            sections.push(section);
        }
        Ok(sections)
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

#[derive(Clone, Debug)]
// Emacs tags/todo match syntax: `|`-separated alternatives of tag terms
// (`work`, `+work`, `-someday`, joined by `&` or nothing), then an optional
// `/` TODO part (`NEXT|WAIT`, `-DONE`, a leading `!` for not-done states).
// Property comparisons such as `PRIORITY="A"` are rejected explicitly.
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

    fn matches(&self, item: &Item) -> bool {
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
            });
        let todo = item.todo.as_deref();
        tags_match
            && (!self.todo_not_done || (todo.is_some() && !item.done))
            && (self.todo_include.is_empty()
                || todo.is_some_and(|todo| self.todo_include.contains(todo)))
            && todo.is_none_or(|todo| !self.todo_exclude.contains(todo))
    }
}

fn parse_tag_terms(alternative: &str, input: &str) -> Result<TagTerms, AgdError> {
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
            return Err(AgdError::new(
                "invalid_match",
                format!(
                    "expected a tag before {found} in '{input}'; property and regexp matches are not supported"
                ),
            ));
        }
        if include {
            terms.include.insert(tag);
        } else {
            terms.exclude.insert(tag);
        }
    }
    Ok(terms)
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
                },
                ViewBlock {
                    kind: "todo".to_string(),
                    match_expression: Some("NEXT|DONE".to_string()),
                    span: None,
                    header: None,
                },
                ViewBlock {
                    kind: "tags".to_string(),
                    match_expression: Some("work".to_string()),
                    span: None,
                    header: None,
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
    fn tags_todo_match_follows_emacs_syntax() {
        let (_temp, index) = fixture(
            "#+TODO: TODO NEXT WAIT | DONE\n* TODO A :work:\n* NEXT B :work:urgent:\n* WAIT C :home:\n* DONE D :home:\n* TODO E :work:someday:\n",
        );
        let titles = |expression: &str| -> Vec<String> {
            index
                .items()
                .filter(|item| TagTodoMatcher::parse(expression).unwrap().matches(item))
                .map(|item| item.title.clone())
                .collect()
        };
        assert_eq!(titles("work"), ["A", "B", "E"]);
        assert_eq!(titles("work-someday"), ["A", "B"]);
        assert_eq!(titles("work&urgent|home"), ["B", "C", "D"]);
        assert_eq!(titles("+work/NEXT|TODO"), ["A", "B", "E"]);
        assert_eq!(titles("home/!"), ["C"]);
        assert_eq!(titles("/-WAIT"), ["A", "B", "D", "E"]);
        assert_eq!(
            TagTodoMatcher::parse("PRIORITY=\"A\"")
                .err()
                .unwrap()
                .code(),
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
