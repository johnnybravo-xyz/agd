use std::sync::LazyLock;

use chrono::{
    Datelike, Days, Duration, Local, Months, NaiveDate, NaiveDateTime, NaiveTime, Weekday,
};
use regex::{Captures, Regex};

use crate::{AgdError, ErrorCode, PlanningTimestamp};

static ABSOLUTE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d{4}-\d{2}-\d{2})(?:[ T](\d{1,2}:\d{2}))?(?:\s+([+.]*\+\d+[hdwmy]))?(?:\s+(-\d+[hdwmy]))?$")
        .expect("absolute date regex")
});
/// `+3`, `-1w`, `+2tue`, `-wed`: a sign, a count, and a unit or weekday.
static RELATIVE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([+-])(\d*)([a-z]*)$").expect("relative date regex"));
/// `14:00`, `9:30`, `3pm`, `9am`, `12:30am`.
static CLOCK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d{1,2})(?::(\d{2}))?(am|pm)?$").expect("clock time regex"));
/// `oct 30` or `30 oct`.
static MONTH_DAY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:([a-z]+)\s+(\d{1,2})|(\d{1,2})\s+([a-z]+))$").expect("month day regex")
});

/// The date forms agd accepts wherever it asks for a date, listed in every
/// `invalid_date` error so the fix is in the message.
const DATE_FORMS: &str = "use 2026-10-30 (or 2026-10-30 14:00, 2026-10-30T14:00), oct 30 or \
30 oct, 14 (the next 14th), today, tomorrow, '.' (today), a weekday (fri), +2tue or -wed, \
+3d -1w +2m +1y (+3 alone means days), a time for today (14:00, 3pm, 9am), \
or a day then a time (fri 9am, mon 10:00)";

/// Reads a date the way Org's date prompt (org-read-date) does, relative to
/// `today`: an ISO date with optional time, repeater and warning, or a day
/// (see `prompt_day`) optionally followed by a time (see `clock_time`). A
/// time alone is today at that time.
pub fn parse_date_input(input: &str, today: NaiveDate) -> Result<PlanningTimestamp, AgdError> {
    let input = input.trim();
    if let Some(captures) = ABSOLUTE_RE.captures(input) {
        return absolute_timestamp(input, &captures);
    }
    let lowered = input.to_ascii_lowercase();
    let (day, last) = lowered
        .rsplit_once(char::is_whitespace)
        .map_or(("", lowered.as_str()), |(day, last)| (day.trim_end(), last));
    let (day, time) = match clock_time(last)? {
        Some(time) => (day, Some(time)),
        None => (lowered.as_str(), None),
    };
    let date = if day.is_empty() && time.is_some() {
        Some(today)
    } else {
        prompt_day(day, today)?
    };
    let date = date.ok_or_else(|| {
        AgdError::new(
            ErrorCode::InvalidDate,
            format!("unsupported date '{input}'; {DATE_FORMS}"),
        )
    })?;
    Ok(PlanningTimestamp {
        time: time.map(|time| time.format("%H:%M").to_string()),
        ..simple_date(date)
    })
}

/// How a date-prompt answer reads as Org text, e.g. `<2026-10-19 Mon 09:00>`
/// for `mon 9am`, so a prompt can show what it will write.
pub fn date_preview(input: &str, today: NaiveDate) -> Result<String, AgdError> {
    format_active_timestamp(&parse_date_input(input, today)?)
}

fn absolute_timestamp(input: &str, captures: &Captures) -> Result<PlanningTimestamp, AgdError> {
    let date = NaiveDate::parse_from_str(&captures[1], "%Y-%m-%d")
        .map_err(|_| AgdError::new(ErrorCode::InvalidDate, format!("invalid date: {input}")))?;
    let time = captures
        .get(2)
        .map(|matched| parse_time(matched.as_str()))
        .transpose()?;
    Ok(PlanningTimestamp {
        date: date.format("%Y-%m-%d").to_string(),
        time: time.map(|value| value.format("%H:%M").to_string()),
        end_time: None,
        repeater: captures.get(3).map(|matched| matched.as_str().to_string()),
        warning: captures.get(4).map(|matched| matched.as_str().to_string()),
        raw: None,
    })
}

