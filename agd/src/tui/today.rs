use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Clear, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};

use org_core::OccurrenceType;

use super::rows::days_since;
use super::theme::{Theme, centered};
use super::{
    App, DOING, DUE, DisplayRow, INBOX, Overlay, PromptKind, SCHEDULE, SLIPPED, Screen, peek,
    review, triage,
};

/// Width at which peek sits beside the list and tags get their column.
const WIDE: u16 = 100;
/// Width of the right-aligned column (category, ✓, ↻, provenance).
const RIGHT: usize = 14;

pub(super) fn draw(frame: &mut Frame<'_>, app: &mut App, area: Rect) {
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(area);
    draw_header(frame, app, parts[0]);
    let wide = area.width >= WIDE;
    let split = app.peek && wide && !matches!(app.screen, Screen::Review | Screen::Triage);
    let body = if split {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
            .split(parts[1])
            .to_vec()
    } else {
        vec![parts[1]]
    };
    app.list_area = body[0];
    match app.screen {
        Screen::Triage => triage::draw(frame, app, body[0]),
        Screen::Review => review::draw(frame, app, body[0]),
        _ => draw_list(frame, app, body[0], wide),
    }
    if split {
        peek::draw(frame, app, body[1], false);
    } else if app.peek
        && matches!(app.overlay, Overlay::None)
        && !matches!(app.screen, Screen::Review | Screen::Triage)
    {
        peek::draw_overlay(frame, app, parts[1]);
    }
    draw_footer(frame, app, parts[2]);
}

fn draw_header(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = &app.theme;
    let tab = |label: String, active: bool| -> Vec<Span<'static>> {
        if active {
            theme.pill(label, theme.accent)
        } else {
            vec![Span::styled(format!(" {label} "), theme.muted())]
        }
    };
    let mut left = vec![
        Span::styled(" Agenda ", theme.strong(theme.accent)),
        Span::raw(" "),
    ];
    left.extend(tab("Today".to_string(), app.screen == Screen::Today));
    left.extend(tab("Week".to_string(), app.screen == Screen::Week));
    left.extend(tab(
        format!("Inbox {}", app.counts.inbox),
        app.screen == Screen::Inbox,
    ));
    let agents = if app.counts.agents > 0 {
        format!("Agents ● {}", app.counts.agents)
    } else {
        "Agents".to_string()
    };
    left.extend(tab(agents, app.screen == Screen::Review));
    if let Screen::View(key) = &app.screen {
        let desc = app
            .settings
            .view(key)
            .map_or(key.as_str(), |view| view.desc.as_str());
        left.extend(tab(format!("{key} {desc}"), true));
    }
    if app.screen == Screen::Triage {
        left = theme.pill("TRIAGE", theme.due);
        left.push(Span::styled("  overdue, then inbox", theme.muted()));
    }
    let mut right = vec![];
    if app.log_mode {
        right.extend(theme.pill("log", theme.done));
        right.push(Span::raw(" "));
    }
    if !app.filter.is_empty() {
        right.extend(theme.pill(format!("/{}", app.filter), theme.accent));
        right.push(Span::raw(" "));
    }
    right.extend(theme.pill(
        format!("ctx {}", app.context.as_deref().unwrap_or("all")),
        theme.muted,
    ));
    right.push(Span::styled(
        format!("  {} ", app.day.format("%a %-d %b")),
        theme.fg(theme.text),
    ));
    frame.render_widget(Paragraph::new(Line::from(left)), area);
    frame.render_widget(
        Paragraph::new(Line::from(right)).alignment(Alignment::Right),
        area,
    );
}

