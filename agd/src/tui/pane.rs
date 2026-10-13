use chrono::Local;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};

use super::{App, DisplayRow};

/// The 2-second glance for a herdr or tmux split: overdue collapses to a
/// count, timed items sort by clock time with ▸ on the next one, done items
/// stay dimmed, and the footer carries the only cross-screen signals.
pub(super) fn draw(frame: &mut Frame<'_>, app: &mut App, area: Rect) {
    let theme = app.theme.clone();
    let block = theme.panel("agd").border_style(theme.fg(theme.accent));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let mut top = Vec::new();
    if app.counts.slipped > 0 {
        top.push(theme.pill(format!("{} slipped", app.counts.slipped), theme.slipped));
        top.push(Span::raw("  "));
        top.extend(theme.key_hint("T", "triage"));
    }
    frame.render_widget(Paragraph::new(Line::from(top)), parts[0]);
    frame.render_widget(
        Paragraph::new(Span::styled(
            Local::now().format("%H:%M").to_string(),
            theme.muted(),
        ))
        .alignment(ratatui::layout::Alignment::Right),
        parts[0],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("TODAY ", theme.strong(theme.accent)),
            Span::styled(
                app.rows
                    .iter()
                    .filter(|row| !row.item.done)
                    .count()
                    .to_string(),
                theme.muted(),
            ),
        ])),
        parts[1],
    );

    let width = parts[2].width.saturating_sub(1) as usize;
    let now = Local::now().format("%H:%M").to_string();
    let next = app.rows.iter().position(|row| {
        !row.item.done
            && row
                .occurrence
                .as_ref()
                .and_then(|occurrence| occurrence.time.as_deref())
                .is_some_and(|time| time >= now.as_str())
    });
    let items: Vec<ListItem<'_>> = if app.rows.is_empty() {
        vec![ListItem::new(Span::styled(
            format!("Nothing scheduled today. {} in inbox.", app.counts.inbox),
            theme.muted(),
        ))]
    } else {
        app.rows
            .iter()
            .enumerate()
            .map(|(index, row)| ListItem::new(pane_line(app, row, Some(index) == next, width)))
            .collect()
    };
    let mut state =
        ListState::default().with_selected((!app.rows.is_empty()).then_some(app.selected));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol(Span::styled("▌", theme.fg(theme.accent)))
            .highlight_spacing(ratatui::widgets::HighlightSpacing::Always)
            .highlight_style(theme.selected()),
        parts[2],
        &mut state,
    );
    app.list_area = parts[2];
    app.click_rows = (0..app.rows.len())
        .skip(state.offset())
        .take(parts[2].height as usize)
        .enumerate()
        .map(|(line, row)| (parts[2].y + line as u16, row))
        .collect();

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            theme.pill(format!("inbox {}", app.counts.inbox), theme.muted),
            Span::raw(" "),
            theme.pill(
                format!("agents {}", app.counts.agents),
                if app.counts.agents > 0 {
                    theme.accent
                } else {
                    theme.muted
                },
            ),
        ])),
        parts[3],
    );
}

fn pane_line(app: &App, row: &DisplayRow, next: bool, width: usize) -> Line<'static> {
    let theme = &app.theme;
    let item = &row.item;
    let time = row
        .occurrence
        .as_ref()
        .and_then(|occurrence| occurrence.time.clone())
        .unwrap_or_default();
    let marker = if next { "▸" } else { " " };
    let right = if item.done {
        Span::styled("✓", theme.strong(theme.done))
    } else if let Some(priority) = &item.priority {
        Span::styled(priority.clone(), theme.strong(theme.due))
    } else if let Some(days) = row
        .occurrence
        .as_ref()
        .filter(|occurrence| occurrence.extra.starts_with("In"))
        .and_then(|occurrence| {
            occurrence
                .extra
                .split_whitespace()
                .nth(1)
                .map(str::to_string)
        })
    {
        Span::styled(format!("{days}d"), theme.fg(theme.due))
    } else if item
        .planning
        .scheduled
        .as_ref()
        .is_some_and(|scheduled| scheduled.repeater.is_some())
    {
        Span::styled("↻", theme.muted())
    } else {
        Span::raw("")
    };
    let title_style = if item.done {
        theme.done_row()
    } else if next {
        theme.strong(theme.text)
    } else {
        theme.fg(theme.text)
    };
    let time_style = if next {
        theme.strong(theme.accent)
    } else {
        theme.muted()
    };
    let mut title = super::peek::display_title(item);
    let available = width.saturating_sub(8 + right.width());
    if title.chars().count() > available {
        title = title
            .chars()
            .take(available.saturating_sub(1))
            .collect::<String>()
            + "…";
    }
    let used = 2 + 6 + title.chars().count();
    Line::from(vec![
        Span::styled(format!("{marker} "), theme.fg(theme.accent)),
        Span::styled(format!("{time:<6}"), time_style),
        Span::styled(title, title_style),
        Span::raw(" ".repeat(width.saturating_sub(used + right.width()))),
        right,
    ])
}
