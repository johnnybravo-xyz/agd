use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};

use agd_core::{Item, OccurrenceType, WorkspaceIndex};

use super::actions::short_date;
use super::keys::{self, Action};
use super::rows::next_repeat;
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
        output.push_str(
            inner
                .split_once("][")
                .map_or(inner, |(_, description)| description),
        );
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
    // On the Agents tab the subject is the task the selected change touched.
    let subject = if app.screen == Screen::Review {
        app.journals
            .get(app.selected)
            .and_then(|record| super::review::touched_item(app, record))
            .map(|item| (item, None))
    } else {
        app.selected_row()
            .map(|row| (row.item.clone(), row.occurrence.clone()))
    };
    let Some((item, occurrence)) = subject else {
        frame.render_widget(
            Paragraph::new(Span::styled("Nothing to peek", theme.muted())).block(block),
            area,
        );
        return;
    };
    let item = &item;
    let text = entry_text(&app.index, item);
    let label = |name: &str| Span::styled(format!("{name:<11}"), theme.muted());
    let mut lines = vec![
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
    ];
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
        let mut spans = vec![
            label(name),
            Span::raw(short_date(timestamp)),
            Span::raw(
                timestamp
                    .time
                    .as_ref()
                    .map(|time| format!(" {time}"))
                    .unwrap_or_default(),
            ),
        ];
        if let Some(repeater) = &timestamp.repeater
            && let Some(next) = next_repeat(&timestamp.date, repeater, app.today)
        {
            spans.push(Span::styled(
                format!("  ↻ {repeater} · next {}", next.format("%-d %b")),
                theme.muted(),
            ));
        }
        lines.push(Line::from(spans));
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
        lines.push(Line::from(vec![
            label("Priority"),
            Span::styled(priority.clone(), theme.strong(theme.due)),
        ]));
    }
    if !item.tags.is_empty() || !item.inherited_tags.is_empty() {
        let mut spans = vec![label("Tags")];
        for tag in &item.tags {
            spans.push(Span::styled(format!("{tag} "), theme.muted()));
        }
        for tag in &item.inherited_tags {
            spans.push(Span::styled(format!("{tag}↑ "), theme.muted()));
        }
        lines.push(Line::from(spans));
    }
    let logged = logged_entries(&text);
    if logged > 0 {
        lines.push(Line::from(vec![
            label("Logged"),
            Span::raw(format!(
                "{logged} change{}",
                if logged == 1 { "" } else { "s" }
            )),
        ]));
    }
    if let Some(reason) = why_listed(
        occurrence
            .as_ref()
            .map(|occurrence| (occurrence.kind, occurrence.extra.as_str())),
    ) {
        lines.push(Line::from(vec![
            label("Why here"),
            Span::styled(reason, theme.muted()),
        ]));
    }
    let body = body_lines(&text);
    if !body.is_empty() {
        lines.push(Line::from(Span::styled(
            "─".repeat(block.inner(area).width as usize),
            theme.fg(theme.border),
        )));
        for line in body
            .iter()
            .take(area.height.saturating_sub(lines.len() as u16 + 4) as usize)
        {
            lines.push(Line::from(line.clone()));
        }
    }
    for (target, description) in links(&text).into_iter().take(3) {
        let shown = if description == target {
            target
        } else {
            description
        };
        lines.push(Line::from(Span::styled(
            format!("{shown} ↗"),
            theme.fg(theme.accent),
        )));
    }
    // The keys of the screen underneath; on the Agents tab ⏎ opens the
    // changed heading rather than the edit popup.
    let (groups, actions) = if app.screen == Screen::Review {
        (
            keys::AGENT_KEYS,
            [Action::Editor, Action::Accept, Action::Undo],
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
    let inner = block.inner(area);
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

/// Under 100 columns peek is a centred overlay instead of a split.
pub(super) fn draw_overlay(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let popup = centered(area, 72, area.height.saturating_sub(4).max(10));
    draw(frame, app, popup, true);
}

/// Which agenda rule put the row here, in org-agenda's own terms.
fn why_listed(occurrence: Option<(OccurrenceType, &str)>) -> Option<String> {
    let (kind, extra) = occurrence?;
    Some(match kind {
        OccurrenceType::PastScheduled => format!("scheduled earlier, still open ({extra})"),
        OccurrenceType::Scheduled => "scheduled for this day".to_string(),
        OccurrenceType::Deadline if extra.ends_with("ago:") => format!("deadline passed ({extra})"),
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
