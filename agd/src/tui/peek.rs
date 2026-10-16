//! Peek (SPC): the entry under the cursor, beside the list from 100
//! columns and over it below: its dates as people say them, its state,
//! priority, tags, body and links, and why the agenda lists it. Also the
//! entry-text helpers other screens share (the body, links, the title as
//! the agenda shows it).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};

use agd_core::{Item, JournalOperation, Occurrence, OccurrenceType, WorkspaceIndex, next_repeat};

use super::actions::when;
use super::keys::{self, Action};
use super::theme::centered;
use super::{App, Screen};

/// The entry's own text: everything after the headline up to the first
/// child, including planning and drawers.
pub(super) fn entry_text(index: &WorkspaceIndex, item: &Item) -> String {
    index
        .resolve(&item.item_ref)
        .map(|(document, headline)| {
            document.source()[headline.line_span.end..headline.section_span.end].to_string()
        })
        .unwrap_or_default()
}

/// `[[target][description]]` and `[[target]]` links, in order.
pub(super) fn links(text: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("]]") else {
            break;
        };
        let inner = &after[..end];
        let (target, description) = inner.split_once("][").unwrap_or((inner, inner));
        found.push((target.to_string(), description.to_string()));
        rest = &after[end + 2..];
    }
    found
}

/// Body lines a reader cares about: no planning line, drawers or blank
/// edges.
fn body_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut in_drawer = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if in_drawer {
            in_drawer = !trimmed.eq_ignore_ascii_case(":END:");
            continue;
        }
        if trimmed.starts_with(':') && trimmed.ends_with(':') && trimmed.len() > 2 {
            in_drawer = true;
            continue;
        }
        if ["SCHEDULED:", "DEADLINE:", "CLOSED:"]
            .iter()
            .any(|keyword| trimmed.starts_with(keyword))
        {
            continue;
        }
        lines.push(link_descriptions(trimmed));
    }
    while lines.first().is_some_and(String::is_empty) {
        lines.remove(0);
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

/// A heading as the agenda shows it: no timestamps, links as their text.
pub(super) fn display_title(item: &Item) -> String {
    link_descriptions(&agd_core::agenda_title(item))
}

/// A link without a description as people read it: a path under home
/// with `~`, as Org's own `file:` links write it.
fn link_target_text(target: &str) -> String {
    match target.strip_prefix("file:") {
        Some(path) => format!("file:{}", crate::words::home_path(path)),
        None => crate::words::home_path(target),
    }
}

/// `[[target][description]]` reads as its description, `[[target]]` as
/// the target.
pub(super) fn link_descriptions(line: &str) -> String {
    let mut output = String::new();
    let mut rest = line;
    while let Some(start) = rest.find("[[") {
        output.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("]]") else {
            output.push_str(&rest[start..]);
            return output;
        };
        let inner = &after[..end];
        match inner.split_once("][") {
            Some((_, description)) => output.push_str(description),
            None => output.push_str(&link_target_text(inner)),
        }
        rest = &after[end + 2..];
    }
    output.push_str(rest);
    output
}

/// Notes and state changes recorded in the LOGBOOK.
fn logged_entries(text: &str) -> usize {
    let mut in_logbook = false;
    let mut count = 0;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case(":LOGBOOK:") {
            in_logbook = true;
        } else if trimmed.eq_ignore_ascii_case(":END:") {
            in_logbook = false;
        } else if in_logbook && trimmed.starts_with("- ") {
            count += 1;
        }
    }
    count
}

