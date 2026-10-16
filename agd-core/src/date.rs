//! Org's date prompt. [`read_date`] reads the date at the start of some words
//! the way org-read-date does (`fri`, `+2d`, `-wed`, `oct 30`, `3pm`, `mon
//! 10:00`, an ISO date), with a repeater and warning after it (`fri +1w
//! -3d`), taking as many words as form one date; [`parse_date_input`] is the
//! same for a whole answer, and [`parse_date_change`] also takes `none`.
//! [`date_preview`] shows the Org text an answer writes, and
//! [`format_active_timestamp`] renders a [`PlanningTimestamp`].
//!
//! Every surface reads dates here: `agd agenda --from`, a capture line's
//! `due:` and `on:`, the TUI's prompts and MCP. Dates count from the `today`
//! the caller passes, so results are deterministic; only [`now_local_naive`]
//! reads the clock. A date it cannot read is an `invalid_date` error that
//! lists every accepted form.

use std::sync::LazyLock;

use chrono::{
    Datelike, Days, Duration, Local, Months, NaiveDate, NaiveDateTime, NaiveTime, Weekday,
};
use regex::Regex;

use crate::{AgdError, Change, ErrorCode, PlanningTimestamp};

/// `2026-10-30`, or `2026-10-30t14:00` (lowercased).
static ISO_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d{4}-\d{2}-\d{2})(?:t(\d{1,2}:\d{2}))?$").expect("ISO date regex")
});
/// `+3`, `-1w`, `+2tue`, `-wed`: a sign, a count, and a unit or weekday.
static RELATIVE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([+-])(\d*)([a-z]*)$").expect("relative date regex"));
/// `14:00`, `9:30`, `3pm`, `9am`, `12:30am`.
static CLOCK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d{1,2})(?::(\d{2}))?(am|pm)?$").expect("clock time regex"));
/// A repeater after a date: `+1w`, `++1m`, `.+2d`.
static REPEATER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:\+|\+\+|\.\+)\d+[hdwmy]$").expect("repeater regex"));
/// A warning after a date: `-3d`, `-1w`.
static WARNING_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^-\d+[hdwmy]$").expect("warning regex"));

/// The date forms agd accepts wherever it asks for a date, listed in every
/// `invalid_date` error so the fix is in the message.
const DATE_FORMS: &str = "use 2026-10-30 (or 2026-10-30 14:00, 2026-10-30T14:00), oct 30 or \
30 oct, 14 (the next 14th), today, tomorrow, '.' (today), a weekday (fri), +2tue or -wed, \
+3d -1w +2m +1y (+3 alone means days), a time for today (14:00, 3pm, 9am), \
or a day then a time (fri 9am, mon 10:00), then a repeater and a warning (fri +1w -3d)";

/// Reads a whole date answer, relative to `today`: one date as
/// [`read_date`] reads it, with nothing left over.
pub fn parse_date_input(input: &str, today: NaiveDate) -> Result<PlanningTimestamp, AgdError> {
    let words: Vec<&str> = input.split_whitespace().collect();
    match read_date(&words, today)? {
        Some((timestamp, taken)) if taken == words.len() => Ok(timestamp),
        _ => Err(unsupported_date(input)),
    }
}

/// The `invalid_date` error for an answer that is no date at all.
pub(crate) fn unsupported_date(input: &str) -> AgdError {
    date_error(format!("unsupported date '{}'", input.trim()))
}

/// A date answer for a field that may be cleared: `none` clears it, as in
/// MCP `plan` and the TUI's date prompts; anything else is a date
/// ([`parse_date_input`]).
pub fn parse_date_change(
    input: &str,
    today: NaiveDate,
) -> Result<Change<PlanningTimestamp>, AgdError> {
    if input.trim().eq_ignore_ascii_case("none") {
        return Ok(Change::Clear);
    }
    parse_date_input(input, today).map(Change::Set)
}

/// The date at the start of `words` and how many words it takes, as Org's
/// date prompt reads it: a day (an ISO date, `today`, `tomorrow`, `.`, a
/// weekday, `+2d`, `-wed`, `14`, `oct 30`) or a time alone (today at that
/// time), then a time, a repeater (`+1w`) and a warning (`-3d`), each
/// optional and taken when the next word is one. None when the first word
/// starts no date. A word after the day that reads as a time but is not one
/// (`25:00`, `13pm`) is an error, not the end of the date.
pub fn read_date(
    words: &[&str],
    today: NaiveDate,
) -> Result<Option<(PlanningTimestamp, usize)>, AgdError> {
    let words: Vec<String> = words.iter().map(|word| word.to_ascii_lowercase()).collect();
    let Some(first) = words.first() else {
        return Ok(None);
    };
    let (date, mut time, mut taken) = if let Some(captures) = ISO_RE.captures(first) {
        let date = NaiveDate::parse_from_str(&captures[1], "%Y-%m-%d")
            .map_err(|_| date_error(format!("invalid date '{}'", &captures[1])))?;
        let time = captures
            .get(2)
            .map(|time| clock_time(time.as_str()))
            .transpose()?
            .flatten();
        (date, time, 1)
    } else if let Some(time) = clock_time(first)? {
        (today, Some(time), 1)
    } else if let Some(date) = words
        .get(1)
        .map(|second| month_day(first, second, today))
        .transpose()?
        .flatten()
    {
        (date, None, 2)
    } else if let Some(date) = day_word(first, today)? {
        (date, None, 1)
    } else {
        return Ok(None);
    };
    if time.is_none()
        && let Some(word) = words.get(taken)
        && let Some(clock) = clock_time(word)?
    {
        time = Some(clock);
        taken += 1;
    }
    let mut timestamp = PlanningTimestamp {
        time: time.map(|time| time.format("%H:%M").to_string()),
        ..simple_date(date)
    };
    if let Some(word) = words.get(taken).filter(|word| REPEATER_RE.is_match(word)) {
        timestamp.repeater = Some(word.clone());
        taken += 1;
    }
    if let Some(word) = words.get(taken).filter(|word| WARNING_RE.is_match(word)) {
        timestamp.warning = Some(word.clone());
        taken += 1;
    }
    Ok(Some((timestamp, taken)))
}

