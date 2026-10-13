use super::theme::{accent, centered_fixed, muted, no_color, rounded, surface};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

pub(super) fn help_heading(title: &'static str) -> Line<'static> {
    Line::from(Span::styled(
        title,
        Style::default().fg(accent()).add_modifier(Modifier::BOLD),
    ))
}

pub(super) fn help_shortcut(key: &'static str, description: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!(" {key:<9}"),
            if no_color() {
                Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
            } else {
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Rgb(30, 41, 59))
                    .add_modifier(Modifier::BOLD)
            },
        ),
        Span::raw("  "),
        Span::styled(description, Style::default().fg(Color::White)),
    ])
}

pub(super) fn draw_help(frame: &mut ratatui::Frame<'_>, area: Rect) {
    let popup = centered_fixed(area, 92, 22);
    frame.render_widget(Clear, popup);
    let panel = rounded("Keyboard shortcuts")
        .style(Style::default().bg(surface()).fg(Color::White))
        .border_style(Style::default().fg(accent()));
    let inner = panel.inner(popup);
    frame.render_widget(panel, popup);
    let content = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(12),
            Constraint::Length(1),
        ])
        .margin(1)
        .split(inner);
    frame.render_widget(
        Paragraph::new("Move quickly, edit safely, and keep context.")
            .style(Style::default().fg(muted())),
        content[0],
    );

    let navigation = vec![
        help_heading("NAVIGATE"),
        help_shortcut("j / k", "Move selection"),
        help_shortcut("enter / e", "Open in editor"),
        help_shortcut("f / b", "Next / previous period"),
        help_shortcut(".", "Return to today"),
        help_shortcut("/", "Filter visible tasks"),
        Line::default(),
        help_heading("VIEWS"),
        help_shortcut("V d/w/t", "Day, week, or todo"),
        help_shortcut("<", "Change context"),
        help_shortcut("p", "Toggle details"),
        help_shortcut("v", "Toggle classic view"),
    ];
    let actions = vec![
        help_heading("EDIT"),
        help_shortcut("t", "Cycle TODO state"),
        help_shortcut("x", "Mark complete"),
        help_shortcut("s / d", "Schedule / deadline"),
        help_shortcut(", / :", "Priority / tags"),
        help_shortcut("space", "Mark for bulk edit"),
        help_shortcut("u", "Undo latest change"),
        Line::default(),
        help_heading("WORKFLOWS"),
        help_shortcut("c", "Capture a task"),
        help_shortcut("T", "Start triage"),
        help_shortcut("A", "Review agent changes"),
        help_shortcut("q", "Quit"),
    ];
    if content[1].width >= 70 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(50),
                Constraint::Length(2),
                Constraint::Percentage(50),
            ])
            .split(content[1]);
        frame.render_widget(Paragraph::new(navigation), columns[0]);
        frame.render_widget(Paragraph::new(actions), columns[2]);
    } else {
        let compact = vec![
            help_heading("NAVIGATE"),
            help_shortcut("j/k", "Move"),
            help_shortcut("enter/e", "Open in editor"),
            help_shortcut("f/b", "Change period"),
            help_shortcut("V d/w/t", "Change view"),
            help_shortcut("/", "Filter"),
            Line::default(),
            help_heading("EDIT & WORKFLOWS"),
            help_shortcut("t / x", "Cycle / complete"),
            help_shortcut("s / d", "Schedule / deadline"),
            help_shortcut("space", "Mark"),
            help_shortcut("u", "Undo"),
            help_shortcut("c / T / A", "Capture / triage / review"),
        ];
        frame.render_widget(Paragraph::new(compact), content[1]);
    }
    frame.render_widget(
        Paragraph::new("Any key closes this panel")
            .style(Style::default().fg(muted()))
            .alignment(ratatui::layout::Alignment::Center),
        content[2],
    );
}
