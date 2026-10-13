use super::App;
use super::theme::{agent_color, centered, rounded, section_color, selection_style};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, ListState, Paragraph, Wrap};

pub(super) fn draw_review(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let items: Vec<ListItem<'_>> = app
        .journals
        .iter()
        .map(|record| {
            let agent = record.actor.strip_prefix("mcp:").unwrap_or(&record.actor);
            let time = record.timestamp.get(11..16).unwrap_or(&record.timestamp);
            let kind = if record.diff.lines().any(|line| line.starts_with("+* ")) {
                "captured"
            } else if record.diff.contains("TODO") || record.diff.contains("DONE") {
                "state"
            } else {
                "edited"
            };
            let mut lines = vec![Line::from(vec![
                Span::styled("● ", Style::default().fg(agent_color(agent))),
                Span::styled(
                    agent.to_string(),
                    Style::default()
                        .fg(agent_color(agent))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!("   {}   {time}   ", record.project)),
                Span::styled(kind, Style::default().fg(section_color("IN PROGRESS"))),
            ])];
            lines.extend(
                record
                    .diff
                    .lines()
                    .filter(|line| {
                        (line.starts_with('+') && !line.starts_with("+++"))
                            || (line.starts_with('-') && !line.starts_with("---"))
                    })
                    .take(2)
                    .map(|line| {
                        Line::from(Span::styled(
                            format!("  {line}"),
                            Style::default().fg(if line.starts_with('+') {
                                Color::Green
                            } else {
                                Color::Red
                            }),
                        ))
                    }),
            );
            ListItem::new(lines)
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(app.selected));
    let title = format!("{} unreviewed", app.journals.len());
    let list = List::new(items)
        .block(rounded(&title))
        .highlight_symbol("▌")
        .highlight_style(selection_style());
    frame.render_stateful_widget(list, area, &mut state);
    if app.review_diff
        && let Some(record) = app.journals.get(app.selected)
    {
        let popup = centered(area, 86, 70);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(record.diff.clone())
                .block(rounded("Raw diff · d close · a accept · u undo"))
                .wrap(Wrap { trim: false }),
            popup,
        );
    }
}
