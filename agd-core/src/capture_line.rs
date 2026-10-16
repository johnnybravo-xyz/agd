use std::collections::BTreeMap;

use chrono::NaiveDate;

use crate::{AgdError, PlanningTimestamp, parse_date_input};

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

/// Reads one capture line: `due:DATE` and `on:DATE` (any `parse_date_input`
/// form, which may continue into the next words: `due:fri 9am`, `on:oct
/// 30`), `+tag`, and `#A` for the priority. Every other token, including a
/// digits-only `+5` and a lowercase `#a`, stays in the title.
pub fn parse_capture_line(input: &str, today: NaiveDate) -> Result<CaptureDraft, AgdError> {
    let mut title = Vec::new();
    let mut scheduled = None;
    let mut deadline = None;
    let mut tags = Vec::new();
    let mut priority = None;
    let tokens: Vec<&str> = input.split_whitespace().collect();
    let mut index = 0;
    while let Some(&token) = tokens.get(index) {
        index += 1;
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
            let (timestamp, taken) = capture_date(value, &tokens[index..], today)?;
            *slot = Some(timestamp);
            index += taken;
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

/// A `due:`/`on:` token's date, with up to two following words when together
/// they read as one date (`due:fri 9am`, `on:oct 30 14:00`); also returns
/// how many following words it took.
fn capture_date(
    value: &str,
    following: &[&str],
    today: NaiveDate,
) -> Result<(PlanningTimestamp, usize), AgdError> {
    for taken in (1..=following.len().min(2)).rev() {
        let words = format!("{value} {}", following[..taken].join(" "));
        if let Ok(timestamp) = parse_date_input(&words, today) {
            return Ok((timestamp, taken));
        }
    }
    Ok((parse_date_input(value, today)?, 0))
}

fn valid_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '@' | '#' | '%'))
}

/// A capture line's priority cookie: one uppercase letter, as in `#A`. A
/// number stays text, so `Fix #42` keeps its issue number; numeric
/// priorities are set from the edit popup instead. Whether the letter is in
/// the configured range is checked when the capture is written.
fn priority_cookie(value: &str) -> bool {
    value.len() == 1 && value.bytes().all(|byte| byte.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // A date may take two more words; words that do not continue it are
        // the title's.
        let draft = parse_capture_line("Review due:oct 30 9am draft", today).unwrap();
        let deadline = draft.deadline.unwrap();
        assert_eq!(
            (deadline.date.as_str(), deadline.time.as_deref()),
            ("2026-10-30", Some("09:00"))
        );
        assert_eq!(draft.title, "Review draft");
        let draft = parse_capture_line("Plan due:fri 3 talks", today).unwrap();
        assert_eq!(draft.deadline.unwrap().date, "2026-10-16");
        assert_eq!(draft.title, "Plan 3 talks");
    }

    #[test]
    fn numbers_and_lowercase_cookies_stay_in_the_title() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        let draft = parse_capture_line("Price is +5 dollars #a note", today).unwrap();
        assert_eq!(draft.title, "Price is +5 dollars #a note");
        assert!(draft.tags.is_empty());
        assert_eq!(draft.priority, None);
        let numeric = parse_capture_line("Fix #42 +v2", today).unwrap();
        assert_eq!(numeric.title, "Fix #42");
        assert_eq!(numeric.tags, vec!["v2"]);
        assert_eq!(numeric.priority, None);
        let issue = parse_capture_line("Fix issue #123", today).unwrap();
        assert_eq!(
            (issue.title.as_str(), issue.priority),
            ("Fix issue #123", None)
        );
    }
}
