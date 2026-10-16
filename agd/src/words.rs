//! How agd says things to people, in the TUI, the pane and the CLI's
//! terminal sentences, so the same fact always reads the same:
//!
//! - a date is `Fri 16 Oct`, with the year when it is not this year's, and
//!   a time after it (`Fri 16 Oct 14:00`); when something happened is its
//!   time today (`17:22`), else the same date and time;
//! - a countdown is `in 3d`, an age `12d`, a missed deadline `-2d`;
//! - a change is `<file:line or N items>  <change> · <title>`, which the
//!   TUI ends with ` · u undo` while it can be undone;
//! - a count names its noun, singular for one (`1 file`, `3 files`), and
//!   the day's counts read `1 overdue · 3 today · 9 inbox` (`summary`);
//! - a tag is `#work`, or Org's `:work:` with `notation = "org"` (`tag`,
//!   `tags`), wherever one is drawn, and a capture line takes it and a
//!   priority (`priority`) the same way;
//! - an error is a sentence without its code (codes are for JSON).
//!
//! Org's own text (the org-agenda layout, `--csv`, JSON) is never reworded.

use std::path::{Path, PathBuf};

use agd_core::{AgdError, AgendaSummary, ErrorCode, Item, Notation};
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

/// When something happened, from a local RFC 3339 time as the journal
/// writes it: `17:22` today, else `when`'s `Thu 15 Oct 17:22`.
pub fn moment(timestamp: &str, today: NaiveDate) -> String {
    let (Some(date), Some(time)) = (timestamp.get(..10), timestamp.get(11..16)) else {
        return timestamp.to_string();
    };
    if date.parse::<NaiveDate>() == Ok(today) {
        time.to_string()
    } else {
        when(date, Some(time), today)
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

/// A sentence with every path under a root written relative to it, as
/// `location` writes one (`projects/alpha.org`), for errors that name a
/// file. Absolute paths are for JSON.
pub fn relative_paths(text: &str, roots: &[PathBuf]) -> String {
    roots.iter().fold(text.to_string(), |text, root| {
        let root = root.display().to_string();
        text.replace(&format!("{}/", root.trim_end_matches('/')), "")
    })
}

/// A path with the home directory written as `~`, as people read it:
/// `~/src/app`. Only a whole path component is replaced, so a sibling such
/// as `/Users/bob2` of home `/Users/bob` stays as it is.
pub fn home_path(path: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    abbreviated(path, &home)
}

/// `path` with `home` written as `~`, at a component boundary only.
fn abbreviated(path: &str, home: &str) -> String {
    let home = home.trim_end_matches('/');
    if home.is_empty() {
        return path.to_string();
    }
    match path.strip_prefix(home) {
        Some("") => "~".to_string(),
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => path.to_string(),
    }
}

/// A tag as agd shows it in `notation`, as a capture line takes it:
/// `#work`, or Org's `:work:`. Every tag drawn goes through here or `tags`;
/// `+work` and `-work` are only the tags prompt's add and remove.
pub fn tag(notation: Notation, name: &str) -> String {
    match notation {
        Notation::Agd => format!("#{name}"),
        Notation::Org => format!(":{name}:"),
    }
}

/// Tags as agd shows them in `notation`: `#work #ops`, or Org's
/// `:work:ops:`; empty without tags.
pub fn tags(notation: Notation, names: &[String]) -> String {
    match notation {
        _ if names.is_empty() => String::new(),
        Notation::Agd => names
            .iter()
            .map(|name| tag(notation, name))
            .collect::<Vec<_>>()
            .join(" "),
        Notation::Org => format!(":{}:", names.join(":")),
    }
}

/// A priority as a capture line takes it in `notation`: `!A`, or Org's
/// `#A`. Rows show the letter alone, as a pill.
pub fn priority(notation: Notation, letter: &str) -> String {
    match notation {
        Notation::Agd => format!("!{letter}"),
        Notation::Org => format!("#{letter}"),
    }
}

/// `count` with its noun, singular for one: "1 file", "0 files".
pub fn counted(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("1 {singular}")
    } else {
        format!("{count} {plural}")
    }
}

/// The day's counts as the status band, the TUI and the panes say them:
/// `1 overdue · 3 today · 9 inbox · 2 unreviewed`, leaving out zero
/// counts; empty when every count is zero.
pub fn summary(summary: &AgendaSummary) -> String {
    [
        (summary.overdue, "overdue"),
        (summary.today, "today"),
        (summary.inbox, "inbox"),
        (summary.unreviewed, "unreviewed"),
    ]
    .iter()
    .filter(|(count, _)| *count > 0)
    .map(|(count, word)| counted(*count, word, word))
    .collect::<Vec<_>>()
    .join(" · ")
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

/// A core error as a sentence. Most messages already read as one; a ref
/// conflict's names the raw ref and invalid_edit's is about the edit, so
/// they are said plainly.
pub fn error_sentence(error: &AgdError) -> String {
    match error.code() {
        ErrorCode::RefConflict => "this heading changed on disk since agd read it".to_string(),
        ErrorCode::InvalidEdit => "nothing would change".to_string(),
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
        assert_eq!(moment("2026-10-16T17:22:05+02:00", today), "17:22");
        assert_eq!(
            moment("2026-10-15T17:22:05+02:00", today),
            "Thu 15 Oct 17:22"
        );
        assert_eq!(
            moment("2025-12-31T09:05:00+00:00", today),
            "Wed 31 Dec 2025 09:05"
        );
    }

    #[test]
    fn errors_name_files_relative_to_their_root() {
        let roots = [PathBuf::from("/notes/org")];
        assert_eq!(
            relative_paths("refused: /notes/org/work/a.org is a symlink", &roots),
            "refused: work/a.org is a symlink"
        );
        assert_eq!(
            relative_paths("/notes/org2/a.org", &roots),
            "/notes/org2/a.org"
        );
    }

    #[test]
    fn home_is_abbreviated_only_at_a_whole_component() {
        assert_eq!(abbreviated("/Users/bob/src/app", "/Users/bob"), "~/src/app");
        assert_eq!(abbreviated("/Users/bob", "/Users/bob/"), "~");
        assert_eq!(
            abbreviated("/Users/bob2/src", "/Users/bob"),
            "/Users/bob2/src"
        );
        assert_eq!(abbreviated("/srv/org", ""), "/srv/org");
    }

    #[test]
    fn tags_and_priorities_read_in_the_configured_notation() {
        let names = ["work".to_string(), "ops".to_string()];
        assert_eq!(tag(Notation::Agd, "work"), "#work");
        assert_eq!(tags(Notation::Agd, &names), "#work #ops");
        assert_eq!(priority(Notation::Agd, "A"), "!A");
        assert_eq!(tag(Notation::Org, "work"), ":work:");
        assert_eq!(tags(Notation::Org, &names), ":work:ops:");
        assert_eq!(priority(Notation::Org, "A"), "#A");
        assert_eq!(tags(Notation::Agd, &[]), "");
        assert_eq!(tags(Notation::Org, &[]), "");
    }

    #[test]
    fn counts_name_their_noun_and_the_day_leaves_out_zeros() {
        assert_eq!(counted(1, "file", "files"), "1 file");
        assert_eq!(counted(0, "file", "files"), "0 files");
        let day = AgendaSummary {
            overdue: 4,
            today: 1,
            inbox: 0,
            unreviewed: 1,
        };
        assert_eq!(summary(&day), "4 overdue · 1 today · 1 unreviewed");
        assert_eq!(summary(&AgendaSummary::default()), "");
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
