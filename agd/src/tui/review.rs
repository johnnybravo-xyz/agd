//! The Agents tab: one card per agent write, every card the same shape:
//!
//! ```text
//! ▌  ● codex          ~/src/app · 15:01                       state
//! ▌    TODO → DONE  Rotate staging certs                work.org:42
//! ```
//!
//! the marker cell (`*` when marked, as on a list row), who, where from and
//! when, and what kind of change; then the change in the fewest words, the
//! task, and where it is. SPC peeks at the change's diff and its task.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};
use unicode_width::UnicodeWidthStr;

use agd_core::{Item, JournalRecord, JournalStatus};

use super::App;
use super::theme::{Theme, truncate};

/// Cells for an agent's name: 13 characters and "…".
const NAME: usize = 14;
/// The change column is as wide as the widest change, up to this.
const CHANGE_MAX: usize = 24;
/// The right column (kind, file:line), as on Today.
const RIGHT: usize = 14;

/// What an agent change did, in the fewest words that let you judge it.
enum Change {
    Captured {
        heading: String,
    },
    State {
        heading: String,
        from: String,
        to: String,
    },
    Dates {
        from: String,
        to: String,
    },
    Refiled {
        heading: String,
    },
    Edited,
}

impl Change {
    /// The kind, as the card's first line names it.
    fn kind(&self) -> &'static str {
        match self {
            Change::Captured { .. } => "captured",
            Change::State { .. } => "state",
            Change::Dates { .. } => "date",
            Change::Refiled { .. } => "refiled",
            Change::Edited => "edited",
        }
    }

    /// The headline the diff names, for when the task is gone.
    fn heading(&self) -> Option<&str> {
        match self {
            Change::Captured { heading }
            | Change::State { heading, .. }
            | Change::Refiled { heading } => Some(heading),
            Change::Dates { .. } | Change::Edited => None,
        }
    }
}

fn classify(record: &JournalRecord, today: chrono::NaiveDate) -> Change {
    let removed = changed(record, '-');
    let added = changed(record, '+');
    let headline = |lines: &[(usize, &str)]| {
        lines
            .iter()
            .find(|(_, line)| line.starts_with('*'))
            .map(|(_, line)| line.to_string())
    };
    if record.transaction.is_some() {
        return Change::Refiled {
            heading: headline(&added)
                .or_else(|| headline(&removed))
                .unwrap_or_default(),
        };
    }
    match (headline(&removed), captured_headline(record, &added)) {
        (None, Some(heading)) => Change::Captured { heading },
        (Some(old), Some(new)) => {
            let (from, to) = (second_word(&old), second_word(&new));
            if from == to {
                Change::Edited
            } else {
                Change::State {
                    heading: new,
                    from,
                    to,
                }
            }
        }
        _ => {
            let date = |lines: &[(usize, &str)]| {
                lines.iter().find_map(|(_, line)| compact_date(line, today))
            };
            match (date(&removed), date(&added)) {
                (from, Some(to)) => Change::Dates {
                    from: from.unwrap_or_else(|| "—".to_string()),
                    to,
                },
                _ => Change::Edited,
            }
        }
    }
}

/// The headline a capture added. A capture that also created its parent
/// (`* Inbox`) adds two; the task is the one on the line its ref names,
/// else the last.
fn captured_headline(record: &JournalRecord, added: &[(usize, &str)]) -> Option<String> {
    let line = record
        .item_ref
        .as_deref()
        .and_then(|item_ref| item_ref.split_once("#sha256:"))
        .and_then(|(location, _)| location.rsplit_once(':'))
        .and_then(|(_, line)| line.parse::<usize>().ok());
    let headlines: Vec<&(usize, &str)> = added
        .iter()
        .filter(|(_, text)| text.starts_with('*'))
        .collect();
    headlines
        .iter()
        .find(|(number, _)| Some(*number) == line)
        .or(headlines.last())
        .map(|(_, text)| text.to_string())
}

