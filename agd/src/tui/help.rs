use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use super::App;
use super::theme::centered;

/// (key, action, org-agenda equivalent)
type Key = (&'static str, &'static str, &'static str);

/// Every key, with the org-agenda key it mirrors where there is one.
const SECTIONS: &[(&str, &[Key])] = &[
    (
        "MOVE",
        &[
            ("j / k", "move", "n / p"),
            ("tab · 1–4", "Today Week Inbox Agents", ""),
            ("f / b / .", "next, previous, today", "f / b / ."),
            ("⏎ / e", "edit title, state, dates", ""),
            ("ctrl-e", "open in $EDITOR", "RET"),
            ("p", "peek", "SPC"),
            ("a", "saved views", "C-c a"),
            ("v", "classic agenda layout", "v"),
            ("l", "log mode (closed items)", "l"),
            ("g", "refresh", "g"),
        ],
    ),
    (
        "CHANGE",
        &[
            ("t", "cycle TODO state", "t"),
            ("x", "mark done", ""),
            ("s / d", "schedule / deadline", "C-c C-s / C-c C-d"),
            (", + -", "priority", ", + -"),
            (":", "tags", ":"),
            ("r", "refile", "C-c C-w"),
            ("u", "undo last write", "C-_"),
            ("y", "copy link", ""),
        ],
    ),
    (
        "FLOW",
        &[
            ("c", "capture", "k"),
            ("T", "triage overdue + inbox", ""),
            ("A", "agent review", ""),
            ("space", "mark, then . m w s", "m · B"),
            ("<", "context filter", "<"),
            ("/", "filter by text or tag", "/"),
            ("esc", "back one level", ""),
            ("q", "quit", "q"),
        ],
    ),
];

pub(super) fn draw(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = &app.theme;
    let popup = centered(area, 150, 15);
    frame.render_widget(Clear, popup);
    let block = theme.panel("Keys").border_style(theme.fg(theme.accent));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    // Three columns when they fit; org-agenda equivalents only with room.
    let wide = inner.width >= 96;
    let column_width = if wide { inner.width / 3 } else { inner.width } as usize;
    let show_org = column_width >= 46;
    let action_width = column_width.saturating_sub(11 + if show_org { 12 } else { 0 });
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(if wide {
            vec![Constraint::Ratio(1, 3); 3]
        } else {
            vec![Constraint::Percentage(100)]
        })
        .split(inner);
    let mut texts: Vec<Vec<Line<'_>>> = vec![Vec::new(); columns.len()];
    for (index, (title, keys)) in SECTIONS.iter().enumerate() {
        let column = &mut texts[if wide { index } else { 0 }];
        column.push(Line::from(Span::styled(*title, theme.strong(theme.accent))));
        for (key, action, org) in *keys {
            let action: String = action.chars().take(action_width).collect();
            let mut spans = vec![
                Span::styled(format!(" {key:<10}"), theme.strong(theme.text)),
                Span::raw(format!("{action:<action_width$}")),
            ];
            if show_org && !org.is_empty() {
                spans.push(Span::styled(org.to_string(), theme.muted()));
            }
            column.push(Line::from(spans));
        }
        if !wide {
            column.push(Line::default());
        }
    }
    for (text, column) in texts.into_iter().zip(columns.iter()) {
        frame.render_widget(Paragraph::new(text), *column);
    }
}
