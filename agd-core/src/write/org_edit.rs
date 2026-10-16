//! The Org text an edit writes: patches for the headline (state, title,
//! priority, tags aligned to `tags_column`), the planning line, the property
//! drawer and LOGBOOK entries, following Org's logging (`!` and `@` markers,
//! the org-log-* settings) and repeaters (org-auto-repeat-maybe).
//!
//! Each patch spans only the line or drawer it replaces and keeps the entry's
//! indentation and line endings, so every other byte is untouched. It only
//! computes patches; applying and writing them is the caller's.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{Duration, Months, NaiveDate, NaiveDateTime};

use crate::{
    AgdError, ByteSpan, ErrorCode, HeadlineSpan, IdPolicy, IndexedDocument, LogKind,
    PlanningTimestamp, Settings, format_active_timestamp,
};

use super::edit::{Change, EditIntent};
use super::patch::{Patch, line_ending};

pub(super) fn build_edit_patches(
    document: &IndexedDocument,
    headline: &HeadlineSpan,
    intent: &EditIntent,
    settings: &Settings,
    now: NaiveDateTime,
) -> Result<Vec<Patch>, AgdError> {
    let source = document.source();
    let logging = document.logging();
    let stamp = format!("[{}]", now.format("%Y-%m-%d %a %H:%M"));
    let mut patches = Vec::new();
    let previous_state = headline.item.todo.clone();
    let mut state = change_value(&headline.item.todo, &intent.state);
    if let Some(requested) = &state
        && !document.todo_active().contains(requested)
        && !document.todo_done().contains(requested)
    {
        return Err(AgdError::new(
            ErrorCode::InvalidTodo,
            format!("TODO state '{requested}' is not enabled for this file"),
        ));
    }
    let mut scheduled = planning_change(&headline.item.planning.scheduled, &intent.scheduled);
    let mut deadline = planning_change(&headline.item.planning.deadline, &intent.deadline);
    let mut closed = headline.item.planning.closed.clone();
    let mut properties = intent.properties.clone();
    if settings.ids == IdPolicy::AddOnWrite
        && headline.item.id.is_none()
        && !properties.contains_key("ID")
    {
        // Same shape as Emacs org-id with the default uuid method.
        properties.insert("ID".to_string(), Some(uuid::Uuid::new_v4().to_string()));
    }
    // LOGBOOK entries in Org's org-log-note-headings format, newest first.
    let mut log: Vec<LogEntry> = Vec::new();
    let mut note = intent.note.clone();
    for (old, new, logged, moved, removed) in [
        (
            &headline.item.planning.scheduled,
            &scheduled,
            logging.reschedule,
            "Rescheduled from",
            "Not scheduled, was",
        ),
        (
            &headline.item.planning.deadline,
            &deadline,
            logging.redeadline,
            "New deadline from",
            "Removed deadline, was",
        ),
    ] {
        let (Some(kind), Some(old)) = (logged, old) else {
            continue;
        };
        let heading = match new {
            Some(new) if format_active_timestamp(new)? == format_active_timestamp(old)? => {
                continue;
            }
            Some(_) => format!("{moved} \"{}\" on {stamp}", inactive_timestamp(old)?),
            None => format!("{removed} \"{}\" on {stamp}", inactive_timestamp(old)?),
        };
        log.push(LogEntry::new(heading, kind, &mut note));
    }

    let state_changed = !matches!(intent.state, Change::Keep) && state != previous_state;
    let requested_done = state_changed
        && state
            .as_ref()
            .is_some_and(|value| done_keywords(document).contains(value));
    let repeating = scheduled
        .as_ref()
        .is_some_and(|timestamp| timestamp.repeater.is_some())
        || deadline
            .as_ref()
            .is_some_and(|timestamp| timestamp.repeater.is_some());
    let state_heading = |to: &str| {
        let from = previous_state
            .as_deref()
            .map(|value| format!("\"{value}\""))
            .unwrap_or_default();
        format!("State {:<12} from {from:<12} {stamp}", format!("\"{to}\""))
    };
    // A keyword's own `!`/`@` marker on entering, or the previous one's on
    // leaving, decides whether the change is logged (org-todo-log-states).
    let keyword_log = state.as_deref().and_then(|value| {
        document
            .todo_keyword(value)
            .and_then(|keyword| keyword.log_enter)
            .or_else(|| {
                previous_state
                    .as_deref()
                    .and_then(|value| document.todo_keyword(value))
                    .and_then(|keyword| keyword.log_leave)
            })
    });
    if requested_done && repeating {
        // org-auto-repeat-maybe: shift the dates, return to the head of the
        // previous keyword's sequence, drop CLOSED, log only per org-log-repeat.
        if let Some(timestamp) = scheduled.as_mut()
            && timestamp.repeater.is_some()
        {
            advance_repeater(timestamp, now.date())?;
        }
        if let Some(timestamp) = deadline.as_mut()
            && timestamp.repeater.is_some()
        {
            advance_repeater(timestamp, now.date())?;
        }
        let done_word = state.clone().unwrap_or_default();
        state = sequence_head(document, previous_state.as_deref());
        closed = None;
        if let Some(kind) = logging.repeat {
            properties.insert("LAST_REPEAT".to_string(), Some(stamp.clone()));
            log.insert(0, LogEntry::new(state_heading(&done_word), kind, &mut note));
        }
    } else if requested_done {
        if logging.done.is_some() {
            closed = Some(PlanningTimestamp {
                date: now.format("%Y-%m-%d").to_string(),
                time: Some(now.format("%H:%M").to_string()),
                end_time: None,
                repeater: None,
                warning: None,
                raw: None,
            });
        }
        let done_word = state.clone().unwrap_or_default();
        if let Some(kind) = keyword_log {
            log.insert(0, LogEntry::new(state_heading(&done_word), kind, &mut note));
        } else if logging.done == Some(LogKind::Note) {
            log.insert(
                0,
                LogEntry::new(format!("CLOSING NOTE {stamp}"), LogKind::Note, &mut note),
            );
        }
    } else if state_changed {
        closed = None;
        if let Some(kind) = keyword_log {
            let to = state.clone().unwrap_or_default();
            log.insert(0, LogEntry::new(state_heading(&to), kind, &mut note));
        }
    }
    if let Some(text) = note.take() {
        log.insert(
            0,
            LogEntry {
                heading: format!("Note taken on {stamp}"),
                note: Some(text),
            },
        );
    }

    if !matches!(intent.state, Change::Keep)
        || !matches!(intent.title, Change::Keep)
        || !matches!(intent.priority, Change::Keep)
        || !intent.tags_add.is_empty()
        || !intent.tags_remove.is_empty()
    {
        let title = change_value(&Some(headline.item.title.clone()), &intent.title)
            .ok_or_else(|| AgdError::new(ErrorCode::InvalidEdit, "title cannot be cleared"))?;
        let priority = change_value(&headline.item.priority, &intent.priority);
        let mut tags: Vec<String> = headline
            .item
            .tags
            .iter()
            .filter(|tag| !intent.tags_remove.contains(*tag))
            .cloned()
            .collect();
        for tag in &intent.tags_add {
            if !tags.contains(tag) {
                tags.push(tag.clone());
            }
        }
        let line = align_tags(
            render_headline(
                headline.level,
                state.as_deref(),
                priority.as_deref(),
                &title,
                &tags,
            ),
            !tags.is_empty(),
            settings.tags_column,
        );
        patches.push(Patch {
            span: headline.line_span,
            replacement: line,
        });
    }

    if !matches!(intent.scheduled, Change::Keep)
        || !matches!(intent.deadline, Change::Keep)
        || closed != headline.item.planning.closed
        || repeating
    {
        let replacement =
            render_planning_line(scheduled.as_ref(), deadline.as_ref(), closed.as_ref())?;
        let indent = headline
            .planning_span
            .map_or("", |span| leading_indent(source, span.start));
        patches.push(Patch {
            span: headline.planning_span.unwrap_or(ByteSpan {
                start: after_headline_line(source, headline.line_span.end),
                end: after_headline_line(source, headline.line_span.end),
            }),
            replacement: replacement
                .map(|line| format!("{indent}{line}{}", line_ending(source)))
                .unwrap_or_default(),
        });
    }

    if !properties.is_empty() {
        let replacement = render_properties(source, headline, &properties, line_ending(source));
        let insert = headline.drawer_span.unwrap_or(ByteSpan {
            start: headline.planning_span.map_or(
                after_headline_line(source, headline.line_span.end),
                |span| span.end,
            ),
            end: headline.planning_span.map_or(
                after_headline_line(source, headline.line_span.end),
                |span| span.end,
            ),
        });
        patches.push(Patch {
            span: insert,
            replacement,
        });
    }

    if !log.is_empty() {
        patches.push(log_patch(source, headline, &log, logging.into_drawer)?);
    }
    Ok(patches)
}

