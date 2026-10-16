//! Triage: one card at a time under a progress gauge, one key per
//! decision on the rows of keys below it.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{LineGauge, Padding, Paragraph, Wrap};

use agd_core::OccurrenceType;

use super::actions::{DONE, NEXT_WEEK, REFILED, SOMEDAY, TODAY, TOMORROW, when};
use super::keys::{self, Action};
use super::rows::days_since;
use super::{App, DisplayRow, INBOX, peek, today};

/// Card rows: borders, the top padding, and heading, path, facts, body
/// line and nudge with blank rows between.
const CARD_HEIGHT: u16 = 11;
/// The card's widest; the gauge and keys line up with it.
const CARD_WIDTH: u16 = 84;

pub(super) fn draw(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = &app.theme;
    let column = centered_column(area);
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(CARD_HEIGHT),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(column);
    draw_progress(frame, app, parts[0]);
    let Some(row) = app.selected_row() else {
        frame.render_widget(
            Paragraph::new(summary(app))
                .wrap(Wrap { trim: true })
                .block(
                    theme
                        .panel("Triage done")
                        .border_style(theme.fg(theme.done))
                        .padding(Padding::new(2, 2, 1, 0)),
                ),
            parts[2],
        );
        return;
    };
    draw_card(frame, app, row, parts[2]);
    let rows = keys::triage_chips(row);
    // Both rows share one column width: the card's longest label.
    let label_width = rows
        .iter()
        .flat_map(|actions| keys::hints(keys::TRIAGE_KEYS, actions))
        .map(|(_, action)| action.chars().count())
        .max()
        .unwrap_or(0)
        + 3;
    for (actions, area) in rows.iter().zip([parts[4], parts[5]]) {
        frame.render_widget(Paragraph::new(key_row(app, actions, label_width)), area);
    }
}

/// The gauge of decided cards and, at its end, which card this is.
fn draw_progress(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = &app.theme;
    // The queue is fixed for the pass, so the counter only moves forward.
    let total = app.triage.queue.len();
    let decided = app.triage.decided.len();
    let position = (app.triage.position + 1).min(total);
    let counter = format!(" {position} / {total}");
    let parts = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(counter.chars().count() as u16),
        ])
        .split(area);
    frame.render_widget(
        LineGauge::default()
            .filled_symbol("━")
            .unfilled_symbol("─")
            .filled_style(theme.fg(theme.accent))
            .unfilled_style(theme.fg(theme.border))
            .ratio(if total == 0 {
                1.0
            } else {
                (decided as f64 / total as f64).min(1.0)
            })
            .label(""),
        parts[0],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(counter, theme.strong(theme.text))).alignment(Alignment::Right),
        parts[1],
    );
}

/// The card's column, centred: the gauge, card and keys share it.
fn centered_column(area: Rect) -> Rect {
    let width = area.width.min(CARD_WIDTH);
    Rect {
        x: area.x + (area.width - width) / 2,
        width,
        ..area
    }
}

/// One row of decision keys: each key in bold accent, then what it does,
/// in columns `label_width` wide.
fn key_row(app: &App, actions: &[Action], label_width: usize) -> Line<'static> {
    let theme = &app.theme;
    // Under the card's text: past its border and padding.
    let mut spans = vec![Span::raw("   ")];
    for (key, action) in keys::hints(keys::TRIAGE_KEYS, actions) {
        spans.push(Span::styled(
            format!("{key:<2}"),
            theme.strong(theme.accent),
        ));
        spans.push(Span::styled(
            format!(" {action:<label_width$}"),
            theme.fg(theme.text),
        ));
    }
    Line::from(spans)
}

