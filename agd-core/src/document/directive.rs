//! In-file directives and the settings they override: TODO keyword sequences
//! with their logging markers (`#+TODO`, `#+SEQ_TODO`, `#+TYP_TODO`), logging
//! from `#+STARTUP`, the priority range from `#+PRIORITIES`, and the readers
//! for `#+KEY:` lines.
//!
//! A file's own lines replace the configured ones; a malformed `#+PRIORITIES`
//! keeps the configured range rather than guessing.

use crate::{LogMode, Settings};

/// How a state change or planning change is recorded, as in Org's `!` (time)
/// and `@` (note) markers and the org-log-* variables.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogKind {
    Time,
    Note,
}

/// One TODO keyword with the logging markers from `KEYWORD(key!/@)`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TodoKeyword {
    pub name: String,
    pub done: bool,
    /// Index of the sequence it belongs to; repeaters reset to its first keyword.
    pub sequence: usize,
    pub log_enter: Option<LogKind>,
    pub log_leave: Option<LogKind>,
}

/// A file's priority cookies, highest to lowest, and the priority of a
/// heading without one: `#+PRIORITIES: A C B` (org-priority-highest,
/// -lowest and -default), else the configured range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PriorityRange {
    pub highest: char,
    pub lowest: char,
    pub default: char,
}

impl PriorityRange {
    /// The configured range (`highest_priority`, `lowest_priority`,
    /// `default_priority`).
    pub fn from_settings(settings: &Settings) -> Self {
        let first = |value: &str, fallback: char| value.chars().next().unwrap_or(fallback);
        Self {
            highest: first(&settings.highest_priority, 'A'),
            lowest: first(&settings.lowest_priority, 'C'),
            default: first(&settings.default_priority, 'B'),
        }
    }

    /// Whether `priority` is one cookie character within the range.
    pub fn contains(&self, priority: &str) -> bool {
        let mut characters = priority.chars();
        match (characters.next(), characters.next()) {
            (Some(value), None) => (self.highest..=self.lowest).contains(&value),
            _ => false,
        }
    }

    /// Every cookie from highest to lowest, as `[A, B, C]`.
    pub fn cookies(&self) -> Vec<String> {
        (self.highest..=self.lowest)
            .map(|value| value.to_string())
            .collect()
    }

    /// Org's numeric priority (org-get-priority): 1000 for each step above
    /// the lowest cookie, a heading without one counting as the default.
    pub fn value(&self, priority: Option<&str>) -> i64 {
        let cookie = priority
            .and_then(|value| value.chars().next())
            .unwrap_or(self.default);
        1000 * (self.lowest as i64 - cookie as i64)
    }

    /// `A–C`, for messages.
    pub fn describe(&self) -> String {
        format!("{}–{}", self.highest, self.lowest)
    }
}

/// Effective logging for one file: configuration overridden by `#+STARTUP`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogSettings {
    pub done: Option<LogKind>,
    pub repeat: Option<LogKind>,
    pub reschedule: Option<LogKind>,
    pub redeadline: Option<LogKind>,
    pub into_drawer: bool,
}

/// TODO keywords from every `#+TODO:`/`#+SEQ_TODO:`/`#+TYP_TODO:` line, or
/// the configured `todo_keywords` lines, which hold the same text. A
/// sequence without `|` makes its last keyword the done state, as in Org.
pub(super) fn todo_keywords(source: &str, settings: &Settings) -> Vec<TodoKeyword> {
    let in_file: Vec<&str> = directives(source, &["TODO", "SEQ_TODO", "TYP_TODO"]).collect();
    let lines = if in_file.is_empty() {
        settings.todo_keywords.iter().map(String::as_str).collect()
    } else {
        in_file
    };
    let mut keywords = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let sequence: Vec<&str> = line.split_whitespace().collect();
        let separator = sequence.iter().position(|word| *word == "|");
        let last = sequence.len().saturating_sub(1);
        for (position, raw) in sequence.iter().enumerate() {
            if *raw == "|" {
                continue;
            }
            let (name, markers) = raw.split_once('(').map_or((*raw, ""), |(name, rest)| {
                (name, rest.trim_end_matches(')'))
            });
            if name.is_empty() {
                continue;
            }
            let (enter, leave) = markers.split_once('/').unwrap_or((markers, ""));
            keywords.push(TodoKeyword {
                name: name.to_string(),
                done: separator.map_or(position == last, |separator| position > separator),
                sequence: index,
                log_enter: log_marker(enter),
                log_leave: log_marker(leave),
            });
        }
    }
    keywords
}

fn log_marker(marker: &str) -> Option<LogKind> {
    if marker.contains('@') {
        Some(LogKind::Note)
    } else if marker.contains('!') {
        Some(LogKind::Time)
    } else {
        None
    }
}

fn log_value(mode: LogMode) -> Option<LogKind> {
    match mode {
        LogMode::Off => None,
        LogMode::Time => Some(LogKind::Time),
        LogMode::Note => Some(LogKind::Note),
    }
}