/// One `- heading` line in a LOGBOOK, with an optional note below it.
struct LogEntry {
    heading: String,
    note: Option<String>,
}

impl LogEntry {
    /// A `@` (note) entry takes the edit's note text, if any; a `!` (time)
    /// entry leaves it for a separate "Note taken" entry.
    fn new(heading: String, kind: LogKind, note: &mut Option<String>) -> Self {
        let note = if kind == LogKind::Note {
            note.take()
        } else {
            None
        };
        Self { heading, note }
    }
}

/// Inserts log entries where org-log-beginning would: at the top of the
/// entry's LOGBOOK drawer (created after planning and properties when
/// missing) with org-log-into-drawer, otherwise right after them.
fn log_patch(
    source: &str,
    headline: &HeadlineSpan,
    entries: &[LogEntry],
    into_drawer: bool,
) -> Result<Patch, AgdError> {
    let newline = line_ending(source);
    let start = headline.drawer_span.or(headline.planning_span).map_or_else(
        || after_headline_line(source, headline.line_span.end),
        |span| span.end,
    );
    let indent = headline
        .drawer_span
        .or(headline.planning_span)
        .map_or("", |span| leading_indent(source, span.start));
    let mut lines = String::new();
    for entry in entries {
        let Some(note) = &entry.note else {
            lines.push_str(&format!("{indent}- {}{newline}", entry.heading));
            continue;
        };
        if note.contains(['\n', '\r']) {
            return Err(AgdError::new(
                ErrorCode::InvalidNote,
                "log notes must be one line",
            ));
        }
        lines.push_str(&format!(
            "{indent}- {} \\\\{newline}{indent}  {note}{newline}",
            entry.heading
        ));
    }
    if into_drawer {
        let section = &source[start..headline.section_span.end];
        if let Some(offset) = logbook_line_end(section) {
            let at = start + offset;
            return Ok(Patch {
                span: ByteSpan { start: at, end: at },
                replacement: lines,
            });
        }
        lines = format!("{indent}:LOGBOOK:{newline}{lines}{indent}:END:{newline}");
    }
    Ok(Patch {
        span: ByteSpan { start, end: start },
        replacement: lines,
    })
}