fn draw_footer(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = &app.theme;
    let mode = match &app.screen {
        Screen::Today => "TODAY",
        Screen::Week => "WEEK",
        Screen::Inbox => "INBOX",
        Screen::View(_) => "VIEW",
        Screen::Triage => "TRIAGE",
        Screen::Review => "AGENTS",
    };
    let mut spans = theme.pill(mode, theme.accent);
    spans.push(Span::raw("  "));
    if let Some(status) = &app.status {
        let color = if status.error {
            theme.slipped
        } else {
            theme.done
        };
        spans.push(Span::styled(status.text.clone(), theme.strong(color)));
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
        return;
    }
    if let Some(row) = app.selected_row()
        && !matches!(app.screen, Screen::Review)
    {
        spans.push(Span::styled(app.short_location(&row.item), theme.muted()));
        spans.push(Span::styled("  │  ", theme.fg(theme.border)));
    }
    // Only the most-used keys; ? lists the rest.
    let keys: &[(&str, &str)] = match app.screen {
        Screen::Triage => &[("?", "keys")],
        Screen::Review => &[
            ("a", "accept"),
            ("u", "undo"),
            ("d", "diff"),
            ("A", "accept all"),
            ("f", "filter by agent"),
            ("esc", "back"),
        ],
        _ if !app.marks.is_empty() => &[
            (".", "today"),
            ("m", "tomorrow"),
            ("w", "next week"),
            ("s", "someday"),
            ("space", "unmark"),
        ],
        _ => &[
            ("x", "done"),
            ("t", "state"),
            ("s", "sched"),
            ("d", "due"),
            ("T", "triage"),
            ("c", "capture"),
            ("a", "views"),
            ("?", "help"),
        ],
    };
    if !app.marks.is_empty() {
        spans.push(Span::styled(
            format!("{} marked  ", app.marks.len()),
            theme.strong(theme.accent),
        ));
    }
    for (key, action) in keys {
        spans.extend(theme.key_hint(key, action));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn list_title(app: &App) -> String {
    match &app.screen {
        Screen::Today if app.classic => format!("Today · {}", app.day.format("%A %-d %B")),
        Screen::Today => {
            let effort = total_effort(app);
            if effort > 0 {
                format!("Today · {}:{:02} effort", effort / 60, effort % 60)
            } else {
                "Today".to_string()
            }
        }
        Screen::Week => format!("Week {}", app.day.format("%V")),
        Screen::Inbox => format!("Inbox · {}", app.rows.len()),
        Screen::View(key) => app
            .settings
            .view(key)
            .map_or_else(|| key.clone(), |view| view.desc.clone()),
        Screen::Triage | Screen::Review => String::new(),
    }
}

/// Sum of `:Effort:` (h:mm) over today's unfinished rows.
fn total_effort(app: &App) -> u32 {
    app.rows
        .iter()
        .filter(|row| !row.item.done)
        .filter_map(|row| {
            let (_, headline) = app.index.resolve(&row.item.item_ref).ok()?;
            let effort = headline
                .properties
                .iter()
                .find(|property| property.key.eq_ignore_ascii_case("Effort"))?;
            let (hours, minutes) = effort.value.trim().split_once(':')?;
            Some(hours.parse::<u32>().ok()? * 60 + minutes.parse::<u32>().ok()?)
        })
        .sum()
}

/// Rows with section headers between them. Headers are drawn but never
/// selected, so the cursor always sits on a task.
fn draw_list(frame: &mut Frame<'_>, app: &mut App, area: Rect, wide: bool) {
    let theme = &app.theme;
    let block = theme.panel(list_title(app));
    let inner = block.inner(area);
    let width = inner.width.saturating_sub(1) as usize;
    let mut items = Vec::new();
    // Row index for each drawn line; headers map to None.
    let mut line_rows: Vec<Option<usize>> = Vec::new();
    let mut selected_line = None;
    for header in &app.headers {
        let mut rows = app
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| &row.section == header)
            .peekable();
        let empty = rows.peek().is_none();
        if empty && app.screen == Screen::Today && !app.classic {
            continue;
        }
        items.push(ListItem::new(section_header(app, header, width)));
        line_rows.push(None);
        for (index, row) in rows {
            if index == app.selected {
                selected_line = Some(items.len());
            }
            items.push(ListItem::new(row_line(app, row, width, wide)));
            line_rows.push(Some(index));
        }
    }
    if app.rows.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            empty_message(app),
            theme.muted(),
        ))));
    }
    let total = items.len();
    let mut state = ListState::default().with_selected(selected_line);
    let list = List::new(items)
        .block(block)
        .highlight_symbol(Span::styled("▌", theme.fg(theme.accent)))
        .highlight_spacing(ratatui::widgets::HighlightSpacing::Always)
        .highlight_style(theme.selected());
    frame.render_stateful_widget(list, area, &mut state);
    app.click_rows = line_rows
        .iter()
        .skip(state.offset())
        .take(inner.height as usize)
        .enumerate()
        .filter_map(|(line, row)| row.map(|row| (inner.y + line as u16, row)))
        .collect();
    if total > inner.height as usize {
        let mut scrollbar = ScrollbarState::new(total).position(selected_line.unwrap_or(0));
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            area,
            &mut scrollbar,
        );
    }
}

fn empty_message(app: &App) -> String {
    match app.screen {
        Screen::Today => format!("Nothing scheduled today. {} in inbox.", app.counts.inbox),
        Screen::Inbox => "Inbox is empty.".to_string(),
        _ => "Nothing here.".to_string(),
    }
}

