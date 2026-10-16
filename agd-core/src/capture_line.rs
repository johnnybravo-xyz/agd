//! The one-line capture syntax: `Renew passport due:fri #admin !A` read into a
//! [`CaptureDraft`], with `due:` and `on:` dates as [`read_date`] reads them,
//! and tags and a priority in the configured [`Notation`]: `#admin` and `!A`,
//! or Org's `:admin:` and `#A`.
//!
//! A token it does not recognize stays in the title (`#42`, `!!`, `+admin`),
//! so a line never loses text. Whether a priority is in the target file's
//! range is checked when the capture is written, not here.

use std::collections::BTreeMap;

use chrono::NaiveDate;

use crate::date::unsupported_date;
use crate::document::is_org_tag;
use crate::{AgdError, ErrorCode, Notation, PlanningTimestamp, read_date};

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
/// 9am`, `on:oct 30`, `due:2026-10-30 14:00 +1w -2d`), tags and a priority
/// in `notation` (`tag_token`, `priority_token`). Every other token stays
/// in the title.
pub fn parse_capture_line(
    input: &str,
    today: NaiveDate,
    notation: Notation,
) -> Result<CaptureDraft, AgdError> {
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
        if let Some(names) = tag_token(token, notation) {
            tags.extend(names.into_iter().map(str::to_string));
        } else if let Some(cookie) = priority_token(token, notation) {
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

/// The tags a capture token names: `#admin` in agd notation, `:admin:` or
/// `:a:b:` in org notation. Digits after `#` stay text, so `Fix #42` keeps
/// its issue number, as do characters outside Org's tag characters.
fn tag_token(token: &str, notation: Notation) -> Option<Vec<&str>> {
    let tags: Vec<&str> = match notation {
        Notation::Agd => {
            let tag = token.strip_prefix('#')?;
            if tag.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            vec![tag]
        }
        Notation::Org => token
            .strip_prefix(':')?
            .strip_suffix(':')?
            .split(':')
            .collect(),
    };
    tags.iter().all(|tag| is_org_tag(tag)).then_some(tags)
}

/// The priority a capture token names: one uppercase letter after `!` in
/// agd notation (`!A`), after `#` in org notation (`#A`). Anything else
/// stays text (`!!`, `!a`, `#42`). Whether the letter is in the file's
/// range is checked when the capture is written.
fn priority_token(token: &str, notation: Notation) -> Option<&str> {
    let marker = match notation {
        Notation::Agd => '!',
        Notation::Org => '#',
    };
    let cookie = token.strip_prefix(marker)?;
    (cookie.len() == 1 && cookie.bytes().all(|byte| byte.is_ascii_uppercase())).then_some(cookie)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 13).unwrap()
    }

    /// A line read in agd's notation, the default.
    fn read(line: &str) -> CaptureDraft {
        parse_capture_line(line, today(), Notation::Agd).unwrap()
    }

    /// (title, tags, priority) of a line read in `notation`.
    fn tokens(line: &str, notation: Notation) -> (String, Vec<String>, Option<String>) {
        let draft = parse_capture_line(line, today(), notation).unwrap();
        (draft.title, draft.tags, draft.priority)
    }

    fn expected(
        title: &str,
        tags: &[&str],
        priority: Option<&str>,
    ) -> (String, Vec<String>, Option<String>) {
        (
            title.to_string(),
            tags.iter().map(|tag| tag.to_string()).collect(),
            priority.map(str::to_string),
        )
    }

    #[test]
    fn unrecognized_capture_tokens_remain_in_title() {
        let draft = read("Renew passport due:fri #admin !A owner:me");
        assert_eq!(draft.title, "Renew passport owner:me");
        assert_eq!(draft.deadline.unwrap().date, "2026-10-16");
        assert_eq!(draft.tags, vec!["admin"]);
        assert_eq!(draft.priority.as_deref(), Some("A"));
    }

    #[test]
    fn agd_notation_takes_hash_tags_and_bang_priorities() {
        let agd = |line| tokens(line, Notation::Agd);
        assert_eq!(
            agd("Call the bank #home #admin !B"),
            expected("Call the bank", &["admin", "home"], Some("B"))
        );
        // Digits after # are an issue number; ! needs one capital letter.
        assert_eq!(agd("Fix #42 now"), expected("Fix #42 now", &[], None));
        assert_eq!(
            agd("Ship it !! !a !AB"),
            expected("Ship it !! !a !AB", &[], None)
        );
        // Org's own markers are text here.
        assert_eq!(
            agd("Plan :work: +home #C"),
            expected("Plan :work: +home", &["C"], None)
        );
        assert_eq!(agd("Price #5-off"), expected("Price #5-off", &[], None));
    }

    #[test]
    fn org_notation_takes_colon_tags_and_hash_priorities() {
        let org = |line| tokens(line, Notation::Org);
        assert_eq!(
            org("Call the bank :home: :a:b: #B"),
            expected("Call the bank", &["a", "b", "home"], Some("B"))
        );
        assert_eq!(org("Fix #42 now"), expected("Fix #42 now", &[], None));
        // agd's markers, an empty group and a time are text here.
        assert_eq!(
            org("Plan #work !A :: 10:30"),
            expected("Plan #work !A :: 10:30", &[], None)
        );
    }

    #[test]
    fn capture_dates_take_a_following_time_token() {
        let draft = read("Dentist due:2026-10-30 14:00 on:fri 9:30");
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
        assert_eq!(read("Standup 14:00 #work").title, "Standup 14:00");
        // A date may take two more words; words that do not continue it are
        // the title's.
        let draft = read("Review due:oct 30 9am draft");
        let deadline = draft.deadline.unwrap();
        assert_eq!(
            (deadline.date.as_str(), deadline.time.as_deref()),
            ("2026-10-30", Some("09:00"))
        );
        assert_eq!(draft.title, "Review draft");
        let draft = read("Plan due:fri 3 talks");
        assert_eq!(draft.deadline.unwrap().date, "2026-10-16");
        assert_eq!(draft.title, "Plan 3 talks");
    }

    #[test]
    fn a_capture_date_takes_its_repeater_and_warning_and_refuses_a_bad_time() {
        let draft = read("Pay rent due:2026-10-30 14:00 +1m -2d #home");
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
        let error = parse_capture_line("Call due:fri 25:00", today(), Notation::Agd).unwrap_err();
        assert_eq!(error.code(), ErrorCode::InvalidDate);
        let error = parse_capture_line("Call due:someday", today(), Notation::Agd).unwrap_err();
        assert!(
            error.to_string().contains("unsupported date 'someday'"),
            "{error}"
        );
    }
}