/// `SCHEDULED: <2026-10-15 Thu 15:00>` reads as `Thu 15 Oct 15:00`.
fn compact_date(line: &str, today: chrono::NaiveDate) -> Option<String> {
    let inner = line.split_once('<')?.1.split_once('>')?.0;
    let mut parts = inner.split_whitespace();
    let date = parts.next()?;
    date.parse::<chrono::NaiveDate>().ok()?;
    let _weekday = parts.next();
    let time = parts.next().filter(|part| part.contains(':'));
    Some(crate::words::when(date, time, today))
}

/// The diff's added (`+`) or removed (`-`) lines with their line in the
/// new file, without the file headers or blank lines.
fn changed(record: &JournalRecord, sign: char) -> Vec<(usize, &str)> {
    let header = format!("{sign}{sign}{sign}");
    let mut lines = Vec::new();
    let mut number = 0;
    for text in record.diff.lines() {
        if let Some(hunk) = text.strip_prefix("@@ ") {
            number = hunk
                .split_whitespace()
                .find_map(|part| part.strip_prefix('+'))
                .and_then(|plus| plus.split(',').next()?.parse().ok())
                .unwrap_or(0);
            continue;
        }
        if text.starts_with(&header) {
            continue;
        }
        if text.starts_with(sign) && !text[1..].trim().is_empty() {
            lines.push((number, text[1..].trim()));
        }
        // Removed lines are not in the new file.
        if !text.starts_with('-') {
            number += 1;
        }
    }
    lines
}

fn second_word(headline: &str) -> String {
    headline
        .split_whitespace()
        .nth(1)
        .filter(|word| word.chars().all(|c| c.is_ascii_uppercase()))
        .unwrap_or("")
        .to_string()
}

/// The task a change touched, as the file reads now: the heading whose
/// section holds the first patch's start. None once the change is undone
/// or nothing is left at that place.
pub(super) fn touched_item(app: &App, record: &JournalRecord) -> Option<Item> {
    if matches!(
        record.status,
        JournalStatus::Undone | JournalStatus::Unapplied
    ) {
        return None;
    }
    let start = record.patches.first()?.start;
    let file = record.file.split(',').next()?;
    app.index
        .document(std::path::Path::new(file))?
        .headlines()
        .iter()
        .find(|headline| headline.section_span.start <= start && start < headline.section_span.end)
        .map(|headline| headline.item.clone())
}

/// The task a change is about: the heading its ref names while the file
/// still holds it (a capture that also made its parent names the task),
/// else the heading at the change's place.
pub(super) fn subject(app: &App, record: &JournalRecord) -> Option<Item> {
    if matches!(
        record.status,
        JournalStatus::Undone | JournalStatus::Unapplied
    ) {
        return None;
    }
    app.index
        .items()
        .find(|item| record.item_ref.as_deref() == Some(item.item_ref.as_str()))
        .cloned()
        .or_else(|| touched_item(app, record))
}

/// Where a change is, for its card and for ⏎: the task's line as the file
/// reads now, else the diff's first changed line.
pub(super) fn changed_place(app: &App, record: &JournalRecord) -> (String, usize) {
    match subject(app, record) {
        Some(item) => (item.file, item.line),
        None => (
            record
                .file
                .split(',')
                .next()
                .unwrap_or_default()
                .to_string(),
            first_changed_line(record),
        ),
    }
}

/// The new-file line of the diff's first added or removed line: the hunk
/// header gives where its context starts, and each context line counts.
fn first_changed_line(record: &JournalRecord) -> usize {
    let mut line: Option<usize> = None;
    for text in record.diff.lines() {
        if let Some(hunk) = text.strip_prefix("@@ ") {
            line = hunk
                .split_whitespace()
                .find_map(|part| part.strip_prefix('+'))
                .and_then(|plus| plus.split(',').next()?.parse().ok());
            continue;
        }
        let Some(current) = line else {
            continue;
        };
        if text.starts_with('+') || text.starts_with('-') {
            return current.max(1);
        }
        line = Some(current + 1);
    }
    line.unwrap_or(1).max(1)
}

/// The agent's normalized client name (`mcp:codex` is `codex`).
pub(super) fn agent_name(record: &JournalRecord) -> &str {
    record.actor.strip_prefix("mcp:").unwrap_or(&record.actor)
}