/// A lowercase day as Org's date prompt reads it: `2026-10-30`, `.` or
/// `today`, `tomorrow`, a weekday (the next one, today included), `+N`/`-N`
/// (see `relative_day`), a day of the month (`14`: this month's, or next
/// month's once it has passed), or a month and day (`oct 30`, `30 oct`:
/// this year's, or next year's once it has passed). None for anything else.
fn prompt_day(day: &str, today: NaiveDate) -> Result<Option<NaiveDate>, AgdError> {
    let invalid = || AgdError::new(ErrorCode::InvalidDate, format!("invalid date: {day}"));
    match day {
        "." | "today" => return Ok(Some(today)),
        "tomorrow" => return Ok(Some(today + Duration::days(1))),
        _ => {}
    }
    if let Ok(date) = NaiveDate::parse_from_str(day, "%Y-%m-%d") {
        return Ok(Some(date));
    }
    // As org-read-date: the next such weekday, a week ahead when today is one.
    if let Some(weekday) = parse_weekday(day) {
        let distance = (weekday.num_days_from_monday() as i64
            - today.weekday().num_days_from_monday() as i64)
            .rem_euclid(7);
        let distance = if distance == 0 { 7 } else { distance };
        return Ok(Some(today + Duration::days(distance)));
    }
    if let Some(captures) = RELATIVE_RE.captures(day) {
        return relative_day(day, &captures, today);
    }
    if day.len() <= 2
        && let Ok(number) = day.parse::<u32>()
    {
        let month = today.with_day(1).expect("first of the month");
        let month = if number < today.day() {
            month
                .checked_add_months(Months::new(1))
                .ok_or_else(invalid)?
        } else {
            month
        };
        return month.with_day(number).map(Some).ok_or_else(invalid);
    }
    let Some(captures) = MONTH_DAY_RE.captures(day) else {
        return Ok(None);
    };
    let (name, number) = match (captures.get(1), captures.get(2)) {
        (Some(name), Some(number)) => (name.as_str(), number.as_str()),
        _ => (&captures[4], &captures[3]),
    };
    let (Some(month), Ok(number)) = (parse_month(name), number.parse::<u32>()) else {
        return Ok(None);
    };
    let this_year = NaiveDate::from_ymd_opt(today.year(), month, number).ok_or_else(invalid)?;
    if this_year >= today {
        return Ok(Some(this_year));
    }
    NaiveDate::from_ymd_opt(today.year() + 1, month, number)
        .map(Some)
        .ok_or_else(invalid)
}

/// `+3` and `-1w` (a count of d, w, m or y, days when the unit is left
/// out), and Org's weekday shifts: `+tue` is the next Tuesday, `+2tue` the
/// one after it, `-wed` the last Wednesday; a shift never lands on today.
fn relative_day(
    input: &str,
    captures: &Captures,
    today: NaiveDate,
) -> Result<Option<NaiveDate>, AgdError> {
    let out_of_range = || {
        AgdError::new(
            ErrorCode::InvalidDate,
            format!("date is out of range: {input}"),
        )
    };
    let forward = &captures[1] == "+";
    let (count, unit) = (&captures[2], &captures[3]);
    if count.is_empty() && unit.is_empty() {
        return Ok(None);
    }
    let amount = match count {
        "" => 1,
        count => count.parse::<u32>().map_err(|_| out_of_range())?,
    };
    if let Some(weekday) = parse_weekday(unit) {
        // org-read-date-get-relative's arithmetic.
        let mut delta = (weekday.num_days_from_monday() as i64
            - today.weekday().num_days_from_monday() as i64)
            .rem_euclid(7);
        if delta == 0 {
            delta = 7;
        }
        if !forward {
            delta -= 7;
            if delta == 0 {
                delta = -7;
            }
        }
        delta += (i64::from(amount) - 1) * if forward { 7 } else { -7 };
        return today
            .checked_add_signed(Duration::days(delta))
            .map(Some)
            .ok_or_else(out_of_range);
    }
    let (days, months) = match unit {
        "" | "d" => (amount, 0),
        "w" => (amount.checked_mul(7).ok_or_else(out_of_range)?, 0),
        "m" => (0, amount),
        "y" => (0, amount.checked_mul(12).ok_or_else(out_of_range)?),
        _ => return Ok(None),
    };
    let date = if forward {
        today
            .checked_add_days(Days::new(days.into()))
            .and_then(|date| date.checked_add_months(Months::new(months)))
    } else {
        today
            .checked_sub_days(Days::new(days.into()))
            .and_then(|date| date.checked_sub_months(Months::new(months)))
    };
    date.map(Some).ok_or_else(out_of_range)
}

