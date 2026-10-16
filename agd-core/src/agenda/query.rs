//! `AgendaEngine::query`: the org-ql subset agd supports (`and`, `or`, `not`,
//! `todo`, `done`, `tags`, `priority`, `category`, `text`, `deadline :to` and
//! `scheduled :to`), with org-ql's meanings.
//!
//! An unknown predicate or malformed query is an `invalid_query` error that
//! says what was expected and lists every supported predicate; nothing is
//! silently ignored.

use chrono::{Duration, NaiveDate};

use crate::{AgdError, ErrorCode, Item};

use super::engine::{AgendaEngine, parse_date};
use super::sort::compare_items;

impl<'a> AgendaEngine<'a> {
    pub fn query(&self, expression: &str, today: NaiveDate) -> Result<Vec<Item>, AgdError> {
        let expression = QlExpr::parse(expression)?;
        let mut items: Vec<Item> = self
            .index
            .items()
            .filter(|item| expression.matches(item, today))
            .cloned()
            .collect();
        items.sort_by(|left, right| compare_items(left, right, self.index));
        Ok(items)
    }
}

/// The org-ql predicates `AgendaEngine::query` supports, as its errors list
/// them.
const QL_PREDICATES: &str = "(and ...), (or ...), (not ...), (todo), (todo \"KEYWORD\" ...), \
(done), (tags \"TAG\"), (priority \"A\"), (category \"NAME\"), (text \"WORDS\"), \
(deadline :to DAYS), (scheduled :to DAYS)";