fn section_header(app: &App, section: &str, width: usize) -> Line<'static> {
    let theme = &app.theme;
    let color = match section {
        SLIPPED => theme.slipped,
        DUE => theme.due,
        DOING => theme.done,
        SCHEDULE | INBOX => theme.accent,
        _ => theme.text,
    };
    let mut spans = vec![Span::styled(section.to_string(), theme.strong(color))];
    // Today's sections show how many rows they hold; day headers do not,
    // as in org-agenda.
    if [SLIPPED, SCHEDULE, DUE, DOING, INBOX].contains(&section) {
        let count = match section {
            SLIPPED => app.counts.slipped,
            _ => app.rows.iter().filter(|row| row.section == section).count(),
        };
        spans.push(Span::styled(format!("  {count}"), theme.muted()));
    }
    if section == SLIPPED {
        let used: usize = spans.iter().map(Span::width).sum();
        let hint = "T triage ";
        spans.push(Span::raw(
            " ".repeat(width.saturating_sub(used + hint.len())),
        ));
        spans.push(Span::styled("T", theme.strong(theme.accent)));
        spans.push(Span::styled(" triage", theme.muted()));
    }
    Line::from(spans)
}

/// One task line: lead cell (badge, time or countdown), TODO keyword,
/// title, priority and tags, and a right-aligned context column.
fn row_line(app: &App, row: &DisplayRow, width: usize, wide: bool) -> Line<'static> {
    let theme = &app.theme;
    let item = &row.item;
    let mut spans = Vec::new();
    if app.marks.contains(&item.item_ref) {
        spans.push(Span::styled("● ", theme.strong(theme.accent)));
    }
    let classic = !matches!(
        row.section.as_str(),
        SLIPPED | SCHEDULE | DUE | DOING | INBOX
    ) && row.occurrence.is_some();
    if classic {
        // The org-agenda prefix: "category:  Sched. 2x:  TODO title".
        let occurrence = row
            .occurrence
            .as_ref()
            .expect("classic rows have occurrences");
        spans.push(Span::styled(
            format!("{:<12}", format!("{}:", item.category)),
            theme.muted(),
        ));
        if let Some(time) = &occurrence.time {
            spans.push(Span::raw(format!("{time}...... ")));
        }
        if !occurrence.extra.is_empty() {
            spans.push(Span::styled(
                format!("{:<11} ", occurrence.extra),
                lead_style(theme, occurrence.kind, &occurrence.extra),
            ));
        }
    } else {
        spans.extend(lead_cell(app, row));
    }
    if let Some(todo) = &item.todo {
        spans.push(Span::styled(format!("{todo} "), keyword_style(app, row)));
    }
    let title_style = if item.done {
        theme.done_row()
    } else {
        theme.fg(theme.text)
    };
    spans.push(Span::styled(peek::display_title(item), title_style));
    if let Some(priority) = &item.priority {
        spans.push(Span::raw(" "));
        spans.extend(theme.pill(priority.clone(), theme.due));
    }
    if wide && !item.tags.is_empty() {
        spans.push(Span::styled(
            format!("  #{}", item.tags.join(" #")),
            theme.fg(theme.accent),
        ));
    }
    let right = right_cell(app, row);
    let used: usize = spans.iter().map(Span::width).sum();
    let right_width = right.width();
    if used + right_width + 2 <= width {
        spans.push(Span::raw(" ".repeat(width - used - right_width)));
        spans.push(right);
    }
    let line = Line::from(spans);
    if item.done {
        line.style(Style::default().add_modifier(Modifier::DIM))
    } else {
        line
    }
}