/// A lowercase clock time: `14:00`, `9:30`, `3pm`, `9am`, `12:30am`. None
/// for any other token, including a bare number, which is a day of the month.
fn clock_time(token: &str) -> Result<Option<NaiveTime>, AgdError> {
    let Some(captures) = CLOCK_RE.captures(token) else {
        return Ok(None);
    };
    let (minute, meridiem) = (captures.get(2), captures.get(3));
    if minute.is_none() && meridiem.is_none() {
        return Ok(None);
    }
    let invalid = || AgdError::new(ErrorCode::InvalidDate, format!("invalid time: {token}"));
    let hour = captures[1].parse::<u32>().map_err(|_| invalid())?;
    let minute = minute.map_or(Ok(0), |minute| {
        minute.as_str().parse::<u32>().map_err(|_| invalid())
    })?;
    let hour = match meridiem.map(|meridiem| meridiem.as_str()) {
        Some(_) if !(1..=12).contains(&hour) => return Err(invalid()),
        Some("am") => hour % 12,
        Some(_) => hour % 12 + 12,
        None => hour,
    };
    NaiveTime::from_hms_opt(hour, minute, 0)
        .map(Some)
        .ok_or_else(invalid)
}

pub fn format_active_timestamp(timestamp: &PlanningTimestamp) -> Result<String, AgdError> {
    let date = NaiveDate::parse_from_str(&timestamp.date, "%Y-%m-%d").map_err(|_| {
        AgdError::new(
            ErrorCode::InvalidDate,
            format!("invalid date: {}", timestamp.date),
        )
    })?;
    let mut value = format!("<{} {}", timestamp.date, date.format("%a"));
    if let Some(time) = &timestamp.time {
        value.push(' ');
        value.push_str(time);
        if let Some(end_time) = &timestamp.end_time {
            value.push('-');
            value.push_str(end_time);
        }
    }
    if let Some(repeater) = &timestamp.repeater {
        value.push(' ');
        value.push_str(repeater);
    }
    if let Some(warning) = &timestamp.warning {
        value.push(' ');
        value.push_str(warning);
    }
    value.push('>');
    Ok(value)
}

pub(crate) fn simple_date(date: NaiveDate) -> PlanningTimestamp {
    PlanningTimestamp {
        date: date.format("%Y-%m-%d").to_string(),
        time: None,
        end_time: None,
        repeater: None,
        warning: None,
        raw: None,
    }
}

fn parse_time(input: &str) -> Result<NaiveTime, AgdError> {
    NaiveTime::parse_from_str(input, "%H:%M")
        .map_err(|_| AgdError::new(ErrorCode::InvalidDate, format!("invalid time: {input}")))
}

fn parse_weekday(input: &str) -> Option<Weekday> {
    match input.to_ascii_lowercase().as_str() {
        "mon" | "monday" => Some(Weekday::Mon),
        "tue" | "tues" | "tuesday" => Some(Weekday::Tue),
        "wed" | "wednesday" => Some(Weekday::Wed),
        "thu" | "thur" | "thurs" | "thursday" => Some(Weekday::Thu),
        "fri" | "friday" => Some(Weekday::Fri),
        "sat" | "saturday" => Some(Weekday::Sat),
        "sun" | "sunday" => Some(Weekday::Sun),
        _ => None,
    }
}

/// A lowercase month name, short or full, as its number.
fn parse_month(name: &str) -> Option<u32> {
    let month = match name {
        "jan" | "january" => 1,
        "feb" | "february" => 2,
        "mar" | "march" => 3,
        "apr" | "april" => 4,
        "may" => 5,
        "jun" | "june" => 6,
        "jul" | "july" => 7,
        "aug" | "august" => 8,
        "sep" | "sept" | "september" => 9,
        "oct" | "october" => 10,
        "nov" | "november" => 11,
        "dec" | "december" => 12,
        _ => return None,
    };
    Some(month)
}

