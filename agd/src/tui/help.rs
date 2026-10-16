//! `?`: every screen's keys from the key tables, the screen on view first,
//! as a palette. Typing filters the actions by name (`sched` finds `s
//! schedule`), ↑↓ (or C-n/C-p, C-j/C-k) choose one, and ⏎ runs it as if
//! its key were pressed; esc clears the filter, then closes. The
//! org-agenda column shows only when it fits.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, ScrollbarState};

use super::App;
use super::input::TextInput;
use super::keys::{self, Binding};
use super::theme::{Theme, centered, scrollbar_track};

/// Gap between the key, help and org-agenda columns.
const GAP: usize = 2;
/// From this many terminal columns help has room for the org-agenda
/// column; under it, two columns that fit 80 wide with room to spare.
const WIDE: u16 = 96;
const ORG_HEADER: &str = "org-agenda";

/// The palette's state: the filter as typed and the highlighted action,
/// an index into `entries`.
#[derive(Debug, Default)]
pub(super) struct Palette {
    pub query: TextInput,
    pub selected: usize,
}

/// One action help lists: its binding and the screen (keys::SCREENS) it
/// belongs to. Only the screen on view's actions can run.
pub(super) struct Entry {
    pub screen: usize,
    pub binding: &'static Binding,
}

/// Every action in help's order, the screen on view first, a group shared
/// by two screens listed once; with a query, those whose key, words or
/// org-agenda key contain it, ignoring case.
pub(super) fn entries(current: usize, query: &str) -> Vec<Entry> {
    let query = query.trim().to_lowercase();
    let mut order = vec![current];
    order.extend((0..keys::SCREENS.len()).filter(|index| *index != current));
    let mut shown: Vec<&str> = Vec::new();
    let mut entries = Vec::new();
    for screen in order {
        for group in keys::SCREENS[screen].1 {
            if shown.contains(&group.title) {
                continue;
            }
            shown.push(group.title);
            for binding in group.bindings {
                let words = format!(
                    "{} {} {} {}",
                    binding.label, binding.short, binding.help, binding.org
                )
                .to_lowercase();
                if query.is_empty() || words.contains(&query) {
                    entries.push(Entry { screen, binding });
                }
            }
        }
    }
    entries
}

/// Column widths for one popup width; `org` is None when it does not fit.
struct Columns {
    key: usize,
    help: usize,
    org: Option<usize>,
}

impl Columns {
    /// Every column at its widest entry when `width` allows; otherwise the
    /// org-agenda column goes and help text wraps.
    fn fit(width: usize, org: bool) -> Self {
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
        let org_width = widest(|binding| binding.org).max(ORG_HEADER.len());
        if org && width >= Self::full(key, help, org_width) {
            return Self {
                key,
                help,
                org: Some(org_width),
            };
        }
        Self {
            key,
            help: width.saturating_sub(1 + key + GAP).clamp(8, help.max(8)),
            org: None,
        }
    }

    fn full(key: usize, help: usize, org: usize) -> usize {
        1 + key + GAP + help + GAP + org
    }

    /// The widest the popup needs to be, with or without the org-agenda
    /// column.
    fn widest(org: bool) -> usize {
        let columns = Self::fit(usize::MAX, org);
        match columns.org {
            Some(org) => Self::full(columns.key, columns.help, org),
            None => 1 + columns.key + GAP + columns.help,
        }
    }
}

pub(super) fn draw(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let super::Overlay::Help(palette) = &app.overlay else {
        return;
    };
    let theme = &app.theme;
    let org = area.width >= WIDE;
    // The popup's border and padding take six columns.
    let width = Columns::widest(org) as u16 + 6;
    let popup = centered(area, width, area.height.saturating_sub(2));
    let block = theme.popup("Keys");
    let inner = block.inner(popup);
    // The filter, a blank row, the keys, a blank row and the key hints.
    let row = |y: u16, height: u16| Rect {
        y: inner.y + y,
        height,
        ..inner
    };
    let body = row(2, inner.height.saturating_sub(4));
    let current = keys::current_screen(app);
    let query = palette.query.text();
    let (lines, selected_line) = help_lines(
        theme,
        current,
        query,
        palette.selected,
        body.width as usize,
        org,
    );
    let max_scroll = lines.len().saturating_sub(body.height as usize);
    // The highlighted action stays in view, near the middle once it can.
    let scroll = selected_line
        .unwrap_or(0)
        .saturating_sub(body.height as usize / 2)
        .min(max_scroll) as u16;
    // Dim whatever is behind the popup, as every popup does.
    frame.buffer_mut().set_style(
        area,
        ratatui::style::Style::default().add_modifier(Modifier::DIM),
    );
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    let mut filter = vec![Span::styled("› ", theme.strong(theme.accent))];
    if query.is_empty() {
        filter.push(Span::styled("type to find an action", theme.muted()));
    } else {
        filter.extend(palette.query.spans(theme, true));
    }
    frame.render_widget(Paragraph::new(Line::from(filter)), row(0, 1));
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), body);
    let escape = if query.is_empty() { "close" } else { "clear" };
    frame.render_widget(
        Paragraph::new(Line::from(
            [
                theme.key_hint("⏎", "run"),
                theme.key_hint("↑↓", "choose"),
                theme.key_hint("esc", escape),
            ]
            .concat(),
        )),
        row(inner.height.saturating_sub(1), inner.height.min(1)),
    );
    if max_scroll > 0 {
        let mut state = ScrollbarState::new(max_scroll).position(scroll as usize);
        frame.render_stateful_widget(theme.scrollbar(), scrollbar_track(popup), &mut state);
    }
}