fn draw_card(frame: &mut Frame<'_>, app: &App, row: &DisplayRow, area: Rect) {
    let theme = &app.theme;
    let item = &row.item;
    let inbox = row.section == INBOX;
    let title = if inbox {
        match app.provenance.get(&super::stable_ref(&item.item_ref)) {
            Some((actor, time)) => format!("Inbox · from {actor} {time}"),
            None => "Inbox".to_string(),
        }
    } else {
        format!("Overdue · {}d", overdue_days(row, app.today))
    };
    let color = if inbox { theme.accent } else { theme.overdue };
    let block = theme
        .panel(title)
        .border_style(theme.fg(color))
        .title_style(theme.strong(color))
        .padding(Padding::new(2, 2, 1, 0));
    let inner_width = block.inner(area).width as usize;

    let mut heading = Vec::new();
    if let Some(todo) = &item.todo {
        heading.push(Span::styled(
            format!("{todo} "),
            today::keyword_style(app, row),
        ));
    }
    heading.push(Span::styled(
        peek::display_title(item),
        theme.strong(theme.text),
    ));
    if let Some(priority) = &item.priority {
        heading.push(Span::raw(" "));
        heading.extend(theme.pill(priority.clone(), theme.due));
    }
    let tags = crate::words::tags(&item.tags);
    let used: usize = heading.iter().map(Span::width).sum();
    if !tags.is_empty() && used + tags.len() + 2 < inner_width {
        heading.push(Span::raw(" ".repeat(inner_width - used - tags.len())));
        heading.push(Span::styled(tags, theme.muted()));
    }
    let file = app
        .short_location(item)
        .split(':')
        .next()
        .unwrap_or_default()
        .to_string();
    let parents = &item.olpath[..item.olpath.len().saturating_sub(1)];
    let mut lines = vec![
        Line::from(heading),
        Line::from(Span::styled(
            std::iter::once(file)
                .chain(parents.iter().cloned())
                .collect::<Vec<_>>()
                .join(" › "),
            theme.muted(),
        )),
    ];

    let reschedules = app
        .reschedules
        .get(&super::stable_ref(&item.item_ref))
        .copied()
        .unwrap_or(0);
    let nudge = reschedules >= app.settings.reschedule_nudge;
    // Facts the task has, as chips; an absent one gets none.
    let mut facts = Vec::new();
    if let Some(scheduled) = &item.planning.scheduled {
        facts.push(theme.pill(
            format!("scheduled {}", when(scheduled, app.today)),
            theme.muted,
        ));
    }
    if reschedules > 0 {
        facts.push(theme.pill(
            format!("rescheduled {reschedules}x"),
            if nudge { theme.overdue } else { theme.muted },
        ));
    }
    if let Some(deadline) = &item.planning.deadline {
        facts.push(theme.pill(format!("deadline {}", when(deadline, app.today)), theme.due));
    }
    if !facts.is_empty() {
        lines.push(Line::default());
        lines.push(Line::from(facts.join(&Span::raw("  "))));
    }
    let text = peek::entry_text(&app.index, item);
    if let Some(first) = text.lines().map(str::trim).find(|line| {
        !line.is_empty()
            && !line.starts_with(':')
            && !line.contains("SCHEDULED:")
            && !line.contains("DEADLINE:")
            && !line.starts_with("- ")
    }) {
        lines.push(Line::default());
        lines.push(Line::from(first.to_string()));
    }
    // The nudge is earned: after enough reschedules, suggest Someday.
    if nudge {
        let key = keys::hints(keys::TRIAGE_KEYS, &[Action::Someday])
            .first()
            .map_or("S", |(key, _)| *key);
        lines.push(Line::default());
        lines.push(Line::from(vec![
            Span::styled("↳ Someday?  ", theme.fg(theme.accent)),
            Span::styled(key, theme.strong(theme.accent)),
        ]));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

/// Days the item has been overdue: from org's `Sched.12x:` or `3 d. ago:`
/// leader, else from its SCHEDULED date.
fn overdue_days(row: &DisplayRow, today: chrono::NaiveDate) -> i64 {
    if let Some(occurrence) = &row.occurrence {
        let digits: String = occurrence
            .extra
            .chars()
            .filter(char::is_ascii_digit)
            .collect();
        if matches!(
            occurrence.kind,
            OccurrenceType::PastScheduled | OccurrenceType::Deadline
        ) && let Ok(days) = digits.parse()
        {
            return days;
        }
    }
    row.item
        .planning
        .scheduled
        .as_ref()
        .and_then(|scheduled| days_since(&scheduled.date, today))
        .unwrap_or(0)
}

/// "Triage done, 7 items in 1m 40s: 2 today, 1 tomorrow, …"
fn summary(app: &App) -> Vec<Line<'static>> {
    let theme = &app.theme;
    let outcomes = &app.triage.outcomes;
    let decided = app.triage.decided.len();
    // From the start to the last decision, so the time does not keep
    // running while the summary is on screen.
    let elapsed = match (app.triage.started, app.triage.last_decision) {
        (Some(started), Some(last)) => last.duration_since(started).as_secs(),
        _ => 0,
    };
    let mut parts = Vec::new();
    for (index, label) in [
        (TODAY, "today"),
        (TOMORROW, "tomorrow"),
        (NEXT_WEEK, "next week"),
        (SOMEDAY, "someday"),
        (DONE, "done"),
        (REFILED, "refiled"),
    ] {
        if outcomes[index] > 0 {
            parts.push(format!("{} {label}", outcomes[index]));
        }
    }
    let headline = if app.triage.queue.is_empty() {
        Line::from(vec![
            Span::styled("Nothing to triage", theme.strong(theme.done)),
            Span::raw(": no overdue items, inbox empty."),
        ])
    } else {
        let lead = if app.counts.inbox == 0 {
            "Inbox zero"
        } else {
            "Triage done"
        };
        Line::from(vec![
            Span::styled(lead, theme.strong(theme.done)),
            Span::raw(format!(
                " · {decided} decided in {}m {}s{}{}",
                elapsed / 60,
                elapsed % 60,
                if parts.is_empty() { "" } else { ": " },
                parts.join(", ")
            )),
        ])
    };
    // Undo is offered only when the pass wrote something it can undo.
    let key = |action: Action| {
        keys::hints(keys::TRIAGE_KEYS, &[action])
            .first()
            .map_or("", |(key, _)| *key)
    };
    let mut hints = Vec::new();
    if !app.triage.undo.is_empty() {
        hints.extend(theme.key_hint(key(Action::Undo), "undoes all"));
    }
    if !app.triage.queue.is_empty() {
        hints.extend(theme.key_hint(key(Action::Previous), "last card"));
    }
    hints.extend(theme.key_hint(key(Action::Back), "goes back"));
    vec![headline, Line::default(), Line::from(hints)]
}