/// What a card and the status line call a change's task: its title as the
/// agenda shows it, else the headline the diff names.
pub(super) fn subject_title(app: &App, record: &JournalRecord) -> String {
    match subject(app, record) {
        Some(item) => super::peek::display_title(&item),
        None => classify(record, app.today)
            .heading()
            .map(crate::words::headline_title)
            .unwrap_or_default(),
    }
}

/// `work.org:42` for a change.
pub(super) fn subject_location(app: &App, record: &JournalRecord) -> String {
    let (file, line) = changed_place(app, record);
    crate::words::location(&file, line, app.index.roots().roots())
}

pub(super) fn draw(frame: &mut Frame<'_>, app: &mut App, area: Rect) {
    let theme = &app.theme;
    let unreviewed = app
        .journals
        .iter()
        .filter(|record| record.status == JournalStatus::Committed)
        .count();
    let mut title = format!("{unreviewed} unreviewed");
    if let Some(agent) = &app.agent_filter {
        title.push_str(&format!(" · {agent}"));
    }
    let block = theme.panel(title);
    let inner = block.inner(area);
    if app.journals.is_empty() {
        app.click_rows.clear();
        frame.render_widget(
            Paragraph::new(vec![
                Line::from("No agent changes to review."),
                Line::from(Span::styled(
                    "What agents write over MCP shows up here, with who wrote it.",
                    theme.muted(),
                )),
            ])
            .block(block),
            area,
        );
        return;
    }
    // The highlight column takes one cell.
    let width = inner.width.saturating_sub(1) as usize;
    let cards: Vec<Card> = app
        .journals
        .iter()
        .map(|record| Card::new(app, record))
        .collect();
    let change_width = cards
        .iter()
        .map(|card| card.change.width())
        .max()
        .unwrap_or(0)
        .min(CHANGE_MAX);
    // A blank row between cards is its own item, so the selection never
    // covers it and the bar runs down exactly the card.
    let mut items = Vec::new();
    let mut lines_for: Vec<Option<usize>> = Vec::new();
    for (index, card) in cards.iter().enumerate() {
        if index > 0 {
            items.push(ListItem::new(Line::default()));
            lines_for.push(None);
        }
        items.push(ListItem::new(card.lines(theme, width, change_width)));
        lines_for.extend([Some(index), Some(index)]);
    }
    let mut state = ListState::default().with_selected(Some(app.selected * 2));
    frame.render_stateful_widget(
        List::new(items)
            .block(block)
            .highlight_symbol(Span::styled("▌", theme.fg(theme.accent)))
            .repeat_highlight_symbol(true)
            .highlight_spacing(ratatui::widgets::HighlightSpacing::Always)
            .highlight_style(theme.selected()),
        area,
        &mut state,
    );
    // Rows on screen for mouse clicks: each card's two lines, from the
    // first item shown.
    let first_line: usize = (0..state.offset())
        .map(|item| if item % 2 == 0 { 2 } else { 1 })
        .sum();
    app.click_rows = lines_for
        .iter()
        .skip(first_line)
        .take(inner.height as usize)
        .enumerate()
        .filter_map(|(line, card)| card.map(|card| (inner.y + line as u16, card)))
        .collect();
}

/// One change, ready to draw.
struct Card {
    /// Marked with `m` or `*`, for `a` to accept.
    marked: bool,
    reviewed: bool,
    agent: String,
    project: String,
    time: String,
    kind: &'static str,
    /// `TODO → DONE`, `captured`, `Thu 15 Oct → Fri 16 Oct`.
    change: String,
    /// The change's colour roles: before and after the arrow, if any.
    change_parts: Vec<(String, ChangeRole)>,
    title: String,
    location: String,
}

#[derive(Clone, Copy)]
enum ChangeRole {
    Was,
    Now,
    Plain,
}

