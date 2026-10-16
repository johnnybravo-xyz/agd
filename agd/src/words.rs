//! How agd says things to people, in the TUI, the pane and the CLI's
//! terminal sentences, so the same fact always reads the same:
//!
//! - a date is `Fri 16 Oct`, with the year when it is not this year's, and
//!   a time after it (`Fri 16 Oct 14:00`);
//! - a countdown is `in 3d`, an age `12d`, a missed deadline `-2d`;
//! - a change is `<file:line or N items>  <change> · <title>`, which the
//!   TUI ends with ` · u undo` while it can be undone;
//! - an error is a sentence without its code (codes are for JSON).
//!
//! Org's own text (the org-agenda layout, `--csv`, JSON) is never reworded.

use std::path::{Path, PathBuf};

use agd_core::{AgdError, Item};
use chrono::{Datelike, NaiveDate};

/// `Fri 16 Oct`, or `Fri 16 Oct 2027` outside `today`'s year.
pub fn day(date: NaiveDate, today: NaiveDate) -> String {
    if date.year() == today.year() {
        date.format("%a %-d %b").to_string()
    } else {
        date.format("%a %-d %b %Y").to_string()
    }
}

/// An Org date (`2026-10-16`) and optional time as `Fri 16 Oct 14:00`; a
/// date that does not parse is shown as written.
pub fn when(date: &str, time: Option<&str>, today: NaiveDate) -> String {
    let Ok(parsed) = date.parse::<NaiveDate>() else {
        return date.to_string();
    };
    match time {
        Some(time) => format!("{} {time}", day(parsed, today)),
        None => day(parsed, today),
    }
}

/// `file:line` relative to the first root that holds the file.
pub fn location(file: &str, line: usize, roots: &[PathBuf]) -> String {
    let path = Path::new(file);
    let relative = roots
        .iter()
        .find_map(|root| path.strip_prefix(root).ok())
        .unwrap_or(path);
    format!("{}:{line}", relative.display())
}

/// `<location>  <change> · <title>`, the one shape of every change line.
pub fn change(location: &str, change: &str, title: &str) -> String {
    format!("{location}  {change} · {title}")
}

/// What a capture wrote: `inbox.org:7  captured · Renew passport ·
/// deadline Fri 23 Oct`, its dates after the title.
pub fn captured(item: &Item, location: &str, today: NaiveDate) -> String {
    let mut line = change(location, "captured", &item.title);
    for (label, timestamp) in [
        ("scheduled", &item.planning.scheduled),
        ("deadline", &item.planning.deadline),
    ] {
        if let Some(timestamp) = timestamp {
            let date = when(&timestamp.date, timestamp.time.as_deref(), today);
            line.push_str(&format!(" · {label} {date}"));
        }
    }
    line
}

/// A raw headline from a diff as a title, for a task no file holds any
/// more: no stars, keyword, priority or tags.
pub fn headline_title(headline: &str) -> String {
    let mut words: Vec<&str> = headline.split_whitespace().skip(1).collect();
    if words
        .first()
        .is_some_and(|word| word.len() > 1 && word.chars().all(|c| c.is_ascii_uppercase()))
    {
        words.remove(0);
    }
    if words.first().is_some_and(|word| word.starts_with("[#")) {
        words.remove(0);
    }
    if words
        .last()
        .is_some_and(|word| word.len() > 1 && word.starts_with(':') && word.ends_with(':'))
    {
        words.pop();
    }
    words.join(" ")
}

/// A core error as a sentence. Most messages already read as one; a stale
/// ref's names the raw ref and no_change's is about the edit, so they are
/// said plainly.
pub fn error_sentence(error: &AgdError) -> String {
    match error.code() {
        "stale_ref" => "this heading changed on disk since agd read it".to_string(),
        "no_change" => "nothing would change".to_string(),
        _ => error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(text: &str) -> NaiveDate {
        text.parse().unwrap()
    }

    #[test]
    fn dates_read_the_same_everywhere() {
        let today = date("2026-10-16");
        assert_eq!(day(date("2026-10-16"), today), "Fri 16 Oct");
        assert_eq!(day(date("2027-01-04"), today), "Mon 4 Jan 2027");
        assert_eq!(when("2026-10-16", Some("14:00"), today), "Fri 16 Oct 14:00");
        assert_eq!(when("someday", None, today), "someday");
    }

    #[test]
    fn a_change_names_its_place_then_what_changed_then_the_task() {
        let roots = [PathBuf::from("/notes/org")];
        let place = location("/notes/org/work/tasks.org", 42, &roots);
        assert_eq!(place, "work/tasks.org:42");
        assert_eq!(
            change(&place, "TODO → DONE", "Pay rent"),
            "work/tasks.org:42  TODO → DONE · Pay rent"
        );
    }
}
