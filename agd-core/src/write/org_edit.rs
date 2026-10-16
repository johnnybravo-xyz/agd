//! The Org text an edit writes: patches for the headline (state, title,
//! priority, tags aligned to `tags_column`), the planning line, the property
//! drawer and LOGBOOK entries, following Org's logging (`!` and `@` markers,
//! the org-log-* settings) and repeaters (org-auto-repeat-maybe).
//!
//! Each patch spans only the line or drawer it replaces and keeps the entry's
//! indentation and line endings, so every other byte is untouched. It only
//! computes patches; applying and writing them is the caller's.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{NaiveDate, NaiveDateTime};

use crate::{
    AgdError, ByteSpan, ErrorCode, HeadlineSpan, IdPolicy, IndexedDocument, LogKind, Planning,
    PlanningTimestamp, Settings, format_active_timestamp,
};

use super::edit::{Change, EditIntent};
use super::patch::{Patch, line_ending};
use crate::agenda::next_repeat;
use crate::date::format_inactive_timestamp;

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
    // org-schedule and org-deadline remove CLOSED as they set a date.
    if matches!(intent.scheduled, Change::Set(_)) || matches!(intent.deadline, Change::Set(_)) {
        closed = None;
    }
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
            Some(_) => format!("{moved} \"{}\" on {stamp}", format_inactive_timestamp(old)?),
            None => format!(
                "{removed} \"{}\" on {stamp}",
                format_inactive_timestamp(old)?
            ),
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
            .ok_or_else(|| AgdError::new(ErrorCode::InvalidTitle, "title cannot be cleared"))?;
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

    let planning = &headline.item.planning;
    // Applied in this order, so dates an edit sets on an empty line read
    // SCHEDULED then DEADLINE, and CLOSED leads.
    let updates = [
        PlanningUpdate {
            keyword: PlanningKeyword::Deadline,
            old: planning.deadline.as_ref(),
            new: deadline.as_ref(),
            set: matches!(intent.deadline, Change::Set(_)),
        },
        PlanningUpdate {
            keyword: PlanningKeyword::Scheduled,
            old: planning.scheduled.as_ref(),
            new: scheduled.as_ref(),
            set: matches!(intent.scheduled, Change::Set(_)),
        },
        PlanningUpdate {
            keyword: PlanningKeyword::Closed,
            old: planning.closed.as_ref(),
            new: closed.as_ref(),
            set: closed != planning.closed,
        },
    ];
    if updates.iter().any(PlanningUpdate::changes) {
        let current_line = headline
            .planning_span
            .map_or("", |span| &source[span.start..span.end]);
        let replacement = planning_line(current_line, planning, &updates)?;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlanningKeyword {
    Scheduled,
    Deadline,
    Closed,
}

impl PlanningKeyword {
    fn label(self) -> &'static str {
        match self {
            Self::Scheduled => "SCHEDULED:",
            Self::Deadline => "DEADLINE:",
            Self::Closed => "CLOSED:",
        }
    }
}

/// One planning keyword's timestamp before and after an edit. `set` means
/// Org removes the keyword and inserts it again at the front of the line,
/// as org-add-planning-info does for a date the edit sets and for CLOSED.
struct PlanningUpdate<'a> {
    keyword: PlanningKeyword,
    old: Option<&'a PlanningTimestamp>,
    new: Option<&'a PlanningTimestamp>,
    set: bool,
}

impl PlanningUpdate<'_> {
    fn changes(&self) -> bool {
        self.set || self.old != self.new
    }
}