pub fn now_local_naive() -> NaiveDateTime {
    Local::now().naive_local()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_org_relative_and_warning_dates() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        assert_eq!(parse_date_input("+2d", today).unwrap().date, "2026-10-15");
        assert_eq!(parse_date_input("fri", today).unwrap().date, "2026-10-16");
        let absolute = parse_date_input("2026-10-30 09:15 +1w -7d", today).unwrap();
        assert_eq!(absolute.time.as_deref(), Some("09:15"));
        assert_eq!(absolute.repeater.as_deref(), Some("+1w"));
        assert_eq!(absolute.warning.as_deref(), Some("-7d"));
    }

    #[test]
    fn reads_org_prompt_shorthands_and_times() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        let date = |input: &str| {
            let timestamp = parse_date_input(input, today).unwrap();
            (timestamp.date, timestamp.time)
        };
        let day = |value: &str| (value.to_string(), None);
        let at = |value: &str, time: &str| (value.to_string(), Some(time.to_string()));
        assert_eq!(date("."), day("2026-10-13"));
        assert_eq!(date("+3"), day("2026-10-16"));
        assert_eq!(date("-1d"), day("2026-10-12"));
        assert_eq!(date("-1w"), day("2026-10-06"));
        assert_eq!(date("-1m"), day("2026-09-13"));
        assert_eq!(date("+1y"), day("2027-10-13"));
        assert_eq!(date("14:00"), at("2026-10-13", "14:00"));
        assert_eq!(date("mon 10:00"), at("2026-10-19", "10:00"));
        assert_eq!(date("tomorrow 9:30"), at("2026-10-14", "09:30"));
        assert_eq!(date("+2d 14:00"), at("2026-10-15", "14:00"));
        assert_eq!(date("2026-10-30T14:00"), at("2026-10-30", "14:00"));
        assert_eq!(date("2026-10-30 14:00"), at("2026-10-30", "14:00"));

        let error = parse_date_input("someday", today).unwrap_err();
        assert_eq!(error.code(), "invalid_date");
        for form in [
            "today",
            "tomorrow",
            "fri",
            "-1w",
            "+2tue",
            "oct 30",
            "3pm",
            "2026-10-30T14:00",
            "fri 9am",
        ] {
            assert!(error.to_string().contains(form), "{form} missing: {error}");
        }
        assert!(parse_date_input("fri 25:00", today).is_err());
        assert!(parse_date_input("13pm", today).is_err());
    }

    /// Each answer is what pinned Emacs's org-read-date gives on Friday
    /// 2026-10-16, except `tomorrow`, which agd reads as a date (Org ignores
    /// the word).
    #[test]
    fn reads_dates_as_org_read_date_does() {
        let friday = NaiveDate::from_ymd_opt(2026, 10, 16).unwrap();
        let cases = [
            (".", "2026-10-16", None),
            ("+0", "2026-10-16", None),
            ("+3", "2026-10-19", None),
            ("-3", "2026-10-13", None),
            ("-1w", "2026-10-09", None),
            ("-wed", "2026-10-14", None),
            ("-fri", "2026-10-09", None),
            ("+tue", "2026-10-20", None),
            ("+fri", "2026-10-23", None),
            ("+2tue", "2026-10-27", None),
            ("sat", "2026-10-17", None),
            ("thu", "2026-10-22", None),
            ("fri", "2026-10-23", None),
            ("16", "2026-10-16", None),
            ("14", "2026-11-14", None),
            ("oct 30", "2026-10-30", None),
            ("30 Oct", "2026-10-30", None),
            ("jan 5", "2027-01-05", None),
            ("sep 15", "2027-09-15", None),
            ("14:00", "2026-10-16", Some("14:00")),
            ("3pm", "2026-10-16", Some("15:00")),
            ("9am", "2026-10-16", Some("09:00")),
            ("12am", "2026-10-16", Some("00:00")),
            ("12pm", "2026-10-16", Some("12:00")),
            ("mon 10:00", "2026-10-19", Some("10:00")),
            ("+2d 14:00", "2026-10-18", Some("14:00")),
            ("oct 30 9am", "2026-10-30", Some("09:00")),
            ("tomorrow 3pm", "2026-10-17", Some("15:00")),
        ];
        for (input, date, time) in cases {
            let timestamp = parse_date_input(input, friday).unwrap();
            assert_eq!(
                (timestamp.date.as_str(), timestamp.time.as_deref()),
                (date, time),
                "{input}"
            );
        }
        assert_eq!(
            date_preview("mon 9am", friday).unwrap(),
            "<2026-10-19 Mon 09:00>"
        );
    }
}