pub(super) fn draw(frame: &mut Frame<'_>, app: &App, area: Rect, overlay: bool) {
    let theme = &app.theme;
    if overlay {
        frame.render_widget(Clear, area);
    }
    let block = if overlay {
        theme.popup("Peek")
    } else {
        theme.panel("Peek")
    };
    // Everything is sized from inside the frame and its padding; the last
    // row is the footer's.
    let inner = block.inner(area);
    let room = inner.height.saturating_sub(1) as usize;
    // On the Agents tab peek shows the selected change's diff, then the
    // task it touched.
    let (diff, subject) = if app.screen == Screen::Review {
        let record = app.journals.get(app.selected);
        let subject = record
            .and_then(|record| super::review::subject(app, record))
            .map(|item| (item, None));
        // A capture's diff is the whole new entry, which the task below
        // shows; it is drawn only when no heading holds the entry now.
        let whole_entry = record
            .is_some_and(|record| record.operation == Some(JournalOperation::Capture))
            && subject.is_some();
        let diff = match record {
            Some(record) if !whole_entry => diff_lines(app, &record.diff),
            _ => Vec::new(),
        };
        (diff, subject)
    } else {
        (
            Vec::new(),
            app.selected_row()
                .map(|row| (row.item.clone(), row.occurrence.clone())),
        )
    };
    let Some((item, occurrence)) = subject else {
        let mut lines = diff;
        let reason = if lines.is_empty() {
            "Nothing to peek"
        } else {
            "No heading holds this change now."
        };
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        lines.push(Line::from(Span::styled(reason, theme.muted())));
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(block),
            area,
        );
        return;
    };
    let item = &item;
    let text = entry_text(&app.index, item);
    let label = |name: &str| Span::styled(format!("{name:<11}"), theme.muted());
    let mut lines = diff;
    if !lines.is_empty() {
        lines.push(Line::default());
    }
    lines.extend([
        Line::from(Span::styled(display_title(item), theme.strong(theme.text))),
        Line::from(Span::styled(
            format!(
                "{} › {}",
                app.short_location(item)
                    .split(':')
                    .next()
                    .unwrap_or_default(),
                item.olpath[..item.olpath.len().saturating_sub(1)].join(" › ")
            ),
            theme.muted(),
        )),
        Line::default(),
    ]);
    if let Some(todo) = &item.todo {
        lines.push(Line::from(vec![label("State"), Span::raw(todo.clone())]));
    }
    for (name, timestamp) in [
        ("Scheduled", &item.planning.scheduled),
        ("Deadline", &item.planning.deadline),
    ] {
        let Some(timestamp) = timestamp else {
            continue;
        };
        let mut spans = vec![label(name), Span::raw(when(timestamp, app.today))];
        if let Some(repeater) = &timestamp.repeater
            && let Ok(next) = next_repeat(timestamp, app.today)
        {
            spans.push(Span::styled(
                format!(
                    "  ↻ {repeater} · next {}",
                    crate::words::day(next, app.today)
                ),
                theme.muted(),
            ));
        }
        lines.push(Line::from(spans));
    }
    // An entry's own plain timestamp says when it is, as the dates above.
    let (whens, body): (Vec<String>, Vec<String>) = body_lines(&text)
        .into_iter()
        .partition(|line| plain_timestamp(line).is_some());
    for line in &whens {
        if let Some((date, time)) = plain_timestamp(line) {
            lines.push(Line::from(vec![
                label("When"),
                Span::raw(crate::words::when(date, time, app.today)),
            ]));
        }
    }
    if let Ok((_, headline)) = app.index.resolve(&item.item_ref)
        && let Some(effort) = headline
            .properties
            .iter()
            .find(|property| property.key.eq_ignore_ascii_case("Effort"))
    {
        lines.push(Line::from(vec![
            label("Effort"),
            Span::raw(effort.value.clone()),
        ]));
    }
    if let Some(priority) = &item.priority {
        let mut spans = vec![label("Priority")];
        spans.extend(theme.pill(priority.clone(), theme.due));
        lines.push(Line::from(spans));
    }
    // The heading's own tags, then those it inherits, muted.
    if !item.tags.is_empty() || !item.inherited_tags.is_empty() {
        let mut spans = vec![label("Tags")];
        if !item.tags.is_empty() {
            spans.push(Span::styled(
                format!("{} ", crate::words::tags(app.settings.notation, &item.tags)),
                theme.fg(theme.text),
            ));
        }
        if !item.inherited_tags.is_empty() {
            spans.push(Span::styled(
                crate::words::tags(app.settings.notation, &item.inherited_tags),
                theme.muted(),
            ));
        }
        lines.push(Line::from(spans));
    }
    let logged = logged_entries(&text);
    if logged > 0 {
        lines.push(Line::from(vec![
            label("Logged"),
            Span::raw(crate::words::counted(logged, "change", "changes")),
        ]));
    }
    if let Some(reason) = occurrence.as_ref().and_then(why_listed) {
        lines.push(Line::from(vec![
            label("Why here"),
            Span::styled(reason, theme.muted()),
        ]));
    }
    let links: Vec<(String, String)> = links(&text).into_iter().take(3).collect();
    if !body.is_empty() {
        lines.push(Line::from(Span::styled(
            "─".repeat(inner.width as usize),
            theme.fg(theme.border),
        )));
        let shown = room.saturating_sub(lines.len() + links.len());
        for line in body.iter().take(shown) {
            // A checked box reads as a done row does.
            let checked = line
                .split_once(' ')
                .is_some_and(|(_, rest)| rest.starts_with("[X]"));
            lines.push(if checked {
                Line::from(Span::styled(
                    line.clone(),
                    theme.muted().add_modifier(Modifier::CROSSED_OUT),
                ))
            } else {
                Line::from(line.clone())
            });
        }
    }
    for (target, description) in links {
        let shown = if description == target {
            link_target_text(&target)
        } else {
            description
        };
        lines.push(Line::from(Span::styled(
            format!("{shown} ↗"),
            theme.fg(theme.accent),
        )));
    }
    // The keys of the screen underneath.
    let (groups, actions) = if app.screen == Screen::Review {
        (
            keys::AGENT_KEYS,
            [Action::Edit, Action::Editor, Action::Accept],
        )
    } else {
        (
            keys::LIST_KEYS,
            [Action::Edit, Action::Editor, Action::CopyLink],
        )
    };
    let footer = Line::from(
        keys::hints(groups, &actions)
            .into_iter()
            .flat_map(|(key, action)| theme.key_hint(key, action))
            .collect::<Vec<_>>(),
    );
    // The footer has its own row, so wrapped text never pushes it out.
    frame.render_widget(block, area);
    let body_area = Rect {
        height: inner.height.saturating_sub(1),
        ..inner
    };
    let footer_area = Rect {
        y: inner.y + body_area.height,
        height: inner.height.min(1),
        ..inner
    };
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body_area);
    frame.render_widget(Paragraph::new(footer), footer_area);
}