/// An org-ql expression, with org-ql's meanings: bare `(todo)` is any
/// not-done TODO keyword, and `:to DAYS` is on or before today plus DAYS,
/// past dates included.
#[derive(Clone, Debug)]
enum QlExpr {
    And(Vec<QlExpr>),
    Or(Vec<QlExpr>),
    Not(Box<QlExpr>),
    /// Empty: any not-done TODO keyword; else one of these keywords.
    Todo(Vec<String>),
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
            return Err(invalid_query(
                "expected the query to end after its first predicate; combine predicates with (and ...) or (or ...)",
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
            Self::Todo(keywords) if keywords.is_empty() => item.todo.is_some() && !item.done,
            Self::Todo(keywords) => item
                .todo
                .as_ref()
                .is_some_and(|todo| keywords.contains(todo)),
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
                .is_some_and(|date| date <= today + Duration::days(*days)),
            Self::Scheduled(days) => item
                .planning
                .scheduled
                .as_ref()
                .and_then(|timestamp| parse_date(&timestamp.date).ok())
                .is_some_and(|date| date <= today + Duration::days(*days)),
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
                    return Err(invalid_query("expected a closing `\"` for a string"));
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
    expect(tokens, cursor, Token::Open, "`(` to start a predicate")?;
    let operator = word(tokens, cursor, "a predicate name after `(`")?;
    let expression = match operator.as_str() {
        "and" | "or" => {
            let mut children = Vec::new();
            while !matches!(tokens.get(*cursor), Some(Token::Close) | None) {
                children.push(parse_ql_expression(tokens, cursor)?);
            }
            if children.is_empty() {
                return Err(invalid_query(format!(
                    "expected one or more predicates inside ({operator} ...)"
                )));
            }
            if operator == "and" {
                QlExpr::And(children)
            } else {
                QlExpr::Or(children)
            }
        }
        "not" => QlExpr::Not(Box::new(parse_ql_expression(tokens, cursor)?)),
        "todo" => {
            let mut keywords = Vec::new();
            while let Some(Token::String(keyword)) = tokens.get(*cursor) {
                keywords.push(keyword.clone());
                *cursor += 1;
            }
            QlExpr::Todo(keywords)
        }
        "tags" => QlExpr::Tags(string_value(tokens, cursor, "(tags \"work\")")?),
        "priority" => QlExpr::Priority(string_value(tokens, cursor, "(priority \"A\")")?),
        "category" => QlExpr::Category(string_value(tokens, cursor, "(category \"home\")")?),
        "text" => QlExpr::Text(string_value(tokens, cursor, "(text \"report\")")?),
        "done" => QlExpr::Done,
        "deadline" | "scheduled" => {
            let usage = format!("`:to DAYS`, as in ({operator} :to 7)");
            if word(tokens, cursor, &usage)? != ":to" {
                return Err(invalid_query(format!("expected {usage}")));
            }
            let days = word(tokens, cursor, &usage)?
                .parse::<i64>()
                .map_err(|_| invalid_query(format!("expected {usage}")))?;
            if operator == "deadline" {
                QlExpr::Deadline(days)
            } else {
                QlExpr::Scheduled(days)
            }
        }
        _ => {
            return Err(invalid_query(format!("unknown predicate `{operator}`")));
        }
    };
    expect(
        tokens,
        cursor,
        Token::Close,
        &format!("`)` to close ({operator} ...)"),
    )?;
    Ok(expression)
}

/// An `invalid_query` error: what was expected, then the supported
/// predicates.
fn invalid_query(problem: impl std::fmt::Display) -> AgdError {
    AgdError::new(
        ErrorCode::InvalidQuery,
        format!("{problem}; supported predicates: {QL_PREDICATES}"),
    )
}

fn expect(
    tokens: &[Token],
    cursor: &mut usize,
    expected: Token,
    what: &str,
) -> Result<(), AgdError> {
    if tokens.get(*cursor) == Some(&expected) {
        *cursor += 1;
        Ok(())
    } else {
        Err(invalid_query(format!("expected {what}")))
    }
}

fn word(tokens: &[Token], cursor: &mut usize, what: &str) -> Result<String, AgdError> {
    match tokens.get(*cursor) {
        Some(Token::Word(value)) => {
            *cursor += 1;
            Ok(value.clone())
        }
        _ => Err(invalid_query(format!("expected {what}"))),
    }
}

fn string_value(tokens: &[Token], cursor: &mut usize, example: &str) -> Result<String, AgdError> {
    match tokens.get(*cursor) {
        Some(Token::String(value)) => {
            *cursor += 1;
            Ok(value.clone())
        }
        _ => Err(invalid_query(format!(
            "expected a quoted string, as in {example}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agenda::test_support::fixture;

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
    fn org_ql_deadline_to_includes_past_dates_and_bare_todo_means_not_done() {
        let (_temp, index) = fixture(
            "#+TODO: TODO NEXT WAIT | DONE\n* TODO Overdue\nDEADLINE: <2026-10-11 Sun>\n* NEXT Soon\nDEADLINE: <2026-10-20 Tue>\n* WAIT Later\nDEADLINE: <2026-10-21 Wed>\n* DONE Finished\nDEADLINE: <2026-10-12 Mon>\n* Plain note\n",
        );
        let today = NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        let titles = |expression: &str| -> Vec<String> {
            AgendaEngine::new(&index)
                .query(expression, today)
                .unwrap()
                .into_iter()
                .map(|item| item.title)
                .collect()
        };
        // Two days past is within `:to 7`, as in org-ql; so is a done item.
        assert_eq!(titles("(deadline :to 7)"), ["Overdue", "Soon", "Finished"]);
        assert_eq!(titles("(scheduled :to 7)"), Vec::<String>::new());
        assert_eq!(titles("(todo)"), ["Overdue", "Soon", "Later"]);
        assert_eq!(titles("(and (todo) (deadline :to 7))"), ["Overdue", "Soon"]);
        assert_eq!(titles("(todo \"NEXT\" \"WAIT\")"), ["Soon", "Later"]);
    }

    #[test]
    fn org_ql_errors_say_what_was_expected_and_list_the_predicates() {
        let (_temp, index) = fixture("* TODO Task\n");
        let today = NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        let error = |expression: &str| {
            let error = AgendaEngine::new(&index)
                .query(expression, today)
                .unwrap_err();
            assert_eq!(error.code().as_str(), "invalid_query");
            error.to_string()
        };
        let unknown = error("(heading \"x\")");
        assert!(
            unknown.starts_with("unknown predicate `heading`; supported predicates: (and ...)")
        );
        assert!(unknown.contains("(todo \"KEYWORD\" ...)"));
        assert!(unknown.contains("(deadline :to DAYS)"));
        assert!(
            error("(tags work)").starts_with("expected a quoted string, as in (tags \"work\")")
        );
        assert!(error("(deadline 7)").starts_with("expected `:to DAYS`, as in (deadline :to 7)"));
        assert!(error("todo").starts_with("expected `(` to start a predicate"));
        assert!(error("(and (todo)").starts_with("expected `)` to close (and ...)"));
        assert!(error("(todo) (done)").starts_with("expected the query to end"));
    }
}
