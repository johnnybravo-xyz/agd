//! Repeaters for agenda dates: the days a repeating timestamp shows on within
//! a range, the date it moves on to when its task is done ([`next_repeat`]),
//! and the interval a `+N`, `++N` or `.+N` cookie names (a habit's `/max` part
//! is ignored).
//!
//! It only reads dates; writing the moved date is the write engine's.

use chrono::{Duration, Months, NaiveDate};

use crate::{AgdError, ErrorCode, PlanningTimestamp};

use crate::date::parse_org_date;

/// Days in [from, to] a timestamp falls on: its date, and every repeat of
/// it when it repeats.
pub(super) fn repeated_dates(
    timestamp: &PlanningTimestamp,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<NaiveDate>, AgdError> {
    let mut date = parse_org_date(&timestamp.date)?;
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
    let base = parse_org_date(&timestamp.date)?;
    let mut dates = Vec::new();
    if base >= from && base <= to {
        dates.push(base);
    }
    if done {
        return Ok(dates);
    }
    for date in repeated_dates(timestamp, from, to)? {
        if date > today && date != base {
            dates.push(date);
        }
    }
    Ok(dates)
}

/// The date a repeating timestamp moves on to when its task is marked done
/// on `today`, as org-auto-repeat-maybe moves it: `+1w` a week from its
/// date, `++1w` by weeks until after today, `.+1w` a week from today.
pub fn next_repeat(timestamp: &PlanningTimestamp, today: NaiveDate) -> Result<NaiveDate, AgdError> {
    let repeater = timestamp
        .repeater
        .as_deref()
        .ok_or_else(|| AgdError::new(ErrorCode::InvalidRepeater, "timestamp has no repeater"))?;
    let (kind, amount, unit) = parse_repeater(repeater)?;
    if kind == ".+" {
        return add_period(today, amount, unit);
    }
    let mut next = add_period(parse_org_date(&timestamp.date)?, amount, unit)?;
    while kind == "++" && next <= today {
        next = add_period(next, amount, unit)?;
    }
    Ok(next)
}

/// A repeater's kind (`+`, `++` or `.+`), interval and unit (`d`, `w`,
/// `m`, `y`): `.+2d/3d` is `(".+", 2, 'd')`. The one reader of repeater
/// text, for the agenda's dates and for completing a repeating task.
fn parse_repeater(input: &str) -> Result<(&str, u64, char), AgdError> {
    // Habit repeaters carry a maximum interval, e.g. `.+2d/3d`; only the
    // repeat interval matters for agenda dates.
    let input = input.split('/').next().unwrap_or(input);
    let invalid = || {
        AgdError::new(
            ErrorCode::InvalidRepeater,
            format!("invalid repeater: {input}"),
        )
    };
    let (kind, value) = if let Some(value) = input.strip_prefix("++") {
        ("++", value)
    } else if let Some(value) = input.strip_prefix(".+") {
        (".+", value)
    } else if let Some(value) = input.strip_prefix('+') {
        ("+", value)
    } else {
        return Err(invalid());
    };
    let unit = value.chars().next_back().ok_or_else(invalid)?;
    let amount = value[..value.len() - unit.len_utf8()]
        .parse::<u64>()
        .map_err(|_| invalid())?;
    if amount == 0 {
        return Err(AgdError::new(
            ErrorCode::InvalidRepeater,
            "repeater interval must be positive",
        ));
    }
    Ok((kind, amount, unit))
}

/// `date` moved on by `amount` units of a repeater's unit.
fn add_period(date: NaiveDate, amount: u64, unit: char) -> Result<NaiveDate, AgdError> {
    let next = match unit {
        'd' => date.checked_add_signed(Duration::days(amount as i64)),
        'w' => date.checked_add_signed(Duration::weeks(amount as i64)),
        'm' => date.checked_add_months(Months::new(amount as u32)),
        'y' => date.checked_add_months(Months::new((amount * 12) as u32)),
        _ => None,
    };
    next.ok_or_else(|| AgdError::new(ErrorCode::InvalidRepeater, "repeater date is out of range"))
}