/// A change's added and removed lines, as the edit popup previews one:
/// added in the done colour, removed in the overdue colour.
fn diff_lines(app: &App, diff: &str) -> Vec<Line<'static>> {
    let theme = &app.theme;
    super::edit::changed_lines(diff)
        .lines()
        .map(|line| {
            let color = if line.starts_with('+') {
                theme.done
            } else {
                theme.overdue
            };
            Line::from(Span::styled(line.to_string(), theme.fg(color)))
        })
        .collect()
}

/// Under 100 columns peek is a centred overlay instead of a split, over a
/// dimmed screen like every other popup.
pub(super) fn draw_overlay(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let screen = frame.area();
    frame
        .buffer_mut()
        .set_style(screen, Style::default().add_modifier(Modifier::DIM));
    let popup = centered(area, 72, area.height.saturating_sub(4).max(10));
    draw(frame, app, popup, true);
}

/// `<2026-10-16 Fri 16:30>` alone on a line: its date and time. Ranges and
/// timestamps inside text stay text.
fn plain_timestamp(line: &str) -> Option<(&str, Option<&str>)> {
    let inner = line.trim().strip_prefix('<')?.strip_suffix('>')?;
    if inner.contains('>') {
        return None;
    }
    let mut parts = inner.split_whitespace();
    let date = parts.next()?;
    date.parse::<chrono::NaiveDate>().ok()?;
    let _weekday = parts.next();
    let time = parts.next().filter(|part| part.contains(':'));
    Some((date, time))
}

/// Which agenda rule put the row here, in org-agenda's own terms.
fn why_listed(occurrence: &Occurrence) -> Option<String> {
    let extra = &occurrence.extra;
    Some(match occurrence.kind {
        OccurrenceType::PastScheduled => format!("scheduled earlier, still open ({extra})"),
        OccurrenceType::Scheduled => "scheduled for this day".to_string(),
        OccurrenceType::Deadline if occurrence.late => format!("deadline passed ({extra})"),
        OccurrenceType::Deadline => "deadline is this day".to_string(),
        OccurrenceType::UpcomingDeadline => {
            format!("deadline warning period ({})", extra.trim_end_matches(':'))
        }
        OccurrenceType::Timestamp => "active timestamp on this day".to_string(),
        OccurrenceType::Block => format!("date range {extra}"),
        OccurrenceType::Closed => "closed this day (log mode)".to_string(),
        OccurrenceType::Todo => return None,
    })
}
