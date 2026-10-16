use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{LineGauge, Paragraph, Wrap};

use agd_core::OccurrenceType;

use super::actions::{DONE, NEXT_WEEK, REFILED, SOMEDAY, TODAY, TOMORROW, short_date};
use super::rows::days_since;
use super::{App, DisplayRow, INBOX, keys, peek};

pub(super) fn draw(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = &app.theme;
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(CARD_HEIGHT + 1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(area);
    // The queue is fixed for the pass, so the counter only moves forward.
    let total = app.triage.queue.len();
    let decided = app.triage.decided.len();
    let position = (app.triage.position + 1).min(total);
    frame.render_widget(
        Paragraph::new(Span::styled(
            format!("{position} / {total} "),
            theme.strong(theme.text),
        ))
        .alignment(ratatui::layout::Alignment::Right),
        parts[0],
    );
    frame.render_widget(
        LineGauge::default()
            .filled_style(theme.fg(theme.accent))
            .unfilled_style(theme.fg(theme.border))
            .ratio(if total == 0 {
                1.0
            } else {
                (decided as f64 / total as f64).min(1.0)
            })
            .label(""),
        parts[1],
    );
    let card_area = centered_card(parts[2]);
    let Some(row) = app.selected_row() else {
        frame.render_widget(
            Paragraph::new(summary(app))
                .wrap(Wrap { trim: true })
                .block(
                    theme
                        .panel("Triage done")
                        .border_style(theme.fg(theme.done)),
                ),
            card_area,
        );
        return;
    };
    draw_card(frame, app, row, card_area);
    let indent = card_area.x.saturating_sub(area.x) as usize;
    let chips = |actions: &[keys::Action]| {
        let mut spans = vec![Span::raw(" ".repeat(indent))];
        for (key, action) in keys::hints(keys::TRIAGE_KEYS, actions) {
            spans.extend(theme.pill(key.to_string(), theme.accent));
            spans.push(Span::styled(format!(" {action:<12}"), theme.fg(theme.text)));
        }
        Line::from(spans)
    };
    for (actions, area) in keys::triage_chips(row).iter().zip([parts[3], parts[4]]) {
        frame.render_widget(Paragraph::new(chips(actions)), area);
    }
}

/// Card rows: heading, path, chips, body line and the nudge, with spacing.
const CARD_HEIGHT: u16 = 11;

fn centered_card(area: Rect) -> Rect {
    let width = area.width.min(84);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + 1,
        width,
        area.height.saturating_sub(1).min(CARD_HEIGHT),
    )
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
        format!("Overdue · {} days", overdue_days(row, app.today))
    };
    let color = if inbox { theme.accent } else { theme.overdue };
    let block = theme
        .panel(title)
        .border_style(theme.fg(color))
        .title_style(theme.strong(color))
        .padding(ratatui::widgets::Padding::horizontal(1));
    let inner_width = block.inner(area).width as usize;

    let mut heading = Vec::new();
    if let Some(todo) = &item.todo {
        heading.push(Span::styled(format!("{todo} "), theme.strong(theme.due)));
    }
    heading.push(Span::styled(
        peek::display_title(item),
        theme.strong(theme.text),
    ));
    if let Some(priority) = &item.priority {
        heading.push(Span::raw(" "));
        heading.extend(theme.pill(priority.clone(), theme.due));
    }
    let tags = item
        .tags
        .iter()
        .map(|tag| format!("#{tag}"))
        .collect::<Vec<_>>()
        .join(" ");
    let used: usize = heading.iter().map(Span::width).sum();
    if !tags.is_empty() && used + tags.len() + 2 < inner_width {
        heading.push(Span::raw(" ".repeat(inner_width - used - tags.len() - 1)));
        heading.push(Span::styled(tags, theme.fg(theme.accent)));
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
        Line::default(),
    ];

    let reschedules = app
        .reschedules
        .get(&super::stable_ref(&item.item_ref))
        .copied()
        .unwrap_or(0);
    let nudge = reschedules >= app.settings.reschedule_nudge;
    let mut chips = theme.pill(
        item.planning
            .scheduled
            .as_ref()
            .map_or("not scheduled".to_string(), |scheduled| {
                format!("scheduled {}", short_date(scheduled))
            }),
        theme.muted,
    );
    chips.push(Span::raw("  "));
    if reschedules > 0 {
        chips.extend(theme.pill(
            format!("rescheduled {reschedules}x"),
            if nudge { theme.overdue } else { theme.muted },
        ));
        chips.push(Span::raw("  "));
    }
    chips.extend(
        theme.pill(
            item.planning
                .deadline
                .as_ref()
                .map_or("no deadline".to_string(), |deadline| {
                    format!("deadline {}", short_date(deadline))
                }),
            if item.planning.deadline.is_some() {
                theme.due
            } else {
                theme.muted
            },
        ),
    );
    lines.push(Line::from(chips));
    lines.push(Line::default());
    let text = peek::entry_text(&app.index, item);
    if let Some(first) = text.lines().map(str::trim).find(|line| {
        !line.is_empty()
            && !line.starts_with(':')
            && !line.contains("SCHEDULED:")
            && !line.contains("DEADLINE:")
            && !line.starts_with("- ")
    }) {
        lines.push(Line::from(first.to_string()));
        lines.push(Line::default());
    }
    // The nudge is earned: after enough reschedules, suggest Someday.
    if nudge {
        lines.push(Line::from(
            vec![Span::styled(
                format!(" ↳ Rescheduled {reschedules} times. Someday? "),
                theme.fg(theme.accent),
            )]
            .into_iter()
            .chain(theme.pill("s", theme.accent))
            .collect::<Vec<_>>(),
        ));
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
    // `u` is offered only when the pass wrote something it can undo.
    let mut keys = Vec::new();
    if !app.triage.undo.is_empty() {
        keys.extend(theme.key_hint("u", "undoes all"));
    }
    if !app.triage.queue.is_empty() {
        keys.extend(theme.key_hint("k", "last card"));
    }
    keys.extend(theme.key_hint("⏎", "returns to Today"));
    vec![headline, Line::default(), Line::from(keys)]
}
