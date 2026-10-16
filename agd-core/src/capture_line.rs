//! The one-line capture syntax: `Renew passport due:fri +admin #A` read into a
//! [`CaptureDraft`], with `due:` and `on:` dates as [`read_date`] reads them,
//! `+tags`, and an uppercase `#A` priority.
//!
//! A token it does not recognize stays in the title (`+5`, `#42`, `#a`), so a
//! line never loses text. Whether a priority is in the target file's range is
//! checked when the capture is written, not here.

use std::collections::BTreeMap;

use chrono::NaiveDate;

use crate::date::unsupported_date;
use crate::{AgdError, ErrorCode, PlanningTimestamp, read_date};

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

/// Reads one capture line: `due:DATE` and `on:DATE` (a date as
/// [`read_date`] reads it, which may continue into the next words: `due:fri
/// 9am`, `on:oct 30`, `due:2026-10-30 14:00 +1w -2d`), a tag (`+tag`) and a
/// priority (`#A`). Every other token, including a digits-only `+5` and a
/// lowercase `#a`, stays in the title.
pub fn parse_capture_line(input: &str, today: NaiveDate) -> Result<CaptureDraft, AgdError> {
    let mut title = Vec::new();
    let mut scheduled = None;
    let mut deadline = None;
    let mut tags = Vec::new();
    let mut priority = None;
    let tokens: Vec<&str> = input.split_whitespace().collect();
    let mut index = 0;
    while let Some(&token) = tokens.get(index) {
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
            let mut words = vec![value];
            words.extend(&tokens[index + 1..]);
            let (timestamp, taken) =
                read_date(&words, today)?.ok_or_else(|| unsupported_date(value))?;
            *slot = Some(timestamp);
            index += taken;
            continue;
        }
        if let Some(tag) = tag_token(token) {
            tags.push(tag.to_string());
        } else if let Some(cookie) = priority_token(token) {
            priority = Some(cookie.to_string());
        } else {
            title.push(token);
        }
        index += 1;
    }
    let title = title.join(" ");
    if title.trim().is_empty() {
        return Err(AgdError::new(
            ErrorCode::InvalidCapture,
            "capture title is empty",
        ));
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

/// The tag a capture token names: `+admin` is `admin`. A digits-only `+5`
/// and characters outside Org's tag characters stay title text.
fn tag_token(token: &str) -> Option<&str> {
    let tag = token.strip_prefix('+')?;
    let valid = !tag.is_empty()
        && tag
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '@' | '#' | '%'))
        && !tag.bytes().all(|byte| byte.is_ascii_digit());
    valid.then_some(tag)
}

/// The priority a capture token names: one uppercase letter, as in `#A`.
/// A number stays text, so `Fix #42` keeps its issue number; numeric
/// priorities are set from the edit popup instead. Whether the letter is in
/// the configured range is checked when the capture is written.
fn priority_token(token: &str) -> Option<&str> {
    let cookie = token.strip_prefix('#')?;
    (cookie.len() == 1 && cookie.bytes().all(|byte| byte.is_ascii_uppercase())).then_some(cookie)
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
    fn a_capture_date_takes_its_repeater_and_warning_and_refuses_a_bad_time() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        let draft =
            parse_capture_line("Pay rent due:2026-10-30 14:00 +1m -2d +home", today).unwrap();
        assert_eq!(draft.title, "Pay rent");
        assert_eq!(draft.tags, ["home"]);
        let deadline = draft.deadline.unwrap();
        assert_eq!(
            (
                deadline.time.as_deref(),
                deadline.repeater.as_deref(),
                deadline.warning.as_deref()
            ),
            (Some("14:00"), Some("+1m"), Some("-2d"))
        );
        let error = parse_capture_line("Call due:fri 25:00", today).unwrap_err();
        assert_eq!(error.code(), ErrorCode::InvalidDate);
        let error = parse_capture_line("Call due:someday", today).unwrap_err();
        assert!(
            error.to_string().contains("unsupported date 'someday'"),
            "{error}"
        );
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