/// End of the `:LOGBOOK:` line within a section, if the entry has one.
fn logbook_line_end(section: &str) -> Option<usize> {
    let mut offset = 0;
    for line in section.split_inclusive('\n') {
        offset += line.len();
        if line.trim().eq_ignore_ascii_case(":LOGBOOK:") {
            return Some(offset);
        }
    }
    None
}

/// Org quotes the old timestamp of a reschedule as an inactive one.
fn inactive_timestamp(timestamp: &PlanningTimestamp) -> Result<String, AgdError> {
    let active = format_active_timestamp(timestamp)?;
    Ok(format!("[{}]", &active[1..active.len() - 1]))
}

/// Setting a new SCHEDULED or DEADLINE date keeps the old repeater and
/// warning cookie unless the new value has its own, as org-schedule does.
fn planning_change(
    current: &Option<PlanningTimestamp>,
    change: &Change<PlanningTimestamp>,
) -> Option<PlanningTimestamp> {
    match (change, current) {
        (Change::Set(new), Some(old)) => {
            let mut new = new.clone();
            new.repeater = new.repeater.or_else(|| old.repeater.clone());
            new.warning = new.warning.or_else(|| old.warning.clone());
            Some(new)
        }
        _ => change_value(current, change),
    }
}

/// The first keyword of the sequence `state` belongs to; a repeating task
/// returns there when marked done (org-todo-kwd-alist head).
fn sequence_head(document: &IndexedDocument, state: Option<&str>) -> Option<String> {
    let sequence = state
        .and_then(|value| document.todo_keyword(value))
        .map_or(0, |keyword| keyword.sequence);
    document
        .todo_keywords()
        .iter()
        .find(|keyword| keyword.sequence == sequence)
        .or_else(|| document.todo_keywords().first())
        .map(|keyword| keyword.name.clone())
}