/// The planning line after `updates`, as Org writes it: a keyword an
/// update sets goes to the front, one it clears is removed, one a repeater
/// moved on changes in place, and every other keyword keeps its place and
/// its timestamp's bytes. None when no keyword is left.
fn planning_line(
    current_line: &str,
    planning: &Planning,
    updates: &[PlanningUpdate<'_>],
) -> Result<Option<String>, AgdError> {
    let mut written: Vec<(usize, PlanningKeyword, &PlanningTimestamp)> = [
        (PlanningKeyword::Scheduled, &planning.scheduled),
        (PlanningKeyword::Deadline, &planning.deadline),
        (PlanningKeyword::Closed, &planning.closed),
    ]
    .into_iter()
    .filter_map(|(keyword, timestamp)| {
        let at = current_line.find(keyword.label())?;
        Some((at, keyword, timestamp.as_ref()?))
    })
    .collect();
    written.sort_by_key(|(at, _, _)| *at);
    let mut fields: Vec<(PlanningKeyword, String)> = Vec::new();
    for (_, keyword, timestamp) in written {
        let text = match &timestamp.raw {
            Some(raw) => raw.clone(),
            None => render_timestamp(timestamp, keyword != PlanningKeyword::Closed)?,
        };
        fields.push((keyword, format!("{} {text}", keyword.label())));
    }
    for update in updates.iter().filter(|update| update.changes()) {
        let position = fields
            .iter()
            .position(|(keyword, _)| *keyword == update.keyword);
        let Some(timestamp) = update.new else {
            if let Some(at) = position {
                fields.remove(at);
            }
            continue;
        };
        let active = update.keyword != PlanningKeyword::Closed;
        let field = format!(
            "{} {}",
            update.keyword.label(),
            render_timestamp(timestamp, active)?
        );
        match position {
            Some(at) if !update.set => fields[at].1 = field,
            _ => {
                if let Some(at) = position {
                    fields.remove(at);
                }
                fields.insert(0, (update.keyword, field));
            }
        }
    }
    let line: Vec<String> = fields.into_iter().map(|(_, field)| field).collect();
    Ok((!line.is_empty()).then(|| line.join(" ")))
}

fn render_timestamp(timestamp: &PlanningTimestamp, active: bool) -> Result<String, AgdError> {
    if active {
        format_active_timestamp(timestamp)
    } else {
        format_inactive_timestamp(timestamp)
    }
}

/// The properties drawer after `changes`, as Org edits it: every line agd
/// does not change keeps its exact bytes (the drawer's own `:PROPERTIES:`
/// and `:END:` lines too), and a changed or new property is written the
/// way Org writes one, in `org-property-format` (`%-10s %s`).
fn render_properties(
    source: &str,
    headline: &HeadlineSpan,
    changes: &BTreeMap<String, Option<String>>,
    newline: &str,
) -> String {
    let indent = headline
        .drawer_span
        .or(headline.planning_span)
        .map_or("", |span| leading_indent(source, span.start));
    let written =
        |key: &str, value: &str| format!("{indent}{:<10} {value}{newline}", format!(":{key}:"));
    let mut lines: Vec<(String, String)> = headline
        .properties
        .iter()
        .map(|property| {
            (
                property.key.clone(),
                source[property.line.start..property.line.end].to_string(),
            )
        })
        .collect();
    for (key, value) in changes {
        let position = lines
            .iter()
            .position(|(existing, _)| existing.eq_ignore_ascii_case(key));
        match (position, value) {
            (Some(index), Some(value)) => {
                let line = written(&lines[index].0, value);
                lines[index].1 = line;
            }
            (Some(index), None) => {
                lines.remove(index);
            }
            (None, Some(value)) => lines.push((key.clone(), written(key, value))),
            (None, None) => {}
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    let (open, close) = match headline.drawer_span {
        Some(drawer) => drawer_fences(&source[drawer.start..drawer.end]),
        None => (
            format!("{indent}:PROPERTIES:{newline}"),
            format!("{indent}:END:{newline}"),
        ),
    };
    let mut output = open;
    for (_, line) in lines {
        output.push_str(&line);
    }
    output.push_str(&close);
    output
}

/// A drawer's first and last lines, `:PROPERTIES:` and `:END:`, as written.
fn drawer_fences(drawer: &str) -> (String, String) {
    let open_end = drawer.find('\n').map_or(drawer.len(), |end| end + 1);
    let body = drawer.trim_end_matches(['\n', '\r']);
    let close_start = body.rfind('\n').map_or(0, |start| start + 1);
    (
        drawer[..open_end].to_string(),
        drawer[close_start..].to_string(),
    )
}

/// Moves a repeating timestamp on; a habit's maximum interval (`.+2d/3d`)
/// stays in the repeater text.
fn advance_repeater(timestamp: &mut PlanningTimestamp, today: NaiveDate) -> Result<(), AgdError> {
    timestamp.date = next_repeat(timestamp, today)?
        .format("%Y-%m-%d")
        .to_string();
    timestamp.raw = None;
    Ok(())
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

    use chrono::{Duration, Local};

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
    fn a_property_edit_keeps_every_other_drawer_line_byte_exact() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tasks.org");
        fs::write(
            &path,
            "* TODO Stretch\n:PROPERTIES:\n:STYLE:    habit\n:Effort: 0:15\n:END:\nBody.\n",
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
        engine
            .edit(
                &item_ref,
                &EditIntent {
                    properties: BTreeMap::from([
                        ("OWNER".to_string(), Some("me".to_string())),
                        ("Effort".to_string(), Some("0:30".to_string())),
                    ]),
                    ..EditIntent::default()
                },
                false,
                &context(),
            )
            .unwrap();
        // STYLE keeps its bytes; the changed Effort and the new OWNER are
        // written as Org writes them.
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "* TODO Stretch\n:PROPERTIES:\n:STYLE:    habit\n:Effort:   0:30\n:OWNER:    me\n:END:\nBody.\n"
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
