//! The narrow pane (`agd --pane`, or any terminal under 60 columns): today
//! at a glance beside an agent or an editor, drawn with Today's marker and
//! lead cells so the two read alike.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};

use super::theme::truncate;
use super::today::{self, Clock};
use super::{App, DisplayRow, FULL_WIDTH};

/// The 2-second glance for a herdr or tmux split: overdue collapses to a
/// count, timed items sort by clock time with ▸ on the next one, done items
/// stay dimmed, and the footer carries the only cross-screen signals. Rows
/// use Today's marker and lead cells, so the two read alike.
pub(super) fn draw(frame: &mut Frame<'_>, app: &mut App, area: Rect) {
    let theme = app.theme.clone();
    let block = theme.panel("agd");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    // A status message too long for one row wraps onto a second.
    let status_width = app
        .status
        .as_ref()
        .map_or(0, |status| Span::raw(status.text.as_str()).width());
    let footer_rows = if status_width + 2 > inner.width as usize {
        2
    } else {
        1
    };
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(footer_rows),
        ])
        .split(inner);

    let mut top = Vec::new();
    if app.counts.overdue > 0 {
        top.extend(theme.pill(format!("{} overdue", app.counts.overdue), theme.overdue));
        // Triage needs the full view, which needs the columns.
        if area.width >= FULL_WIDTH {
            top.push(Span::raw("  "));
            top.extend(theme.key_hint("T", "triage"));
        }
    }
    frame.render_widget(Paragraph::new(Line::from(top)), parts[0]);
    frame.render_widget(
        Paragraph::new(Span::styled(
            app.clock.format("%H:%M").to_string(),
            theme.muted(),
        ))
        .alignment(ratatui::layout::Alignment::Right),
        parts[0],
    );
    // One blank line above the section, as on Today.
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("TODAY", theme.strong(theme.accent)),
            Span::styled(
                format!("  {}", app.rows.iter().filter(|row| !row.item.done).count()),
                theme.muted(),
            ),
        ])),
        parts[2],
    );

    let list_area = parts[3];
    // The highlight column takes one cell.
    let width = list_area.width.saturating_sub(1) as usize;
    let next = today::next_timed(app);
    let items: Vec<ListItem<'_>> = if app.rows.is_empty() {
        // The inbox count is already in the footer.
        let lines = if app.index.documents().next().is_none() {
            vec!["No .org files yet.", "c captures the first task."]
        } else {
            vec!["All clear for today."]
        };
        vec![ListItem::new(
            lines
                .into_iter()
                .map(|line| Line::from(Span::styled(line, theme.muted())))
                .collect::<Vec<_>>(),
        )]
    } else {
        (0..app.rows.len())
            .map(|index| {
                let clock = today::clock_of(app, index, next);
                ListItem::new(pane_line(app, &app.rows[index], clock, width))
            })
            .collect()
    };
    let mut state =
        ListState::default().with_selected((!app.rows.is_empty()).then_some(app.selected));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol(Span::styled("▌", theme.fg(theme.accent)))
            .highlight_spacing(ratatui::widgets::HighlightSpacing::Always)
            .highlight_style(theme.selected()),
        list_area,
        &mut state,
    );
    app.list_area = list_area;
    app.click_rows = (0..app.rows.len())
        .skip(state.offset())
        .take(list_area.height as usize)
        .enumerate()
        .map(|(line, row)| (list_area.y + line as u16, row))
        .collect();

    draw_footer(frame, app, parts[4]);
}

/// The last write's confirmation or a refusal while it lasts, else the
/// inbox and agent counts named as the tabs are; `?` at the right.
fn draw_footer(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = &app.theme;
    let line = match &app.status {
        Some(status) => Line::from(Span::styled(
            status.text.clone(),
            today::status_style(app, status),
        )),
        None => {
            let mut spans = vec![Span::styled(
                format!("Inbox {}  Agents", app.counts.inbox),
                theme.muted(),
            )];
            if app.counts.agents > 0 {
                spans.push(Span::styled(
                    format!(" ● {}", app.counts.agents),
                    theme.strong(theme.accent),
                ));
            }
            Line::from(spans)
        }
    };
    let help = Line::from(vec![
        Span::raw(" "),
        Span::styled("?", theme.strong(theme.accent)),
    ]);
    let room = area.width.saturating_sub(help.width() as u16);
    frame.render_widget(
        Paragraph::new(line).wrap(ratatui::widgets::Wrap { trim: true }),
        Rect {
            width: room,
            ..area
        },
    );
    frame.render_widget(
        Paragraph::new(help).alignment(ratatui::layout::Alignment::Right),
        Rect { height: 1, ..area },
    );
}

/// Today's marker and lead cells, the title, and a right-hand mark: the
/// priority pill, ↻ for a repeating task, ✓ for done (the pane shows no
/// keyword, so ✓ says it).
fn pane_line(app: &App, row: &DisplayRow, clock: Clock, width: usize) -> Line<'static> {
    let theme = &app.theme;
    let item = &row.item;
    let repeats = [&item.planning.scheduled, &item.planning.deadline]
        .into_iter()
        .flatten()
        .any(|timestamp| timestamp.repeater.is_some());
    // Priority is the pill, as on every row.
    let right = if item.done {
        vec![Span::styled("✓", theme.strong(theme.done))]
    } else if let Some(priority) = &item.priority {
        theme.pill(priority.clone(), theme.due)
    } else if repeats {
        vec![Span::styled("↻", theme.muted())]
    } else {
        Vec::new()
    };
    let right_width: usize = right.iter().map(Span::width).sum();
    let title_style = if item.done {
        theme.done_row()
    } else if clock == Clock::Next {
        theme.strong(theme.text)
    } else {
        theme.fg(theme.text)
    };
    let mut spans = vec![today::marker(app, row, clock)];
    spans.extend(today::lead_cell(app, row, clock));
    spans.push(Span::raw(" "));
    let used: usize = spans.iter().map(Span::width).sum();
    // Display width, not characters: a CJK or emoji title takes two
    // columns a character. A right-hand mark keeps a space before it.
    let gap = usize::from(right_width > 0);
    let title = truncate(
        &super::peek::display_title(item),
        width.saturating_sub(used + right_width + gap),
    );
    let title_width = Span::raw(title.as_str()).width();
    spans.push(Span::styled(title, title_style));
    spans.push(Span::raw(
        " ".repeat(width.saturating_sub(used + title_width + right_width)),
    ));
    spans.extend(right);
    Line::from(spans)
}
