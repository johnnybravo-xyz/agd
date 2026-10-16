//! Repeaters for agenda dates: the days a repeating timestamp shows on within
//! a range, and the interval a `+N`, `++N` or `.+N` cookie names (a habit's
//! `/max` part is ignored).
//!
//! It only reads dates; moving a repeating task on when it is marked done is
//! the write engine's.

use chrono::{Duration, Months, NaiveDate};

use crate::{AgdError, ErrorCode, PlanningTimestamp};

use super::engine::parse_date;

pub(super) fn repeated_dates(
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
pub(super) fn planning_dates(
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

/// A repeater's kind (`+`, `++` or `.+`), interval and unit (`d`, `w`,
/// `m`, `y`): `.+2d/3d` is `(".+", 2, 'd')`. The one reader of repeater
/// text, for the agenda's dates and for completing a repeating task.
pub(crate) fn parse_repeater(input: &str) -> Result<(&str, u64, char), AgdError> {
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
            ErrorCode::InvalidRepeater,
            format!("invalid repeater: {input}"),
        ));
    };
    let unit = value.chars().next_back().ok_or_else(|| {
        AgdError::new(
            ErrorCode::InvalidRepeater,
            format!("invalid repeater: {input}"),
        )
    })?;
    let amount = value[..value.len() - unit.len_utf8()]
        .parse::<u64>()
        .map_err(|_| {
            AgdError::new(
                ErrorCode::InvalidRepeater,
                format!("invalid repeater: {input}"),
            )
        })?;
    if amount == 0 {
        return Err(AgdError::new(
            ErrorCode::InvalidRepeater,
            "repeater interval must be positive",
        ));
    }
    Ok((kind, amount, unit))
}

/// `date` moved on by `amount` units of a repeater's unit.
pub(crate) fn add_period(date: NaiveDate, amount: u64, unit: char) -> Result<NaiveDate, AgdError> {
    let next = match unit {
        'd' => date.checked_add_signed(Duration::days(amount as i64)),
        'w' => date.checked_add_signed(Duration::weeks(amount as i64)),
        'm' => date.checked_add_months(Months::new(amount as u32)),
        'y' => date.checked_add_months(Months::new((amount * 12) as u32)),
        _ => None,
    };
    next.ok_or_else(|| AgdError::new(ErrorCode::InvalidRepeater, "repeater date is out of range"))
}
