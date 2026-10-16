use chrono::{Datelike, Duration, Months, NaiveDate};
use serde::Serialize;

use crate::{AgdError, ErrorCode, WeekStart};

/// How many days an agenda shows, as org-agenda-span. config.toml writes it
/// as "day", "week", "month" or a TOML integer; command-line and MCP input
/// is the same names or a number of days (`AgendaSpan::from_str`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgendaSpan {
    Day,
    /// Seven days from the week start.
    Week,
    /// The calendar month.
    Month,
    /// A number of days other than 1 and 7, which are `Day` and `Week` as
    /// in Org; 14 days also start on the week start.
    Days(u32),
}

const SPAN_CHOICES: &str = "day, week, month or a number of days";

impl AgendaSpan {
    /// A span of `days` days, as Org reads an integer org-agenda-span.
    pub fn days(days: i64) -> Result<Self, String> {
        match days {
            1 => Ok(Self::Day),
            7 => Ok(Self::Week),
            days if days > 1 => u32::try_from(days)
                .map(Self::Days)
                .map_err(|_| format!("a span of {days} days is too long")),
            _ => Err(format!("a span is at least 1 day, not {days}")),
        }
    }

    fn named(name: &str) -> Option<Self> {
        match name {
            "day" => Some(Self::Day),
            "week" => Some(Self::Week),
            "month" => Some(Self::Month),
            _ => None,
        }
    }

    /// The first and last day shown for an agenda that starts at `from`.
    pub fn bounds(self, from: NaiveDate, week_start: WeekStart) -> (NaiveDate, NaiveDate) {
        match self {
            Self::Day => (from, from),
            Self::Week => {
                let start = week_containing(from, week_start);
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
            Self::Days(days) => {
                let start = if days == 14 {
                    week_containing(from, week_start)
                } else {
                    from
                };
                (start, start + Duration::days(i64::from(days) - 1))
            }
        }
    }
}

/// The week start on or before `day`.
fn week_containing(day: NaiveDate, week_start: WeekStart) -> NaiveDate {
    let desired = match week_start {
        WeekStart::Monday => 0,
        WeekStart::Sunday => 6,
    };
    let current = day.weekday().num_days_from_monday() as i64;
    day - Duration::days((current - desired).rem_euclid(7))
}

impl std::fmt::Display for AgendaSpan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Day => formatter.write_str("day"),
            Self::Week => formatter.write_str("week"),
            Self::Month => formatter.write_str("month"),
            Self::Days(days) => write!(formatter, "{days}"),
        }
    }
}

impl std::str::FromStr for AgendaSpan {
    type Err = AgdError;

    /// day, week, month or a number of days such as "10".
    fn from_str(value: &str) -> Result<Self, AgdError> {
        let span_error = |detail: String| {
            AgdError::new(ErrorCode::InvalidSpan, format!("agenda span: {detail}"))
        };
        if let Some(span) = Self::named(value) {
            return Ok(span);
        }
        let days = value
            .parse::<i64>()
            .map_err(|_| span_error(format!("expected {SPAN_CHOICES}, not \"{value}\"")))?;
        Self::days(days).map_err(span_error)
    }
}

impl Serialize for AgendaSpan {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Days(days) => serializer.serialize_u32(*days),
            named => serializer.serialize_str(&named.to_string()),
        }
    }
}

impl<'de> serde::Deserialize<'de> for AgendaSpan {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(AgendaSpanVisitor)
    }
}

/// A span name as a string, or a number of days as an integer. A quoted
/// number is an error that says to drop the quotes.
struct AgendaSpanVisitor;

impl serde::de::Visitor<'_> for AgendaSpanVisitor {
    type Value = AgendaSpan;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(SPAN_CHOICES)
    }

    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<AgendaSpan, E> {
        if let Some(span) = AgendaSpan::named(value) {
            return Ok(span);
        }
        if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(E::custom(format!(
                "write a number of days without quotes: {value}, not \"{value}\""
            )));
        }
        Err(E::custom(format!(
            "expected {SPAN_CHOICES}, not \"{value}\""
        )))
    }

    fn visit_i64<E: serde::de::Error>(self, days: i64) -> Result<AgendaSpan, E> {
        AgendaSpan::days(days).map_err(E::custom)
    }

    fn visit_u64<E: serde::de::Error>(self, days: u64) -> Result<AgendaSpan, E> {
        AgendaSpan::days(i64::try_from(days).unwrap_or(i64::MAX)).map_err(E::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_count_spans_start_where_org_starts_them() {
        let date = |text: &str| NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap();
        // 2026-10-15 is a Thursday.
        let thursday = date("2026-10-15");
        assert_eq!(
            AgendaSpan::Days(10).bounds(thursday, WeekStart::Monday),
            (thursday, date("2026-10-24"))
        );
        // 14 days start on the week start, as 7 (a week) do.
        assert_eq!(
            AgendaSpan::Days(14).bounds(thursday, WeekStart::Sunday),
            (date("2026-10-11"), date("2026-10-24"))
        );
        assert_eq!(AgendaSpan::days(1), Ok(AgendaSpan::Day));
        assert_eq!(AgendaSpan::days(7), Ok(AgendaSpan::Week));
        assert_eq!("10".parse::<AgendaSpan>().unwrap(), AgendaSpan::Days(10));
        assert_eq!("month".parse::<AgendaSpan>().unwrap(), AgendaSpan::Month);
        let error = "wek".parse::<AgendaSpan>().unwrap_err();
        assert_eq!(error.code(), "invalid_span");
        assert_eq!(
            error.to_string(),
            "agenda span: expected day, week, month or a number of days, not \"wek\""
        );
    }
}