fn change_value<T: Clone>(current: &Option<T>, change: &Change<T>) -> Option<T> {
    match change {
        Change::Keep => current.clone(),
        Change::Set(value) => Some(value.clone()),
        Change::Clear => None,
    }
}

fn done_keywords(document: &IndexedDocument) -> BTreeSet<String> {
    document.todo_done().iter().cloned().collect()
}

fn render_headline(
    level: usize,
    state: Option<&str>,
    priority: Option<&str>,
    title: &str,
    tags: &[String],
) -> String {
    let mut fields = vec!["*".repeat(level)];
    if let Some(state) = state {
        fields.push(state.to_string());
    }
    if let Some(priority) = priority {
        fields.push(format!("[#{priority}]"));
    }
    fields.push(title.to_string());
    let mut line = fields.join(" ");
    if !tags.is_empty() {
        line.push_str(&format!(" :{}:", tags.join(":")));
    }
    line
}

/// Aligns tags to org-tags-column the way Emacs does whenever it rewrites a
/// headline: a negative column right-aligns them to end there, a positive
/// one starts them there, and a long title keeps a single space.
pub(super) fn align_tags(rendered: String, has_tags: bool, column: i64) -> String {
    use unicode_width::UnicodeWidthStr;

    let Some(tags_start) = rendered.rfind(" :").filter(|_| has_tags) else {
        return rendered;
    };
    let (head, tags) = rendered.split_at(tags_start);
    let tags = tags.trim_start();
    let start = if column < 0 {
        column.unsigned_abs() as usize - tags.width().min(column.unsigned_abs() as usize)
    } else {
        column as usize
    };
    let gap = start.saturating_sub(head.width()).max(1);
    format!("{head}{}{tags}", " ".repeat(gap))
}

fn render_planning_line(
    scheduled: Option<&PlanningTimestamp>,
    deadline: Option<&PlanningTimestamp>,
    closed: Option<&PlanningTimestamp>,
) -> Result<Option<String>, AgdError> {
    let mut fields = Vec::new();
    if let Some(timestamp) = scheduled {
        fields.push(format!("SCHEDULED: {}", render_timestamp(timestamp, true)?));
    }
    if let Some(timestamp) = deadline {
        fields.push(format!("DEADLINE: {}", render_timestamp(timestamp, true)?));
    }
    if let Some(timestamp) = closed {
        fields.push(format!("CLOSED: {}", render_timestamp(timestamp, false)?));
    }
    Ok((!fields.is_empty()).then(|| fields.join(" ")))
}

fn render_timestamp(timestamp: &PlanningTimestamp, active: bool) -> Result<String, AgdError> {
    if active {
        return format_active_timestamp(timestamp);
    }
    let active = format_active_timestamp(timestamp)?;
    Ok(format!("[{}]", &active[1..active.len() - 1]))
}