/// The help text and the line of the highlighted action. Without a query:
/// every screen's keys under its title, the screen on view first. With
/// one: the matching actions alone, each from another screen naming it.
fn help_lines(
    theme: &Theme,
    current: usize,
    query: &str,
    selected: usize,
    width: usize,
    org: bool,
) -> (Vec<Line<'static>>, Option<usize>) {
    let columns = Columns::fit(width, org);
    let entries = entries(current, query);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut selected_line = None;
    if entries.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("No action matches “{}”", query.trim()),
            theme.muted(),
        )));
        return (lines, None);
    }
    let filtered = !query.trim().is_empty();
    if !filtered {
        lines.extend(
            wrap(keys::MOVING, width)
                .into_iter()
                .map(|line| Line::from(Span::styled(line, theme.muted()))),
        );
        if columns.org.is_some() {
            let pad = 1 + columns.key + GAP + columns.help + GAP;
            lines.push(Line::from(Span::styled(
                format!("{:pad$}{ORG_HEADER}", ""),
                theme.muted(),
            )));
        }
    }
    let mut previous: Option<(usize, &str)> = None;
    for (index, entry) in entries.iter().enumerate() {
        let (title, groups) = keys::SCREENS[entry.screen];
        let group = groups
            .iter()
            .find(|group| shares(entry, group))
            .map_or("", |group| group.title);
        if !filtered {
            if previous.is_none_or(|(screen, _)| screen != entry.screen) {
                if !lines.is_empty() {
                    lines.push(Line::default());
                }
                lines.push(Line::from(Span::styled(
                    title.to_uppercase(),
                    theme.strong(theme.accent),
                )));
                // A group listed under an earlier screen is not repeated.
                for skipped in groups.iter().filter(|candidate| {
                    entries
                        .iter()
                        .any(|other| other.screen != entry.screen && shares(other, candidate))
                }) {
                    lines.push(Line::from(Span::styled(
                        format!("{}: as above", skipped.title),
                        theme.muted(),
                    )));
                }
            }
            if previous.is_none_or(|(_, last)| last != group) {
                lines.push(Line::from(Span::styled(group.to_string(), theme.muted())));
            }
        }
        previous = Some((entry.screen, group));
        if index == selected {
            selected_line = Some(lines.len());
        }
        let elsewhere = (filtered && entry.screen != current).then_some(title);
        lines.extend(binding_lines(
            theme,
            entry.binding,
            &columns,
            index == selected,
            elsewhere,
        ));
    }
    (lines, selected_line)
}

/// Whether `entry` belongs to `group`.
fn shares(entry: &Entry, group: &keys::KeyGroup) -> bool {
    group
        .bindings
        .iter()
        .any(|binding| std::ptr::eq(binding, entry.binding))
}

/// One binding's rows: the key in accent bold, what it does (wrapping
/// under its own column), and org-agenda's key where it fits. The
/// highlighted one carries the selection bar; one from another screen
/// names that screen, since it does not run here.
fn binding_lines(
    theme: &Theme,
    binding: &Binding,
    columns: &Columns,
    selected: bool,
    elsewhere: Option<&str>,
) -> Vec<Line<'static>> {
    let (key_width, help_width) = (columns.key, columns.help);
    let mut lines = Vec::new();
    for (row, text) in wrap(binding.help, help_width).into_iter().enumerate() {
        let key = if row == 0 { binding.label } else { "" };
        let bar = if selected && row == 0 { "▌" } else { " " };
        // Another screen's action names that screen after what it does,
        // in place of org-agenda's key.
        let screen = elsewhere
            .filter(|_| row == 0)
            .map(|title| format!(" · {}", title.split(" · ").next().unwrap_or(title)))
            .unwrap_or_default();
        let padding = help_width.saturating_sub(text.chars().count() + screen.chars().count());
        let mut spans = vec![
            Span::styled(bar, theme.fg(theme.accent)),
            Span::styled(
                format!("{key:<key_width$}{:GAP$}", ""),
                theme.strong(theme.accent),
            ),
            Span::raw(text),
            Span::styled(screen, theme.muted()),
            Span::raw(" ".repeat(padding)),
        ];
        if columns.org.is_some() && row == 0 && elsewhere.is_none() && !binding.org.is_empty() {
            spans.push(Span::raw(" ".repeat(GAP)));
            spans.push(Span::styled(binding.org.to_string(), theme.muted()));
        }
        let line = Line::from(spans);
        lines.push(if selected {
            line.style(theme.selected().remove_modifier(Modifier::UNDERLINED))
        } else {
            line
        });
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
