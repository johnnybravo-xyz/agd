use std::collections::BTreeMap;
use std::sync::LazyLock;

use chrono::{
    Datelike, Days, Duration, Local, Months, NaiveDate, NaiveDateTime, NaiveTime, Weekday,
};
use regex::Regex;

use crate::{AgdError, CaptureTemplate, PlanningTimestamp};

static ABSOLUTE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d{4}-\d{2}-\d{2})(?:[ T](\d{1,2}:\d{2}))?(?:\s+([+.]*\+\d+[hdwmy]))?(?:\s+(-\d+[hdwmy]))?$")
        .expect("absolute date regex")
});
static RELATIVE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([+-])(\d+)([dwmy])?$").expect("relative date regex"));
static CLOCK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d{1,2}:\d{2}$").expect("clock time regex"));
static ESCAPE_RE: LazyLock<Regex> = LazyLock::new(|| {
    // %^{name}t/T/u/U and a bare %^t/T/u/U prompt for a date, as Org does.
    Regex::new(r"%(\?|i|U|u|T|t|%|\^g|\^\{([^}]+)\}([tTuU])?|\^([tTuU]))")
        .expect("capture escape regex")
});

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CaptureDraft {
    pub title: String,
    pub scheduled: Option<PlanningTimestamp>,
    pub deadline: Option<PlanningTimestamp>,
    pub tags: Vec<String>,
    pub priority: Option<String>,
    /// Extra `:PROPERTIES:` entries, e.g. where an agent found the task.
    pub properties: BTreeMap<String, String>,
}

/// The date forms agd accepts wherever it asks for a date, listed in every
/// `invalid_date` error so the fix is in the message.
const DATE_FORMS: &str = "use 2026-10-30, 2026-10-30 14:00 (or 2026-10-30T14:00), today, tomorrow, \
'.' (today), a weekday (fri), +3d -1w +2m +1y (+3 alone means days), 14:00 (today at 14:00), \
or a day then a time (fri 14:00, +2d 9:30)";