fn render_properties(
    source: &str,
    headline: &HeadlineSpan,
    changes: &BTreeMap<String, Option<String>>,
    newline: &str,
) -> String {
    let mut properties: Vec<(String, String)> = headline
        .properties
        .iter()
        .map(|property| (property.key.clone(), property.value.clone()))
        .collect();
    for (key, value) in changes {
        let position = properties
            .iter()
            .position(|(existing, _)| existing.eq_ignore_ascii_case(key));
        match (position, value) {
            (Some(index), Some(value)) => properties[index].1 = value.clone(),
            (Some(index), None) => {
                properties.remove(index);
            }
            (None, Some(value)) => properties.push((key.clone(), value.clone())),
            (None, None) => {}
        }
    }
    if properties.is_empty() {
        return String::new();
    }
    let indent = headline
        .drawer_span
        .or(headline.planning_span)
        .map_or("", |span| leading_indent(source, span.start));
    let mut output = format!("{indent}:PROPERTIES:{newline}");
    for (key, value) in properties {
        output.push_str(&format!("{indent}:{key}: {value}{newline}"));
    }
    output.push_str(&format!("{indent}:END:{newline}"));
    output
}

fn advance_repeater(timestamp: &mut PlanningTimestamp, today: NaiveDate) -> Result<(), AgdError> {
    let repeater = timestamp
        .repeater
        .as_deref()
        .ok_or_else(|| AgdError::new(ErrorCode::InvalidRepeater, "timestamp has no repeater"))?;
    let (kind, amount_and_unit) = if let Some(value) = repeater.strip_prefix("++") {
        ("++", value)
    } else if let Some(value) = repeater.strip_prefix(".+") {
        (".+", value)
    } else if let Some(value) = repeater.strip_prefix('+') {
        ("+", value)
    } else {
        return Err(AgdError::new(
            ErrorCode::InvalidRepeater,
            format!("invalid repeater: {repeater}"),
        ));
    };
    let split = amount_and_unit.len().saturating_sub(1);
    let amount = amount_and_unit[..split].parse::<u64>().map_err(|_| {
        AgdError::new(
            ErrorCode::InvalidRepeater,
            format!("invalid repeater: {repeater}"),
        )
    })?;
    let unit = &amount_and_unit[split..];
    let original = NaiveDate::parse_from_str(&timestamp.date, "%Y-%m-%d").map_err(|_| {
        AgdError::new(
            ErrorCode::InvalidDate,
            format!("invalid date: {}", timestamp.date),
        )
    })?;
    let mut next = if kind == ".+" {
        add_period(today, amount, unit)?
    } else {
        add_period(original, amount, unit)?
    };
    if kind == "++" {
        while next <= today {
            next = add_period(next, amount, unit)?;
        }
    }
    timestamp.date = next.format("%Y-%m-%d").to_string();
    timestamp.raw = None;
    Ok(())
}

fn add_period(date: NaiveDate, amount: u64, unit: &str) -> Result<NaiveDate, AgdError> {
    let result = match unit {
        "d" => date.checked_add_signed(Duration::days(amount as i64)),
        "w" => date.checked_add_signed(Duration::weeks(amount as i64)),
        "m" => date.checked_add_months(Months::new(amount as u32)),
        "y" => date.checked_add_months(Months::new((amount * 12) as u32)),
        _ => None,
    };
    result.ok_or_else(|| AgdError::new(ErrorCode::InvalidRepeater, "repeater date is out of range"))
}

fn after_headline_line(source: &str, content_end: usize) -> usize {
    if source.as_bytes().get(content_end) == Some(&b'\r')
        && source.as_bytes().get(content_end + 1) == Some(&b'\n')
    {
        content_end + 2
    } else if source.as_bytes().get(content_end) == Some(&b'\n') {
        content_end + 1
    } else {
        content_end
    }
}