/// Six-character lead: slipped days as a red badge, clock time, a
/// deadline countdown, or how long a WAIT has waited.
fn lead_cell(app: &App, row: &DisplayRow) -> Vec<Span<'static>> {
    let theme = &app.theme;
    let Some(occurrence) = &row.occurrence else {
        if row.section == DOING
            && let Some(age) = state_age(app, row)
        {
            return vec![Span::styled(
                format!("{:<6}", format!("{age}d")),
                theme.muted(),
            )];
        }
        return vec![Span::raw(" ".repeat(6))];
    };
    match occurrence.kind {
        OccurrenceType::PastScheduled => {
            let days = occurrence
                .extra
                .trim_start_matches("Sched.")
                .trim()
                .trim_end_matches("x:")
                .to_string();
            badge(theme, format!("{days}x"))
        }
        OccurrenceType::Deadline if occurrence.extra.ends_with("ago:") => {
            let days = occurrence.extra.split_whitespace().next().unwrap_or("?");
            badge(theme, format!("-{days}d"))
        }
        OccurrenceType::UpcomingDeadline => {
            let days = occurrence
                .extra
                .trim_start_matches("In")
                .split_whitespace()
                .next()
                .unwrap_or("?");
            vec![Span::styled(
                format!("{:<6}", format!("in {days}d")),
                theme.fg(theme.due),
            )]
        }
        OccurrenceType::Deadline => vec![Span::styled(
            format!("{:<6}", "due"),
            theme.strong(theme.due),
        )],
        _ => match &occurrence.time {
            Some(time) => {
                let style = if row.item.done {
                    theme.muted()
                } else {
                    theme.strong(theme.accent)
                };
                vec![Span::styled(format!("{time:<6}"), style)]
            }
            None => vec![Span::raw(" ".repeat(6))],
        },
    }
}

/// A red slipped badge filling the 6-column lead cell: a 4-column label
/// between its caps or padding.
fn badge(theme: &Theme, text: String) -> Vec<Span<'static>> {
    theme.pill(format!("{text:<4}"), theme.slipped)
}

fn lead_style(theme: &Theme, kind: OccurrenceType, extra: &str) -> Style {
    match kind {
        OccurrenceType::PastScheduled => theme.fg(theme.slipped),
        OccurrenceType::Deadline if extra.ends_with("ago:") => theme.fg(theme.slipped),
        OccurrenceType::Deadline | OccurrenceType::UpcomingDeadline => theme.fg(theme.due),
        _ => theme.muted(),
    }
}

/// Done keywords green, the first keyword of a sequence amber, middle
/// states (NEXT, WAIT, …) in accent.
fn keyword_style(app: &App, row: &DisplayRow) -> Style {
    let theme = &app.theme;
    if row.item.done {
        return theme.fg(theme.done);
    }
    let first = app
        .index
        .document(std::path::Path::new(&row.item.file))
        .and_then(|document| {
            let keyword = document.todo_keyword(row.item.todo.as_deref()?)?;
            document
                .todo_keywords()
                .iter()
                .find(|candidate| candidate.sequence == keyword.sequence)
        })
        .is_some_and(|head| Some(head.name.as_str()) == row.item.todo.as_deref());
    if first {
        theme.fg(theme.due)
    } else {
        theme.strong(theme.accent)
    }
}

/// Category for slipped rows, ✓ for done, the repeat interval for repeating
/// tasks, and who captured an inbox item.
fn right_cell(app: &App, row: &DisplayRow) -> Span<'static> {
    let theme = &app.theme;
    let item = &row.item;
    if item.done {
        return Span::styled("✓ ", theme.strong(theme.done));
    }
    if row.section == INBOX {
        return match app.provenance.get(&item.item_ref) {
            Some((actor, time)) if actor != "you" => {
                Span::styled(format!("{actor} {time} "), theme.fg(theme.agent(actor)))
            }
            Some((_, time)) => Span::styled(format!("you {time} "), theme.muted()),
            None => Span::raw(""),
        };
    }
    let repeater = item
        .planning
        .scheduled
        .as_ref()
        .or(item.planning.deadline.as_ref())
        .and_then(|timestamp| timestamp.repeater.as_deref());
    if let Some(repeater) = repeater {
        let interval = repeater.trim_start_matches(['.', '+']);
        return Span::styled(
            format!("↻ {} ", interval.split('/').next().unwrap_or(interval)),
            theme.muted(),
        );
    }
    if row.section == SLIPPED || row.section == DOING {
        let mut text = item.category.clone();
        text.truncate(RIGHT);
        return Span::styled(format!("{text} "), theme.muted());
    }
    Span::raw("")
}

/// Days since the item entered its current state, from the LOGBOOK
/// `State "WAIT" from …` line Org writes.
fn state_age(app: &App, row: &DisplayRow) -> Option<i64> {
    let todo = row.item.todo.as_deref()?;
    let text = peek::entry_text(&app.index, &row.item);
    let marker = format!("State \"{todo}\"");
    let line = text.lines().find(|line| line.contains(&marker))?;
    let start = line.rfind('[')? + 1;
    days_since(&line[start..], app.today)
}