/// Reads a date the way Org's date prompt does: an ISO date with optional
/// time, repeater and warning, or a day relative to `today` (see
/// `relative_day`), optionally followed by ` HH:MM`. A bare `HH:MM` is today
/// at that time.
pub fn parse_date_input(input: &str, today: NaiveDate) -> Result<PlanningTimestamp, AgdError> {
    let input = input.trim();
    if CLOCK_RE.is_match(input) {
        return timed_date(today, input);
    }
    if let Some((day, time)) = input.rsplit_once(' ')
        && CLOCK_RE.is_match(time)
        && let Some(date) = relative_day(day.trim(), today)?
    {
        return timed_date(date, time);
    }
    if let Some(date) = relative_day(input, today)? {
        return Ok(simple_date(date));
    }
    let Some(captures) = ABSOLUTE_RE.captures(input) else {
        return Err(AgdError::new(
            "invalid_date",
            format!("unsupported date '{input}'; {DATE_FORMS}"),
        ));
    };
    let date = NaiveDate::parse_from_str(captures.get(1).expect("date").as_str(), "%Y-%m-%d")
        .map_err(|_| AgdError::new("invalid_date", format!("invalid date: {input}")))?;
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

/// A day relative to `today` as Org's date prompt reads it: `.` or `today`,
/// `tomorrow`, a weekday name (the next one, today included), or `+N`/`-N`
/// with a d, w, m or y unit (days when the unit is left out). None when the
/// input is none of these.
fn relative_day(input: &str, today: NaiveDate) -> Result<Option<NaiveDate>, AgdError> {
    if input == "." || input.eq_ignore_ascii_case("today") {
        return Ok(Some(today));
    }
    if input.eq_ignore_ascii_case("tomorrow") {
        return Ok(Some(today + Duration::days(1)));
    }
    if let Some(weekday) = parse_weekday(input) {
        let current = today.weekday().num_days_from_monday() as i64;
        let requested = weekday.num_days_from_monday() as i64;
        let distance = (requested - current).rem_euclid(7);
        return Ok(Some(today + Duration::days(distance)));
    }
    let Some(captures) = RELATIVE_RE.captures(input) else {
        return Ok(None);
    };
    let out_of_range = || AgdError::new("invalid_date", format!("date is out of range: {input}"));
    let amount = captures[2].parse::<u32>().map_err(|_| out_of_range())?;
    let forward = &captures[1] == "+";
    let (days, months) = match captures.get(3).map_or("d", |unit| unit.as_str()) {
        "d" => (amount, 0),
        "w" => (amount.checked_mul(7).ok_or_else(out_of_range)?, 0),
        "m" => (0, amount),
        _ => (0, amount.checked_mul(12).ok_or_else(out_of_range)?),
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

fn timed_date(date: NaiveDate, time: &str) -> Result<PlanningTimestamp, AgdError> {
    let time = parse_time(time)?;
    Ok(PlanningTimestamp {
        time: Some(time.format("%H:%M").to_string()),
        ..simple_date(date)
    })
}

/// Reads one capture line: `due:DATE` and `on:DATE` (any `parse_date_input`
/// form; a following `HH:MM` token is that date's time), `+tag`, and `#A`
/// for the priority. Every other token, including a digits-only `+5` and a
/// lowercase `#a`, stays in the title.
pub fn parse_capture_line(input: &str, today: NaiveDate) -> Result<CaptureDraft, AgdError> {
    let mut title = Vec::new();
    let mut scheduled = None;
    let mut deadline = None;
    let mut tags = Vec::new();
    let mut priority = None;
    let mut tokens = input.split_whitespace().peekable();
    while let Some(token) = tokens.next() {
        let planning = if let Some(value) = token.strip_prefix("due:") {
            Some((value, &mut deadline))
        } else {
            token
                .strip_prefix("on:")
                .map(|value| (value, &mut scheduled))
        };
        if let Some((value, slot)) = planning
            && !value.is_empty()
        {
            let value = match tokens.next_if(|next| CLOCK_RE.is_match(next)) {
                Some(time) => format!("{value} {time}"),
                None => value.to_string(),
            };
            *slot = Some(parse_date_input(&value, today)?);
        } else if let Some(tag) = token.strip_prefix('+')
            && valid_tag(tag)
            && !tag.bytes().all(|byte| byte.is_ascii_digit())
        {
            tags.push(tag.to_string());
        } else if let Some(value) = token.strip_prefix('#')
            && priority_cookie(value)
        {
            priority = Some(value.to_string());
        } else {
            title.push(token);
        }
    }
    let title = title.join(" ");
    if title.trim().is_empty() {
        return Err(AgdError::new("invalid_capture", "capture title is empty"));
    }
    tags.sort();
    tags.dedup();
    Ok(CaptureDraft {
        title,
        scheduled,
        deadline,
        tags,
        priority,
        properties: BTreeMap::new(),
    })
}

pub fn render_capture_template(
    template: &CaptureTemplate,
    draft: &CaptureDraft,
    body: &str,
    prompts: &BTreeMap<String, String>,
    now: NaiveDateTime,
) -> Result<String, AgdError> {
    if template.template.contains("%(") {
        return Err(AgdError::new(
            "unsupported_template",
            "capture templates may not execute %(sexp) forms",
        ));
    }
    let rendered = expand_capture_escapes(&template.template, draft, body, prompts, now)?;

    let newline = if rendered.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut lines: Vec<String> = rendered.split(newline).map(str::to_string).collect();
    if let Some(first) = lines.first_mut() {
        if let Some(priority) = &draft.priority {
            insert_before_title(first, &format!("[#{}]", priority));
        }
        if !draft.tags.is_empty() {
            first.push_str(&format!(" :{}:", draft.tags.join(":")));
        }
    }
    // One planning line, as Org writes it: SCHEDULED then DEADLINE. A
    // template's own planning line (often `DEADLINE: %^{deadline}t`) is
    // merged with the due:/on: dates, which win; a keyword left without a
    // date is dropped.
    let template_planning = lines
        .get(1)
        .filter(|line| is_planning_line(line))
        .map(|line| planning_dates(line));
    if template_planning.is_some() {
        lines.remove(1);
    }
    let (mut scheduled, mut deadline) = template_planning.unwrap_or_default();
    if let Some(timestamp) = &draft.scheduled {
        scheduled = Some(format_active_timestamp(timestamp)?);
    }
    if let Some(timestamp) = &draft.deadline {
        deadline = Some(format_active_timestamp(timestamp)?);
    }
    let mut planning = Vec::new();
    if let Some(timestamp) = scheduled {
        planning.push(format!("SCHEDULED: {timestamp}"));
    }
    if let Some(timestamp) = deadline {
        planning.push(format!("DEADLINE: {timestamp}"));
    }
    if !planning.is_empty() {
        lines.insert(1.min(lines.len()), planning.join(" "));
    }
    add_properties(
        &mut lines,
        &draft.properties,
        usize::from(!planning.is_empty()) + 1,
    )?;
    Ok(lines.join(newline))
}

fn is_planning_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("SCHEDULED:") || trimmed.starts_with("DEADLINE:")
}

/// The SCHEDULED and DEADLINE timestamps on a template's planning line;
/// a keyword with no timestamp after it yields None.
fn planning_dates(line: &str) -> (Option<String>, Option<String>) {
    let date_after = |keyword: &str| {
        let rest = line.split_once(keyword)?.1.trim_start();
        let close = match rest.chars().next()? {
            '<' => '>',
            '[' => ']',
            _ => return None,
        };
        let end = rest.find(close)?;
        Some(rest[..=end].to_string())
    };
    (date_after("SCHEDULED:"), date_after("DEADLINE:"))
}

/// Adds properties to the template's own `:PROPERTIES:` drawer, or opens one
/// right after the headline and planning line (`after`), keeping keys the
/// template already sets.
fn add_properties(
    lines: &mut Vec<String>,
    properties: &BTreeMap<String, String>,
    after: usize,
) -> Result<(), AgdError> {
    if properties.is_empty() {
        return Ok(());
    }
    let mut entries = Vec::new();
    for (key, value) in properties {
        if key.is_empty() || key.contains([' ', '\t', ':']) || value.contains(['\n', '\r']) {
            return Err(AgdError::new(
                "invalid_property",
                format!("capture property '{key}' needs a plain key and a one-line value"),
            ));
        }
        let present = lines.iter().any(|line| {
            line.trim_start()
                .to_ascii_uppercase()
                .starts_with(&format!(":{}:", key.to_ascii_uppercase()))
        });
        if !present {
            entries.push(format!(":{key}: {value}"));
        }
    }
    let drawer = lines.iter().position(|line| line.trim() == ":PROPERTIES:");
    let end = drawer.and_then(|start| {
        lines[start..]
            .iter()
            .position(|line| line.trim() == ":END:")
            .map(|offset| start + offset)
    });
    match end {
        Some(end) => {
            lines.splice(end..end, entries);
        }
        None => {
            let at = after.min(lines.len());
            let mut drawer = vec![":PROPERTIES:".to_string()];
            drawer.extend(entries);
            drawer.push(":END:".to_string());
            lines.splice(at..at, drawer);
        }
    }
    Ok(())
}

/// One `%^{...}` field a template asks for. A date prompt (`%^{name}t`,
/// `%^t`, ...) takes the same input as `due:` and may be left empty.
/// `planning` names the keyword (`DEADLINE`, `SCHEDULED`) when the prompt
/// is that keyword's date on the template's planning line: `due:` or `on:`
/// then answers it, so the prompt need not be asked.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapturePrompt {
    pub name: String,
    pub date: bool,
    pub planning: Option<String>,
}

impl CapturePrompt {
    /// True when the capture line's own `due:`/`on:` already gives this
    /// prompt's date.
    pub fn answered_by(&self, draft: &CaptureDraft) -> bool {
        match self.planning.as_deref() {
            Some("DEADLINE") => draft.deadline.is_some(),
            Some("SCHEDULED") => draft.scheduled.is_some(),
            _ => false,
        }
    }
}

/// The `%^{prompt}` fields a template asks for, in order and without
/// duplicates; `%^g` asks for "tags" and a bare `%^t` for "date".
pub fn capture_prompts(template: &CaptureTemplate) -> Vec<CapturePrompt> {
    let text = &template.template;
    let mut prompts: Vec<CapturePrompt> = Vec::new();
    for captures in ESCAPE_RE.captures_iter(text) {
        let start = captures.get(0).expect("escape match").start();
        let line_start = text[..start].rfind('\n').map_or(0, |index| index + 1);
        let before = &text[line_start..start];
        // The planning keyword closest before the prompt on its line.
        let planning = ["DEADLINE:", "SCHEDULED:"]
            .iter()
            .filter_map(|keyword| before.rfind(keyword).map(|at| (at, *keyword)))
            .max()
            .map(|(_, keyword)| keyword.trim_end_matches(':').to_string());
        let prompt = match (&captures[1], captures.get(2), captures.get(4)) {
            ("^g", _, _) => CapturePrompt {
                name: "tags".to_string(),
                date: false,
                planning: None,
            },
            (_, Some(name), _) => {
                let date = captures.get(3).is_some();
                CapturePrompt {
                    name: name.as_str().to_string(),
                    date,
                    planning: if date { planning } else { None },
                }
            }
            (_, None, Some(_)) => CapturePrompt {
                name: "date".to_string(),
                date: true,
                planning,
            },
            _ => continue,
        };
        if !prompts.iter().any(|existing| existing.name == prompt.name) {
            prompts.push(prompt);
        }
    }
    prompts
}

/// A date prompt's answer as an Org timestamp: `t` active date, `T` active
/// with time, `u`/`U` the inactive forms. Empty stays empty.
fn prompted_date(value: &str, kind: &str, today: NaiveDate) -> Result<String, AgdError> {
    if value.trim().is_empty() {
        return Ok(String::new());
    }
    let mut timestamp = parse_date_input(value, today)?;
    if kind == "t" || kind == "u" {
        timestamp.time = None;
    }
    let active = format_active_timestamp(&timestamp)?;
    Ok(if kind == "u" || kind == "U" {
        format!("[{}]", &active[1..active.len() - 1])
    } else {
        active
    })
}

// Expands every escape in one pass over the template so that user-supplied
// title, body and prompt text is inserted literally and never re-expanded.
fn expand_capture_escapes(
    template: &str,
    draft: &CaptureDraft,
    body: &str,
    prompts: &BTreeMap<String, String>,
    now: NaiveDateTime,
) -> Result<String, AgdError> {
    let mut rendered = String::with_capacity(template.len() + draft.title.len() + body.len());
    let mut copied_to = 0;
    for captures in ESCAPE_RE.captures_iter(template) {
        let escape = captures.get(0).expect("escape match");
        rendered.push_str(&template[copied_to..escape.start()]);
        copied_to = escape.end();
        match &captures[1] {
            "?" => rendered.push_str(&draft.title),
            "i" => rendered.push_str(body),
            "U" => rendered.push_str(&format!("[{}]", now.format("%Y-%m-%d %a %H:%M"))),
            "u" => rendered.push_str(&format!("[{}]", now.format("%Y-%m-%d %a"))),
            "T" => rendered.push_str(&format!("<{}>", now.format("%Y-%m-%d %a %H:%M"))),
            "t" => rendered.push_str(&format!("<{}>", now.format("%Y-%m-%d %a"))),
            "%" => rendered.push('%'),
            "^g" => rendered.push_str(prompts.get("tags").ok_or_else(|| {
                AgdError::new("missing_prompt", "capture tag prompt has no 'tags' value")
            })?),
            _ => {
                let date_kind = captures
                    .get(3)
                    .or(captures.get(4))
                    .map(|kind| kind.as_str());
                let prompt = captures.get(2).map_or("date", |name| name.as_str());
                match date_kind {
                    Some(kind) => {
                        let value = prompts.get(prompt).map_or("", String::as_str);
                        let date = prompted_date(value, kind, now.date()).map_err(|error| {
                            AgdError::new("invalid_date", format!("{prompt}: {error}"))
                        })?;
                        rendered.push_str(&date);
                    }
                    None => {
                        let value = prompts.get(prompt).ok_or_else(|| {
                            AgdError::new(
                                "missing_prompt",
                                format!("capture prompt '{prompt}' has no value"),
                            )
                        })?;
                        rendered.push_str(value);
                    }
                }
            }
        }
    }
    rendered.push_str(&template[copied_to..]);
    Ok(rendered)
}

pub fn format_active_timestamp(timestamp: &PlanningTimestamp) -> Result<String, AgdError> {
    let date = NaiveDate::parse_from_str(&timestamp.date, "%Y-%m-%d")
        .map_err(|_| AgdError::new("invalid_date", format!("invalid date: {}", timestamp.date)))?;
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

fn simple_date(date: NaiveDate) -> PlanningTimestamp {
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
        .map_err(|_| AgdError::new("invalid_date", format!("invalid time: {input}")))
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

fn valid_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '@' | '#' | '%'))
}

