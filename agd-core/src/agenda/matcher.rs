//! Emacs's tags/TODO match syntax (`work-someday|home/!NEXT`), with property
//! terms (`TODO="NEXT"`, `Effort<15`, `CLOSED>="<-3w>"`) compared as Org
//! compares them.
//!
//! Regexp terms (`{...}`) are refused with `invalid_match` rather than
//! guessed at. A property is read from the heading itself, not inherited, as
//! Org's matcher does by default.

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::sync::LazyLock;

use chrono::{Duration, Local, NaiveDate, NaiveDateTime, NaiveTime};
use regex::Regex;

use crate::{AgdError, ErrorCode, HeadlineSpan, PlanningTimestamp, Settings};

#[derive(Clone, Debug)]
// Emacs tags/todo match syntax: `|`-separated alternatives of terms joined by
// `&`, `+`, `-` or nothing, then an optional `/` TODO part (`NEXT|WAIT`,
// `-DONE`, a leading `!` for not-done states). A term is a tag (`work`) or a
// property comparison (`TODO="NEXT"`, `Effort<15`, `CLOSED>="<-3w>"`) with
// Org's semantics; regexp terms (`{...}`) are rejected explicitly.
pub(super) struct TagTodoMatcher {
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
pub(super) fn matcher_now(today: NaiveDate) -> NaiveDateTime {
    today.and_time(Local::now().time())
}

impl TagTodoMatcher {
    pub(super) fn parse(input: &str) -> Result<Self, AgdError> {
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

    pub(super) fn matches(
        &self,
        headline: &HeadlineSpan,
        now: NaiveDateTime,
        settings: &Settings,
    ) -> bool {
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
        |message: String| AgdError::new(ErrorCode::InvalidMatch, format!("{message} in '{input}'"));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agenda::test_support::fixture;

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
            TagTodoMatcher::parse("TODO={NE}")
                .err()
                .unwrap()
                .code()
                .as_str(),
            "invalid_match"
        );
    }
}
