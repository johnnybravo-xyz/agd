use std::collections::BTreeMap;
use std::sync::LazyLock;

use chrono::{Datelike, Duration, Local, Months, NaiveDate, NaiveDateTime, NaiveTime, Weekday};
use regex::Regex;

use crate::{AgdError, CaptureTemplate, PlanningTimestamp};

static ABSOLUTE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d{4}-\d{2}-\d{2})(?:[ T](\d{1,2}:\d{2}))?(?:\s+([+.]*\+\d+[hdwmy]))?(?:\s+(-\d+[hdwmy]))?$")
        .expect("absolute date regex")
});
static RELATIVE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\+(\d+)([dwmy])$").expect("relative date regex"));
static ESCAPE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"%(\?|i|U|u|T|t|%|\^g|\^\{([^}]+)\})").expect("capture escape regex")
});

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureDraft {
    pub title: String,
    pub scheduled: Option<PlanningTimestamp>,
    pub deadline: Option<PlanningTimestamp>,
    pub tags: Vec<String>,
    pub priority: Option<String>,
}

pub fn parse_date_input(input: &str, today: NaiveDate) -> Result<PlanningTimestamp, AgdError> {
    let input = input.trim();
    if let Some(captures) = ABSOLUTE_RE.captures(input) {
        let date = NaiveDate::parse_from_str(captures.get(1).expect("date").as_str(), "%Y-%m-%d")
            .map_err(|_| AgdError::new("invalid_date", format!("invalid date: {input}")))?;
        let time = captures
            .get(2)
            .map(|matched| parse_time(matched.as_str()))
            .transpose()?;
        return Ok(PlanningTimestamp {
            date: date.format("%Y-%m-%d").to_string(),
            time: time.map(|value| value.format("%H:%M").to_string()),
            end_time: None,
            repeater: captures.get(3).map(|matched| matched.as_str().to_string()),
            warning: captures.get(4).map(|matched| matched.as_str().to_string()),
            raw: None,
        });
    }
    if let Some(captures) = RELATIVE_RE.captures(input) {
        let amount = captures
            .get(1)
            .expect("amount")
            .as_str()
            .parse::<u64>()
            .map_err(|_| AgdError::new("invalid_date", format!("invalid date: {input}")))?;
        let unit = captures.get(2).expect("unit").as_str();
        let date = match unit {
            "d" => today.checked_add_signed(Duration::days(amount as i64)),
            "w" => today.checked_add_signed(Duration::weeks(amount as i64)),
            "m" => today.checked_add_months(Months::new(amount as u32)),
            "y" => today.checked_add_months(Months::new((amount * 12) as u32)),
            _ => None,
        }
        .ok_or_else(|| AgdError::new("invalid_date", format!("date is out of range: {input}")))?;
        return Ok(simple_date(date));
    }
    if input.eq_ignore_ascii_case("today") {
        return Ok(simple_date(today));
    }
    if input.eq_ignore_ascii_case("tomorrow") {
        return Ok(simple_date(today + Duration::days(1)));
    }
    if let Some(weekday) = parse_weekday(input) {
        let current = today.weekday().num_days_from_monday() as i64;
        let requested = weekday.num_days_from_monday() as i64;
        let distance = (requested - current).rem_euclid(7);
        return Ok(simple_date(today + Duration::days(distance)));
    }
    Err(AgdError::new(
        "invalid_date",
        format!("unsupported Org date '{input}'; use YYYY-MM-DD, +Nd, +Nw, +Nm, +Ny, or weekday"),
    ))
}

pub fn parse_capture_line(input: &str, today: NaiveDate) -> Result<CaptureDraft, AgdError> {
    let mut title = Vec::new();
    let mut scheduled = None;
    let mut deadline = None;
    let mut tags = Vec::new();
    let mut priority = None;
    for token in input.split_whitespace() {
        if let Some(value) = token.strip_prefix("due:")
            && !value.is_empty()
        {
            deadline = Some(parse_date_input(value, today)?);
        } else if let Some(value) = token.strip_prefix("on:")
            && !value.is_empty()
        {
            scheduled = Some(parse_date_input(value, today)?);
        } else if let Some(tag) = token.strip_prefix('+')
            && !tag.is_empty()
            && valid_tag(tag)
        {
            tags.push(tag.to_string());
        } else if let Some(value) = token.strip_prefix('#')
            && value.len() == 1
            && value.bytes().all(|byte| byte.is_ascii_alphabetic())
        {
            priority = Some(value.to_ascii_uppercase());
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
    })
}

pub fn render_capture_template(
    template: &CaptureTemplate,
    draft: &CaptureDraft,
    body: &str,
    prompts: &BTreeMap<String, String>,
    now: NaiveDateTime,
) -> Result<String, AgdError> {
    if !template.enabled {
        return Err(AgdError::new(
            "disabled_template",
            format!(
                "capture template is disabled: {}",
                template.diagnostics.join("; ")
            ),
        ));
    }
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
    let mut planning = Vec::new();
    if let Some(scheduled) = &draft.scheduled {
        planning.push(format!(
            "SCHEDULED: {}",
            format_active_timestamp(scheduled)?
        ));
    }
    if let Some(deadline) = &draft.deadline {
        planning.push(format!("DEADLINE: {}", format_active_timestamp(deadline)?));
    }
    if !planning.is_empty() {
        lines.splice(1..1, planning);
    }
    Ok(lines.join(newline))
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
                let prompt = captures.get(2).expect("prompt name").as_str();
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
    tag.chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '_' | '@' | '#' | '%'))
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
    fn unrecognized_capture_tokens_remain_in_title() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        let draft = parse_capture_line("Renew passport due:fri +admin #A owner:me", today).unwrap();
        assert_eq!(draft.title, "Renew passport owner:me");
        assert_eq!(draft.deadline.unwrap().date, "2026-10-16");
        assert_eq!(draft.tags, vec!["admin"]);
        assert_eq!(draft.priority.as_deref(), Some("A"));
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
    fn template_preview_is_exact_org_text() {
        let template = CaptureTemplate::default();
        let draft = CaptureDraft {
            title: "Renew passport".to_string(),
            scheduled: None,
            deadline: Some(simple_date(NaiveDate::from_ymd_opt(2026, 10, 30).unwrap())),
            tags: vec!["admin".to_string()],
            priority: Some("A".to_string()),
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
}