/// An Org priority cookie's value: one uppercase letter, or a number of at
/// most two digits (Org's numeric priorities run 0-64). Whether it is in the
/// configured range is checked when the capture is written.
fn priority_cookie(value: &str) -> bool {
    let letter = value.len() == 1 && value.bytes().all(|byte| byte.is_ascii_uppercase());
    let number = (1..=2).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_digit());
    letter || number
}

fn insert_before_title(line: &mut String, value: &str) {
    let stars_end = line
        .find(|character: char| !matches!(character, '*' | ' ' | '\t'))
        .unwrap_or(line.len());
    if stars_end >= line.len() {
        line.push_str(value);
        return;
    }
    let rest = &line[stars_end..];
    let first_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let insertion = if rest[..first_end]
        .bytes()
        .all(|byte| byte.is_ascii_uppercase() || matches!(byte, b'_' | b'-'))
    {
        stars_end + first_end
    } else {
        stars_end
    };
    line.insert_str(insertion, &format!(" {value}"));
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
            "2026-10-30T14:00",
            "fri 14:00",
        ] {
            assert!(error.to_string().contains(form), "{form} missing: {error}");
        }
        assert!(parse_date_input("fri 25:00", today).is_err());
    }

    #[test]
    fn unrecognized_capture_tokens_remain_in_title() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        let draft = parse_capture_line("Renew passport due:fri +admin #A owner:me", today).unwrap();
        assert_eq!(draft.title, "Renew passport owner:me");
        assert_eq!(draft.deadline.unwrap().date, "2026-10-16");
        assert_eq!(draft.tags, vec!["admin"]);
        assert_eq!(draft.priority.as_deref(), Some("A"));
    }

    #[test]
    fn capture_dates_take_a_following_time_token() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        let draft = parse_capture_line("Dentist due:2026-10-30 14:00 on:fri 9:30", today).unwrap();
        assert_eq!(draft.title, "Dentist");
        let deadline = draft.deadline.unwrap();
        assert_eq!(
            (deadline.date.as_str(), deadline.time.as_deref()),
            ("2026-10-30", Some("14:00"))
        );
        let scheduled = draft.scheduled.unwrap();
        assert_eq!(
            (scheduled.date.as_str(), scheduled.time.as_deref()),
            ("2026-10-16", Some("09:30"))
        );
        // A time with no date token before it is title text.
        let draft = parse_capture_line("Standup 14:00 +work", today).unwrap();
        assert_eq!(draft.title, "Standup 14:00");
    }

    #[test]
    fn numbers_and_lowercase_cookies_stay_in_the_title() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        let draft = parse_capture_line("Price is +5 dollars #a note", today).unwrap();
        assert_eq!(draft.title, "Price is +5 dollars #a note");
        assert!(draft.tags.is_empty());
        assert_eq!(draft.priority, None);
        let numeric = parse_capture_line("Triage +v2 #5", today).unwrap();
        assert_eq!(numeric.title, "Triage");
        assert_eq!(numeric.tags, vec!["v2"]);
        assert_eq!(numeric.priority.as_deref(), Some("5"));
        let issue = parse_capture_line("Fix issue #123", today).unwrap();
        assert_eq!(
            (issue.title.as_str(), issue.priority),
            ("Fix issue #123", None)
        );
    }

    #[test]
    fn user_text_is_inserted_literally() {
        let template = CaptureTemplate {
            template: "* TODO %?\n%i\n:CREATED: %U\n100%%".to_string(),
            ..CaptureTemplate::default()
        };
        let draft = CaptureDraft {
            title: "Literal %U and %i".to_string(),
            scheduled: None,
            deadline: None,
            tags: Vec::new(),
            priority: None,
            properties: BTreeMap::new(),
        };
        let now = NaiveDate::from_ymd_opt(2026, 10, 13)
            .unwrap()
            .and_hms_opt(14, 22, 0)
            .unwrap();
        let rendered =
            render_capture_template(&template, &draft, "body %? %t", &BTreeMap::new(), now)
                .unwrap();
        assert_eq!(
            rendered,
            "* TODO Literal %U and %i\nbody %? %t\n:CREATED: [2026-10-13 Tue 14:22]\n100%"
        );
    }

    #[test]
    fn capture_properties_join_the_template_drawer_or_open_one() {
        let now = NaiveDate::from_ymd_opt(2026, 10, 13)
            .unwrap()
            .and_hms_opt(14, 22, 0)
            .unwrap();
        let draft = CaptureDraft {
            title: "Fix flaky test".to_string(),
            properties: BTreeMap::from([
                ("REPO".to_string(), "agd".to_string()),
                (
                    "SOURCE".to_string(),
                    "[[file:~/src/agd/write.rs::42]]".to_string(),
                ),
            ]),
            ..CaptureDraft::default()
        };
        let with_drawer = render_capture_template(
            &CaptureTemplate::default(),
            &draft,
            "",
            &BTreeMap::new(),
            now,
        )
        .unwrap();
        assert_eq!(
            with_drawer,
            "* TODO Fix flaky test\n:PROPERTIES:\n:CREATED: [2026-10-13 Tue 14:22]\n:REPO: agd\n:SOURCE: [[file:~/src/agd/write.rs::42]]\n:END:"
        );
        let plain = CaptureTemplate {
            template: "* TODO %?\nNotes".to_string(),
            ..CaptureTemplate::default()
        };
        let opened = render_capture_template(&plain, &draft, "", &BTreeMap::new(), now).unwrap();
        assert!(opened.starts_with("* TODO Fix flaky test\n:PROPERTIES:\n:REPO: agd\n"));
        assert!(opened.ends_with(":END:\nNotes"));
    }

    #[test]
    fn template_preview_is_exact_org_text() {
        let template = CaptureTemplate::default();
        let draft = CaptureDraft {
            title: "Renew passport".to_string(),
            scheduled: None,
            deadline: Some(simple_date(NaiveDate::from_ymd_opt(2026, 10, 30).unwrap())),
            tags: vec!["admin".to_string()],
            priority: Some("A".to_string()),
            properties: BTreeMap::new(),
        };
        let rendered = render_capture_template(
            &template,
            &draft,
            "",
            &BTreeMap::new(),
            NaiveDate::from_ymd_opt(2026, 10, 13)
                .unwrap()
                .and_hms_opt(9, 30, 0)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            rendered,
            "* TODO [#A] Renew passport :admin:\nDEADLINE: <2026-10-30 Fri>\n:PROPERTIES:\n:CREATED: [2026-10-13 Tue 09:30]\n:END:"
        );
    }

    fn deadline_template() -> CaptureTemplate {
        CaptureTemplate {
            template: "* TODO %?\nDEADLINE: %^{deadline}t\nCREATED: %U".to_string(),
            ..CaptureTemplate::default()
        }
    }

    fn render(
        template: &CaptureTemplate,
        draft: &CaptureDraft,
        deadline: &str,
    ) -> Result<String, AgdError> {
        let now = NaiveDate::from_ymd_opt(2026, 10, 13)
            .unwrap()
            .and_hms_opt(9, 30, 0)
            .unwrap();
        let prompts = BTreeMap::from([("deadline".to_string(), deadline.to_string())]);
        render_capture_template(template, draft, "", &prompts, now)
    }

    #[test]
    fn a_date_prompt_is_a_date_and_lists_as_one() {
        assert_eq!(
            capture_prompts(&deadline_template()),
            vec![CapturePrompt {
                name: "deadline".to_string(),
                date: true,
                planning: Some("DEADLINE".to_string()),
            }]
        );
        let draft = CaptureDraft {
            title: "Renew passport".to_string(),
            ..CaptureDraft::default()
        };
        assert_eq!(
            render(&deadline_template(), &draft, "fri").unwrap(),
            "* TODO Renew passport\nDEADLINE: <2026-10-16 Fri>\nCREATED: [2026-10-13 Tue 09:30]"
        );
        let error = render(&deadline_template(), &draft, "someday").unwrap_err();
        assert!(error.to_string().contains("deadline"), "{error}");
    }

    #[test]
    fn token_dates_merge_into_the_template_planning_line() {
        let draft = CaptureDraft {
            title: "Renew passport".to_string(),
            scheduled: Some(simple_date(NaiveDate::from_ymd_opt(2026, 10, 19).unwrap())),
            deadline: Some(simple_date(NaiveDate::from_ymd_opt(2026, 10, 28).unwrap())),
            ..CaptureDraft::default()
        };
        let expected = "* TODO Renew passport\nSCHEDULED: <2026-10-19 Mon> DEADLINE: <2026-10-28 Wed>\nCREATED: [2026-10-13 Tue 09:30]";
        // The prompt left empty: the due: token fills the deadline.
        assert_eq!(render(&deadline_template(), &draft, "").unwrap(), expected);
        // Both given: the token wins, and there is still one planning line.
        assert_eq!(
            render(&deadline_template(), &draft, "fri").unwrap(),
            expected
        );
        // Neither: the empty DEADLINE keyword is dropped.
        let plain = CaptureDraft {
            title: "Renew passport".to_string(),
            ..CaptureDraft::default()
        };
        assert_eq!(
            render(&deadline_template(), &plain, "").unwrap(),
            "* TODO Renew passport\nCREATED: [2026-10-13 Tue 09:30]"
        );
    }

    #[test]
    fn timed_and_inactive_date_prompts_render_as_org_does() {
        let template = CaptureTemplate {
            template: "* Meeting %?\n%^{when}T\nLogged %^{deadline}u".to_string(),
            ..CaptureTemplate::default()
        };
        let draft = CaptureDraft {
            title: "Sync".to_string(),
            ..CaptureDraft::default()
        };
        let now = NaiveDate::from_ymd_opt(2026, 10, 13)
            .unwrap()
            .and_hms_opt(9, 30, 0)
            .unwrap();
        let prompts = BTreeMap::from([
            ("when".to_string(), "2026-10-30 14:00".to_string()),
            ("deadline".to_string(), "2026-10-30 14:00".to_string()),
        ]);
        assert_eq!(
            render_capture_template(&template, &draft, "", &prompts, now).unwrap(),
            "* Meeting Sync\n<2026-10-30 Fri 14:00>\nLogged [2026-10-30 Fri]"
        );
    }
}