/// Prompts, the refile picker and the views dispatcher.
pub(super) fn draw_overlay(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = &app.theme;
    frame
        .buffer_mut()
        .set_style(area, Style::default().add_modifier(Modifier::DIM));
    let input_line = |text: &str| {
        Line::from(vec![
            Span::styled("› ", theme.strong(theme.accent)),
            Span::raw(text.to_string()),
            Span::styled("▏", theme.fg(theme.accent)),
        ])
    };
    let footer = |hints: &[(&str, &str)]| {
        Line::from(
            hints
                .iter()
                .flat_map(|(key, action)| theme.key_hint(key, action))
                .collect::<Vec<_>>(),
        )
    };
    match &app.overlay {
        Overlay::Prompt { kind, input } => {
            let (title, hint) = match kind {
                PromptKind::Schedule => ("Schedule", "fri · +2d · 2026-10-30 · 14:00"),
                PromptKind::Deadline => ("Deadline", "fri · +2d · 2026-10-30 -3d"),
                PromptKind::Priority => ("Priority", "A–C, empty clears"),
                PromptKind::Tags => ("Tags", "+tag -tag"),
                PromptKind::Filter => ("Filter", "text or tag; empty clears"),
            };
            // border + top padding, input, gap, hint, gap, footer, border
            let popup = centered(area, 60, 2 + 3 + 1 + 1 + 1 + 1 + 1);
            frame.render_widget(Clear, popup);
            let block = theme.popup(title);
            let inner = block.inner(popup);
            frame.render_widget(block, popup);
            let parts = Layout::default()
                .direction(Direction::Vertical)
                .constraints([3, 1, 1, 1, 1].map(Constraint::Length))
                .split(inner);
            frame.render_widget(
                Paragraph::new(input_line(input)).block(theme.input(true)),
                parts[0],
            );
            frame.render_widget(Paragraph::new(Span::styled(hint, theme.muted())), parts[2]);
            frame.render_widget(
                Paragraph::new(footer(&[("⏎", "apply"), ("esc", "cancel")])),
                parts[4],
            );
        }
        Overlay::Refile(picker) => {
            let popup = centered(area, 84, area.height.saturating_sub(6).max(12));
            frame.render_widget(Clear, popup);
            let block = theme.popup("Refile to");
            let inner = block.inner(popup);
            frame.render_widget(block, popup);
            let parts = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Length(1),
                    Constraint::Min(1),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ])
                .split(inner);
            frame.render_widget(
                Paragraph::new(input_line(&picker.input)).block(theme.input(true)),
                parts[0],
            );
            let items: Vec<ListItem<'_>> = picker
                .matches()
                .iter()
                .map(|(_, label)| ListItem::new(label.clone()))
                .collect();
            let mut state = ListState::default().with_selected(Some(picker.selected));
            frame.render_stateful_widget(
                List::new(items)
                    .highlight_symbol(Span::styled("▌", theme.fg(theme.accent)))
                    .highlight_spacing(ratatui::widgets::HighlightSpacing::Always)
                    .highlight_style(theme.selected()),
                parts[2],
                &mut state,
            );
            frame.render_widget(
                Paragraph::new(footer(&[
                    ("⏎", "refile"),
                    ("↑↓", "choose"),
                    ("esc", "cancel"),
                ])),
                parts[4],
            );
        }
        Overlay::Views { selected } => {
            let views = &app.settings.views;
            // border + top padding, views, gap, footer, border
            let popup = centered(area, 64, 2 + views.len() as u16 + 1 + 1 + 1);
            frame.render_widget(Clear, popup);
            let block = theme.popup("Views");
            let inner = block.inner(popup);
            frame.render_widget(block, popup);
            let parts = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(1),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ])
                .split(inner);
            let items: Vec<ListItem<'_>> = views
                .iter()
                .map(|view| {
                    let kinds: Vec<&str> = view
                        .blocks
                        .iter()
                        .map(|block| block.kind.as_str())
                        .collect();
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{:<3}", view.key), theme.strong(theme.accent)),
                        Span::raw(view.desc.clone()),
                        Span::styled(format!("  {}", kinds.join(" + ")), theme.muted()),
                    ]))
                })
                .collect();
            let mut state = ListState::default().with_selected(Some(*selected));
            frame.render_stateful_widget(
                List::new(items)
                    .highlight_symbol(Span::styled("▌", theme.fg(theme.accent)))
                    .highlight_spacing(ratatui::widgets::HighlightSpacing::Always)
                    .highlight_style(theme.selected()),
                parts[0],
                &mut state,
            );
            frame.render_widget(
                Paragraph::new(footer(&[("key", "open"), ("⏎", "open"), ("esc", "close")])),
                parts[2],
            );
        }
        _ => {}
    }
}