/// org-log-* settings for one file, with `#+STARTUP:` options such as
/// `logrepeat`, `lognotereschedule` or `nologdrawer` applied on top.
pub(super) fn log_settings(source: &str, settings: &Settings) -> LogSettings {
    let agenda = settings;
    let mut logging = LogSettings {
        done: log_value(agenda.log_done),
        repeat: log_value(agenda.log_repeat),
        reschedule: log_value(agenda.log_reschedule),
        redeadline: log_value(agenda.log_redeadline),
        into_drawer: agenda.log_into_drawer,
    };
    for option in directives(source, &["STARTUP"]).flat_map(str::split_whitespace) {
        let option = option.to_ascii_lowercase();
        let (kind, name) = if let Some(name) = option.strip_prefix("nolog") {
            (None, name)
        } else if let Some(name) = option.strip_prefix("lognote") {
            (Some(LogKind::Note), name)
        } else if let Some(name) = option.strip_prefix("log") {
            (Some(LogKind::Time), name)
        } else {
            continue;
        };
        match name {
            "done" => logging.done = kind,
            "repeat" => logging.repeat = kind,
            "reschedule" => logging.reschedule = kind,
            "redeadline" => logging.redeadline = kind,
            "drawer" => logging.into_drawer = kind.is_some(),
            _ => {}
        }
    }
    logging
}

/// The file's priority range: its `#+PRIORITIES: highest lowest default`
/// line when agd can use it, else the configured range.
pub(super) fn priority_range(source: &str, settings: &Settings) -> PriorityRange {
    directive(source, "PRIORITIES")
        .and_then(priorities_line)
        .unwrap_or_else(|| PriorityRange::from_settings(settings))
}

/// A `#+PRIORITIES` line agd ignores, keeping the configured range: one
/// with numeric priorities, which agd does not support, or one that is not
/// three letters running highest, lowest, default. `agd setup check`
/// reports it.
pub(crate) fn ignored_priorities_line(source: &str) -> Option<&str> {
    directive(source, "PRIORITIES").filter(|value| priorities_line(value).is_none())
}

/// `A C B`: three uppercase letters, highest before lowest, the default
/// between them.
fn priorities_line(value: &str) -> Option<PriorityRange> {
    let letters: Vec<char> = value
        .split_whitespace()
        .map(|token| {
            let mut characters = token.chars();
            match (characters.next(), characters.next()) {
                (Some(letter @ 'A'..='Z'), None) => Some(letter),
                _ => None,
            }
        })
        .collect::<Option<_>>()?;
    match letters.as_slice() {
        [highest, lowest, default]
            if highest <= lowest && (highest..=lowest).contains(&default) =>
        {
            Some(PriorityRange {
                highest: *highest,
                lowest: *lowest,
                default: *default,
            })
        }
        _ => None,
    }
}

fn directives<'a>(source: &'a str, names: &'a [&str]) -> impl Iterator<Item = &'a str> {
    source.lines().filter_map(move |line| {
        let (key, value) = line.strip_prefix("#+")?.split_once(':')?;
        names
            .iter()
            .any(|name| key.eq_ignore_ascii_case(name))
            .then_some(value.trim())
    })
}

pub(super) fn directive<'a>(source: &'a str, name: &str) -> Option<&'a str> {
    source.lines().find_map(|line| {
        let (key, value) = line.strip_prefix("#+")?.split_once(':')?;
        key.eq_ignore_ascii_case(name).then_some(value.trim())
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::document::OrgDocument;

    #[test]
    fn in_file_priorities_override_the_configured_range() {
        let parse = |source: &str| {
            OrgDocument::parse(
                PathBuf::from("/tmp/p.org"),
                source.as_bytes().to_vec(),
                &Settings::default(),
            )
            .unwrap()
            .priorities()
        };
        let in_file = parse("#+PRIORITIES: A E C\n* TODO [#E] Low\n");
        assert_eq!(
            in_file,
            PriorityRange {
                highest: 'A',
                lowest: 'E',
                default: 'C'
            }
        );
        assert!(in_file.contains("E"));
        assert!(!in_file.contains("F"));
        assert_eq!(in_file.value(None), in_file.value(Some("C")));
        assert!(in_file.value(Some("A")) > in_file.value(Some("E")));
        // Numeric and malformed lines keep the configured A-C, default B.
        for source in [
            "#+PRIORITIES: 1 5 3\n",
            "#+PRIORITIES: C A B\n",
            "#+PRIORITIES: A C\n",
            "#+PRIORITIES: 1 10 5\n",
        ] {
            assert_eq!(
                parse(source),
                PriorityRange::from_settings(&Settings::default())
            );
            assert!(ignored_priorities_line(source).is_some(), "{source}");
        }
        assert_eq!(ignored_priorities_line("#+PRIORITIES: A E C\n"), None);
    }
}
