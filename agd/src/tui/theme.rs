use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, BorderType, Borders};

pub(super) fn tab_style(selected: bool, color: Color) -> Style {
    if selected {
        if no_color() {
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            Style::default()
                .fg(color)
                .bg(Color::Rgb(20, 35, 48))
                .add_modifier(Modifier::BOLD)
        }
    } else {
        Style::default().fg(muted())
    }
}

pub(super) fn key_chip(key: &str) -> Span<'_> {
    Span::styled(
        format!(" {key} "),
        if no_color() {
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            Style::default()
                .fg(Color::White)
                .bg(Color::Rgb(30, 41, 59))
                .add_modifier(Modifier::BOLD)
        },
    )
}

pub(super) fn agent_color(agent: &str) -> Color {
    if no_color() {
        return Color::Reset;
    }
    const COLORS: [Color; 4] = [Color::Magenta, Color::Green, Color::Cyan, Color::Yellow];
    let index = agent
        .bytes()
        .fold(0usize, |value, byte| value.wrapping_add(byte as usize))
        % COLORS.len();
    COLORS[index]
}

pub(super) fn section_color(section: &str) -> Color {
    if no_color() {
        return Color::Reset;
    }
    match section {
        "SLIPPED" => Color::Red,
        "DUE SOON" => Color::Yellow,
        "IN PROGRESS" => Color::Green,
        _ => accent(),
    }
}

pub(super) fn accent() -> Color {
    if no_color() {
        return Color::Reset;
    }
    let truecolor = std::env::var("COLORTERM")
        .map(|value| value.contains("truecolor") || value.contains("24bit"))
        .unwrap_or(false);
    if !truecolor {
        return Color::Cyan;
    }
    let light_background = std::env::var("COLORFGBG")
        .ok()
        .and_then(|value| value.rsplit(';').next()?.parse::<u8>().ok())
        .is_some_and(|background| background >= 7);
    if light_background {
        Color::Rgb(0, 95, 115)
    } else {
        Color::Rgb(76, 201, 240)
    }
}

pub(super) fn muted() -> Color {
    if no_color() {
        Color::Reset
    } else {
        Color::Rgb(148, 163, 184)
    }
}

pub(super) fn surface() -> Color {
    if no_color() {
        Color::Reset
    } else {
        Color::Rgb(15, 23, 32)
    }
}

pub(super) fn selection_style() -> Style {
    if no_color() {
        Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        Style::default()
            .bg(Color::Rgb(20, 35, 48))
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    }
}

pub(super) fn no_color() -> bool {
    std::env::var_os("NO_COLOR").is_some()
}

pub(super) fn rounded(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(muted()))
        .title(Span::styled(
            format!(" {title} "),
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ))
}

pub(super) fn centered(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

pub(super) fn centered_fixed(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(2));
    let height = height.min(area.height.saturating_sub(2));
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}