/// How a date-prompt answer reads as Org text, e.g. `<2026-10-19 Mon 09:00>`
/// for `mon 9am`, so a prompt can show what it will write.
pub fn date_preview(input: &str, today: NaiveDate) -> Result<String, AgdError> {
    format_active_timestamp(&parse_date_input(input, today)?)
}

/// An `invalid_date` error: what is wrong, then every accepted form.
fn date_error(detail: String) -> AgdError {
    AgdError::new(ErrorCode::InvalidDate, format!("{detail}; {DATE_FORMS}"))
}

/// A one-word day as Org's date prompt reads it, lowercase: `.` or
/// `today`, `tomorrow`, a weekday (the next one, a week ahead when today is
/// one), `+N`/`-N` (see `relative_day`), or a day of the month (`14`: this
/// month's, or next month's once it has passed). None for anything else.
fn day_word(day: &str, today: NaiveDate) -> Result<Option<NaiveDate>, AgdError> {
    let invalid = || date_error(format!("invalid date '{day}'"));
    match day {
        "." | "today" => return Ok(Some(today)),
        "tomorrow" => return Ok(Some(today + Duration::days(1))),
        _ => {}
    }
    if let Some(weekday) = parse_weekday(day) {
        let distance = (weekday.num_days_from_monday() as i64
            - today.weekday().num_days_from_monday() as i64)
            .rem_euclid(7);
        let distance = if distance == 0 { 7 } else { distance };
        return Ok(Some(today + Duration::days(distance)));
    }
    if RELATIVE_RE.is_match(day) {
        return relative_day(day, today);
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
    Ok(None)
}

/// A month and day in two words, `oct 30` or `30 oct`: this year's, or
/// next year's once it has passed. None when the words are not one.
fn month_day(first: &str, second: &str, today: NaiveDate) -> Result<Option<NaiveDate>, AgdError> {
    let (name, number) = match (parse_month(first), parse_month(second)) {
        (Some(_), _) => (first, second),
        (None, Some(_)) => (second, first),
        (None, None) => return Ok(None),
    };
    let (Some(month), Ok(number)) = (parse_month(name), number.parse::<u32>()) else {
        return Ok(None);
    };
    let invalid = || date_error(format!("invalid date '{first} {second}'"));
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
fn relative_day(input: &str, today: NaiveDate) -> Result<Option<NaiveDate>, AgdError> {
    let Some(captures) = RELATIVE_RE.captures(input) else {
        return Ok(None);
    };
    let out_of_range = || date_error(format!("date '{input}' is out of range"));
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
/// for any other word, including a bare number, which is a day of the
/// month; an error for one shaped like a time that is not one (`25:00`).
fn clock_time(word: &str) -> Result<Option<NaiveTime>, AgdError> {
    let Some(captures) = CLOCK_RE.captures(word) else {
        return Ok(None);
    };
    let (minute, meridiem) = (captures.get(2), captures.get(3));
    if minute.is_none() && meridiem.is_none() {
        return Ok(None);
    }
    let invalid = || date_error(format!("invalid time '{word}'"));
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
        assert_eq!(error.code().as_str(), "invalid_date");
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

    #[test]
    fn a_date_takes_the_longest_run_of_words_and_refuses_leftovers() {
        let friday = NaiveDate::from_ymd_opt(2026, 10, 16).unwrap();
        let warned = parse_date_input("fri -3d", friday).unwrap();
        assert_eq!(
            (warned.date.as_str(), warned.warning.as_deref()),
            ("2026-10-23", Some("-3d"))
        );
        let full = parse_date_input("2026-10-30 14:00 +1w -2d", friday).unwrap();
        assert_eq!(
            format_active_timestamp(&full).unwrap(),
            "<2026-10-30 Fri 14:00 +1w -2d>"
        );
        let (date, taken) = read_date(&["fri", "3", "talks"], friday).unwrap().unwrap();
        assert_eq!((date.date.as_str(), taken), ("2026-10-23", 1));
        assert_eq!(read_date(&["Review"], friday).unwrap(), None);
        for input in ["fri 25:00", "fri talks", "13pm"] {
            let error = parse_date_input(input, friday).unwrap_err();
            assert_eq!(error.code(), ErrorCode::InvalidDate, "{input}");
            assert!(error.to_string().contains("fri 9am"), "{error}");
        }
        assert_eq!(parse_date_change(" None ", friday).unwrap(), Change::Clear);
        assert!(matches!(
            parse_date_change("fri", friday).unwrap(),
            Change::Set(timestamp) if timestamp.date == "2026-10-23"
        ));
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
