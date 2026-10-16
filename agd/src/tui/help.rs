//! `?`: every screen's keys from the key tables, the screen on view first,
//! scrolled with j/k. The org-agenda column shows only when it fits.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, ScrollbarState};

use super::keys::{self, KeyGroup};
use super::theme::{Theme, centered};
use super::{App, Overlay};

/// Gap between the key, help and org-agenda columns.
const GAP: usize = 2;
const ORG_HEADER: &str = "org-agenda";

/// Column widths for one popup width; `org` is None when it does not fit.
struct Columns {
    key: usize,
    help: usize,
    org: Option<usize>,
}

impl Columns {
    /// Every column at its widest entry when `width` allows; otherwise the
    /// org-agenda column goes and help text wraps.
    fn fit(width: usize) -> Self {
        let bindings = || {
            keys::SCREENS
                .iter()
                .flat_map(|(_, groups)| groups.iter())
                .flat_map(|group| group.bindings.iter())
        };
        let widest = |text: fn(&keys::Binding) -> &str| {
            bindings()
                .map(|binding| text(binding).chars().count())
                .max()
                .unwrap_or(0)
        };
        let key = widest(|binding| binding.label);
        let help = widest(|binding| binding.help);
        let org = widest(|binding| binding.org).max(ORG_HEADER.len());
        if width >= Self::full(key, help, org) {
            return Self {
                key,
                help,
                org: Some(org),
            };
        }
        Self {
            key,
            help: width.saturating_sub(1 + key + GAP).max(8),
            org: None,
        }
    }

    fn full(key: usize, help: usize, org: usize) -> usize {
        1 + key + GAP + help + GAP + org
    }

    /// The widest the popup needs to be.
    fn widest() -> usize {
        let columns = Self::fit(usize::MAX);
        Self::full(columns.key, columns.help, columns.org.unwrap_or(0))
    }
}

pub(super) fn draw(frame: &mut Frame<'_>, app: &mut App, area: Rect) {
    // The popup's border and padding take six columns.
    let width = Columns::widest() as u16 + 6;
    let popup = centered(area, width, area.height.saturating_sub(2));
    let block = app.theme.popup("Keys · j/k scroll · esc closes");
    let inner = block.inner(popup);
    let lines = help_lines(&app.theme, keys::current_screen(app), inner.width as usize);
    let max_scroll = lines.len().saturating_sub(inner.height as usize) as u16;
    let scroll = match &mut app.overlay {
        Overlay::Help { scroll } => {
            *scroll = (*scroll).min(max_scroll);
            *scroll
        }
        _ => 0,
    };
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), inner);
    if max_scroll > 0 {
        let mut state = ScrollbarState::new(max_scroll as usize).position(scroll as usize);
        frame.render_stateful_widget(app.theme.scrollbar(), popup, &mut state);
    }
}

/// The help text: the screen on view first, then the others; a group
/// shared by two screens is listed once.
fn help_lines(theme: &Theme, current: usize, width: usize) -> Vec<Line<'static>> {
    let columns = Columns::fit(width);
    let mut order = vec![current];
    order.extend((0..keys::SCREENS.len()).filter(|index| *index != current));
    let mut lines: Vec<Line<'static>> = wrap(keys::MOVING, width)
        .into_iter()
        .map(|line| Line::from(Span::styled(line, theme.muted())))
        .collect();
    if columns.org.is_some() {
        let pad = 1 + columns.key + GAP + columns.help + GAP;
        lines.push(Line::from(Span::styled(
            format!("{:pad$}{ORG_HEADER}", ""),
            theme.muted(),
        )));
    }
    let mut shown: Vec<&str> = Vec::new();
    for index in order {
        let (title, groups) = keys::SCREENS[index];
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        lines.push(Line::from(Span::styled(
            title.to_uppercase(),
            theme.strong(theme.accent),
        )));
        for group in groups {
            if shown.contains(&group.title) {
                lines.push(Line::from(Span::styled(
                    format!("{}: as above", group.title),
                    theme.muted(),
                )));
                continue;
            }
            shown.push(group.title);
            lines.extend(group_lines(theme, group, &columns));
        }
    }
    lines
}

/// A group's heading and one row per binding; help text wraps under its
/// own column rather than running into the next.
fn group_lines(theme: &Theme, group: &KeyGroup, columns: &Columns) -> Vec<Line<'static>> {
    let (key_width, help_width) = (columns.key, columns.help);
    let mut lines = vec![Line::from(Span::styled(
        group.title.to_string(),
        theme.muted(),
    ))];
    for binding in group.bindings {
        for (row, text) in wrap(binding.help, help_width).into_iter().enumerate() {
            let key = if row == 0 { binding.label } else { "" };
            let mut spans = vec![
                Span::styled(
                    format!(" {key:<key_width$}{:GAP$}", ""),
                    theme.strong(theme.text),
                ),
                Span::raw(format!("{text:<help_width$}")),
            ];
            if columns.org.is_some() && row == 0 && !binding.org.is_empty() {
                spans.push(Span::raw(" ".repeat(GAP)));
                spans.push(Span::styled(binding.org.to_string(), theme.muted()));
            }
            lines.push(Line::from(spans));
        }
    }
    lines
}

/// Words of `text` in lines of at most `width` characters.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = vec![String::new()];
    for word in text.split(' ') {
        let current = lines.last_mut().expect("one line");
        let length = current.chars().count();
        if length > 0 && length + 1 + word.chars().count() > width {
            lines.push(word.to_string());
        } else {
            if length > 0 {
                current.push(' ');
            }
            current.push_str(word);
        }
    }
    lines
}