fn leading_indent(source: &str, offset: usize) -> &str {
    let end = source[offset..]
        .find(['\r', '\n'])
        .map_or(source.len(), |relative| offset + relative);
    let line = &source[offset..end];
    let bytes = line
        .bytes()
        .take_while(|byte| matches!(byte, b' ' | b'\t'))
        .count();
    &line[..bytes]
}

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::Local;

    use super::*;
    use crate::write::test_support::{context, settings};
    use crate::{Repeat, WorkspaceIndex, WriteEngine};

    #[test]
    fn add_on_write_inserts_an_id_only_when_missing() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tasks.org");
        fs::write(
            &path,
            "* TODO New\n* TODO Known\n:PROPERTIES:\n:ID: kept\n:END:\n",
        )
        .unwrap();
        let mut settings = settings(temp.path());
        settings.ids = IdPolicy::AddOnWrite;
        let refs: Vec<String> = WorkspaceIndex::load(settings.clone())
            .unwrap()
            .items()
            .map(|item| item.item_ref.clone())
            .collect();
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl")).unwrap();
        let done = EditIntent {
            state: Change::Set("DONE".to_string()),
            ..EditIntent::default()
        };
        let added = engine.edit(&refs[0], &done, false, &context()).unwrap();
        let id = added.item.id.clone().unwrap();
        assert_eq!(id.len(), 36);
        assert_eq!(added.item.item_ref, id);
        engine.edit("kept", &done, false, &context()).unwrap();
        let written = fs::read_to_string(&path).unwrap();
        assert_eq!(written.matches(":ID:").count(), 2);
        assert!(
            written.contains(&format!(":ID:       {id}"))
                || written.contains(&format!(":ID: {id}"))
        );
    }

    #[test]
    fn completing_repeater_advances_date_and_records_last_repeat() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("repeat.org");
        fs::write(
            &path,
            "#+TODO: TODO | DONE\n* TODO Water plants\nSCHEDULED: <2020-01-01 Wed .+1d>\n",
        )
        .unwrap();
        let settings = settings(temp.path());
        let item_ref = WorkspaceIndex::load(settings.clone())
            .unwrap()
            .items()
            .next()
            .unwrap()
            .item_ref
            .clone();
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl")).unwrap();
        let repeated = engine
            .edit(
                &item_ref,
                &EditIntent {
                    state: Change::Set("DONE".to_string()),
                    ..EditIntent::default()
                },
                false,
                &context(),
            )
            .unwrap();
        let written = fs::read_to_string(path).unwrap();
        let tomorrow = (Local::now().date_naive() + Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        assert_eq!(
            repeated.repeated,
            Some(Repeat {
                done_state: "DONE".to_string(),
                next_date: tomorrow.clone(),
            })
        );
        assert!(written.contains("* TODO Water plants\n"));
        assert!(written.contains(&format!("SCHEDULED: <{tomorrow}")));
        assert!(written.contains(":LAST_REPEAT: ["));
    }

    #[test]
    fn indented_planning_edit_and_undo_are_byte_exact() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("agenda.org");
        let original = concat!(
            "* Work Tasks\n",
            "** TODO [#A] Complete quarterly report                                :work:\n",
            "   SCHEDULED: <2026-10-14 Wed> DEADLINE: <2026-10-19 Mon>\n",
            "   :PROPERTIES:\n",
            "   :ID: work-task-001\n",
            "   :END:\n",
            "   Need to finish Q3 report for the board meeting.\n\n",
            "** TODO [#B] Review pull requests :work:\n",
        );
        fs::write(&path, original).unwrap();
        let settings = settings(temp.path());
        let engine = WriteEngine::new(settings, temp.path().join("journal-real.jsonl")).unwrap();
        let mutation = engine
            .edit(
                "work-task-001",
                &EditIntent {
                    scheduled: Change::Set(PlanningTimestamp {
                        date: "2026-10-30".to_string(),
                        time: None,
                        end_time: None,
                        repeater: None,
                        warning: None,
                        raw: None,
                    }),
                    ..EditIntent::default()
                },
                false,
                &context(),
            )
            .unwrap();
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains("   SCHEDULED: <2026-10-30 Fri> DEADLINE:"));
        assert!(written.contains("   Need to finish Q3 report for the board meeting."));
        engine.undo(&mutation.journal_id, &context()).unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), original);
    }
}