impl Card {
    fn new(app: &App, record: &JournalRecord) -> Self {
        let change = classify(record, app.today);
        let arrow = |from: &str, to: &str| {
            vec![
                (from.to_string(), ChangeRole::Was),
                (" → ".to_string(), ChangeRole::Plain),
                (to.to_string(), ChangeRole::Now),
            ]
        };
        let change_parts = match &change {
            Change::State { from, to, .. } => arrow(from, to),
            Change::Dates { from, to } => arrow(from, to),
            other => vec![(other.kind().to_string(), ChangeRole::Now)],
        };
        Self {
            marked: app.marks.contains(&record.id),
            reviewed: record.status != JournalStatus::Committed,
            agent: agent_name(record).to_string(),
            project: shorten_project(&record.project),
            time: record.timestamp.get(11..16).unwrap_or_default().to_string(),
            kind: change.kind(),
            change: change_parts.iter().map(|(text, _)| text.as_str()).collect(),
            change_parts,
            title: subject_title(app, record),
            location: subject_location(app, record),
        }
    }

    /// Who, where from, when and the kind; then the change, the task and
    /// where it is. A reviewed card is muted throughout.
    fn lines(&self, theme: &Theme, width: usize, change_width: usize) -> Vec<Line<'static>> {
        let muted = theme.muted();
        let role_style = |role: ChangeRole| -> Style {
            if self.reviewed {
                return muted;
            }
            match (role, self.kind) {
                (ChangeRole::Plain, _) => muted,
                (ChangeRole::Was, "state") => theme.fg(theme.due),
                (ChangeRole::Now, "state") => theme.strong(theme.done),
                (ChangeRole::Was, _) => muted,
                (ChangeRole::Now, "captured") => theme.fg(theme.done),
                (ChangeRole::Now, "date") => theme.fg(theme.accent),
                (ChangeRole::Now, _) => theme.fg(theme.text),
            }
        };
        let dot = if self.reviewed {
            muted
        } else {
            theme.fg(theme.agent(&self.agent))
        };
        let name = if self.reviewed {
            muted
        } else {
            theme.strong(theme.text)
        };
        let kind = if self.reviewed {
            Span::styled("reviewed ", muted)
        } else {
            Span::styled(format!("{} ", self.kind), role_style(ChangeRole::Now))
        };
        // The marker cell, as on a list row: `*` when marked.
        let marker = if self.marked {
            Span::styled("* ", theme.strong(theme.accent))
        } else {
            Span::raw("  ")
        };
        let mut head = vec![
            marker,
            Span::styled("● ", dot),
            Span::styled(format!("{:<NAME$} ", truncate(&self.agent, NAME)), name),
        ];
        let used = head.iter().map(Span::width).sum::<usize>() + kind.width();
        let place = format!("{} · {}", self.project, self.time);
        head.push(Span::styled(
            truncate(&place, width.saturating_sub(used + 1)),
            muted,
        ));
        head.push(Span::raw(" ".repeat(width.saturating_sub(
            head.iter().map(Span::width).sum::<usize>() + kind.width(),
        ))));
        head.push(kind);

        // Under the agent's name: past the marker cell and the dot.
        let mut body = vec![Span::raw("    ")];
        for (text, role) in &self.change_parts {
            body.push(Span::styled(text.clone(), role_style(*role)));
        }
        body.push(Span::raw(
            " ".repeat(change_width.saturating_sub(self.change.width()) + 2),
        ));
        let location = format!("{} ", truncate(&self.location, RIGHT));
        let used = body.iter().map(Span::width).sum::<usize>();
        let room = width.saturating_sub(used + location.width() + 2);
        let title = truncate(&self.title, room);
        let text = if self.reviewed {
            muted
        } else {
            theme.fg(theme.text)
        };
        body.push(Span::styled(title.clone(), text));
        body.push(Span::raw(" ".repeat(
            width.saturating_sub(used + title.width() + location.width()),
        )));
        body.push(Span::styled(location, muted));
        vec![Line::from(head), Line::from(body)]
    }
}

/// The agent's project as `~/src/app`, or its last two path parts, cut
/// from the left to 22 cells.
fn shorten_project(path: &str) -> String {
    let home = crate::words::home_path(path);
    let short = if home.starts_with('~') {
        home
    } else {
        let parts: Vec<&str> = path.trim_end_matches('/').rsplit('/').take(2).collect();
        format!(
            "…/{}",
            parts.into_iter().rev().collect::<Vec<_>>().join("/")
        )
    };
    if short.chars().count() <= 22 {
        return short;
    }
    let tail: String = short
        .chars()
        .rev()
        .take(21)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("…{tail}")
}
