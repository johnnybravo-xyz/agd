use std::collections::BTreeMap;
use std::sync::LazyLock;

use chrono::{NaiveDate, NaiveDateTime};
use regex::Regex;

use crate::{
    AgdError, CaptureDraft, CaptureTemplate, ErrorCode, format_active_timestamp, parse_date_input,
};

static ESCAPE_RE: LazyLock<Regex> = LazyLock::new(|| {
    // %^{name}t/T/u/U and a bare %^t/T/u/U prompt for a date, as Org does.
    Regex::new(r"%(\?|i|U|u|T|t|%|\^g|\^\{([^}]+)\}([tTuU])?|\^([tTuU]))")
        .expect("capture escape regex")
});

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

pub fn render_capture_template(
    template: &CaptureTemplate,
    draft: &CaptureDraft,
    body: &str,
    prompts: &BTreeMap<String, String>,
    now: NaiveDateTime,
) -> Result<String, AgdError> {
    if template.template.contains("%(") {
        return Err(AgdError::new(
            ErrorCode::UnsupportedTemplate,
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
                AgdError::new(
                    ErrorCode::MissingPrompt,
                    "capture tag prompt has no 'tags' value",
                )
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
                            AgdError::new(ErrorCode::InvalidDate, format!("{prompt}: {error}"))
                        })?;
                        rendered.push_str(&date);
                    }
                    None => {
                        let value = prompts.get(prompt).ok_or_else(|| {
                            AgdError::new(
                                ErrorCode::MissingPrompt,
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
                ErrorCode::InvalidProperty,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::date::simple_date;

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
