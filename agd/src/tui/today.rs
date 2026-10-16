//! The full layout every screen shares (the header with its tabs and
//! filters, the body, the footer with the status line and key hints) and
//! the lists drawn in it: Today, Week, Inbox and saved views, row by row.
//! Also the prompts, the refile picker, the bulk menu and the Views popup
//! drawn over them.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, ListState, Paragraph, ScrollbarState};

use agd_core::{OccurrenceType, keyword_color};
use chrono::{NaiveTime, Timelike};
use unicode_width::UnicodeWidthStr;

use super::input::TextInput;
use super::rows::days_since;
use super::theme::{Theme, centered, scrollbar_track, truncate};
use super::{
    App, DOING, DUE, DisplayRow, Header, INBOX, OVERDUE, Overlay, PromptKind, SCHEDULE, Screen,
    actions, keys, peek, review, triage,
};

/// Width at which peek sits beside the list and tags get their column.
const WIDE: u16 = 100;
/// Width of the right-aligned column (category, ↻, provenance).
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
    let split = app.peek && wide && app.screen != Screen::Triage;
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
    } else if app.peek && matches!(app.overlay, Overlay::None) && app.screen != Screen::Triage {
        peek::draw_overlay(frame, app, parts[1]);
    }
    draw_footer(frame, app, parts[2]);
}

/// The Agenda badge and tabs at the left; the active filters and the date
/// at the right, in the room the tabs leave.
fn draw_header(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let width = area.width as usize;
    let left = header_tabs(app, width);
    let right = header_pills(app, width.saturating_sub(span_width(&left) + 1));
    frame.render_widget(Paragraph::new(Line::from(left)), area);
    frame.render_widget(
        Paragraph::new(Line::from(right)).alignment(Alignment::Right),
        area,
    );
}

/// The badge and one tab per screen, plus an open view that is not a tab
/// and Triage while it runs. When they do not fit, inactive tabs go from
/// the right; the active tab always stays.
fn header_tabs(app: &App, width: usize) -> Vec<Span<'static>> {
    let theme = &app.theme;
    // The name is a filled badge; the current tab is bold and underlined,
    // so the two never look alike.
    // A count that asks for attention (`● 2` changes to review) stands out
    // in bold accent whichever tab is open.
    let tab = |label: String, alert: Option<String>, active: bool| {
        let style = if active {
            theme.strong(theme.text).add_modifier(Modifier::UNDERLINED)
        } else {
            theme.muted()
        };
        let mut spans = vec![Span::raw(" "), Span::styled(label, style)];
        if let Some(alert) = alert {
            spans.push(Span::styled(
                format!(" {alert}"),
                theme.strong(theme.accent),
            ));
        }
        spans.push(Span::raw(" "));
        (spans, active)
    };
    let view_desc = |key: &str| {
        app.settings
            .view(key)
            .map_or_else(|| key.to_string(), |view| view.desc.clone())
    };
    let screens = app.tabs();
    let mut tabs: Vec<(Vec<Span<'static>>, bool)> = screens
        .iter()
        .map(|screen| {
            let label = match screen {
                Screen::View(key) => view_desc(key),
                Screen::Week => "Week".to_string(),
                Screen::Inbox => format!("Inbox {}", app.counts.inbox),
                Screen::Review => "Agents".to_string(),
                _ => "Today".to_string(),
            };
            let alert = (*screen == Screen::Review && app.counts.agents > 0)
                .then(|| format!("● {}", app.counts.agents));
            tab(label, alert, app.screen == *screen)
        })
        .collect();
    if let Screen::View(key) = &app.screen
        && !screens.contains(&app.screen)
    {
        tabs.push(tab(format!("{key} {}", view_desc(key)), None, true));
    }
    if app.screen == Screen::Triage {
        tabs.push(tab("Triage".to_string(), None, true));
    }
    let mut left = theme.pill("Agenda", theme.accent);
    left.push(Span::raw(" "));
    let total = |tabs: &[(Vec<Span<'static>>, bool)]| {
        span_width(&left)
            + tabs
                .iter()
                .map(|(spans, _)| span_width(spans))
                .sum::<usize>()
    };
    while total(&tabs) > width
        && let Some(inactive) = tabs.iter().rposition(|(_, active)| !active)
    {
        tabs.remove(inactive);
    }
    left.extend(tabs.into_iter().flat_map(|(spans, _)| spans));
    left
}

/// The active filters (text, log mode, context) as accent pills, then the
/// day; whatever does not fit in `room` is left out, the day first.
fn header_pills(app: &App, room: usize) -> Vec<Span<'static>> {
    let theme = &app.theme;
    let mut pills: Vec<Vec<Span<'static>>> = Vec::new();
    if !app.filter.is_empty() {
        pills.push(theme.pill(format!("/{}", app.filter), theme.accent));
    }
    if app.log_mode {
        pills.push(theme.pill("log", theme.accent));
    }
    if let Some(context) = &app.context {
        pills.push(theme.pill(format!("ctx {context}"), theme.accent));
    }
    pills.push(vec![Span::styled(
        format!(" {} ", crate::words::day(app.day, app.today)),
        theme.fg(theme.text),
    )]);
    let mut right: Vec<Span<'static>> = Vec::new();
    for pill in pills {
        if span_width(&right) + span_width(&pill) + 1 > room {
            break;
        }
        right.push(Span::raw(" "));
        right.extend(pill);
    }
    right
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
    // `? help` is reserved at the right, so it shows at any width and
    // beside any status message.
    let help = Line::from(vec![
        Span::styled("?", theme.strong(theme.accent)),
        Span::styled(" keys ", theme.muted()),
    ]);
    let room = (area.width as usize).saturating_sub(help.width() + 1);
    let mut spans = theme.pill(mode, theme.accent);
    spans.push(Span::raw("  "));
    if let Some(status) = &app.status {
        let color = if status.error {
            theme.overdue
        } else {
            theme.done
        };
        spans.push(Span::styled(status.text.clone(), theme.strong(color)));
    } else {
        spans.extend(footer_hints(app, room.saturating_sub(span_width(&spans))));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
    frame.render_widget(Paragraph::new(help).alignment(Alignment::Right), area);
}

fn span_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(Span::width).sum()
}

/// The cursor row's location, the marked count and the most-used keys
/// (`?` lists the rest), as many whole hints as fit in `room` columns.
fn footer_hints(app: &App, room: usize) -> Vec<Span<'static>> {
    let theme = &app.theme;
    let mut spans = Vec::new();
    if let Some(row) = app.selected_row()
        && !matches!(app.screen, Screen::Review)
    {
        spans.push(Span::styled(app.short_location(&row.item), theme.muted()));
        spans.push(Span::styled("  │  ", theme.fg(theme.border)));
    }
    let marked = if app.screen == Screen::Review {
        app.marked_changes.len()
    } else {
        app.marks.len()
    };
    if marked > 0 {
        spans.push(Span::styled(
            format!("{marked} marked  "),
            theme.strong(theme.accent),
        ));
    }
    for (key, action) in keys::footer_hints(app) {
        let hint = theme.key_hint(&key, action);
        if span_width(&spans) + span_width(&hint) > room {
            break;
        }
        spans.extend(hint);
    }
    spans
}

fn list_title(app: &App) -> String {
    match &app.screen {
        Screen::Today if app.classic => {
            let date = app.day.format("%A %-d %B").to_string();
            match relative_day(app) {
                Some(word) => format!("{word} · {date}"),
                None => date,
            }
        }
        Screen::Today => {
            let effort = total_effort(app);
            if effort > 0 {
                format!(
                    "{} · {}:{:02} effort",
                    day_name(app),
                    effort / 60,
                    effort % 60
                )
            } else {
                day_name(app)
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

/// The day `f`/`b` moved the list to: Today, else its date, after
/// Tomorrow or Yesterday when it is one.
fn day_name(app: &App) -> String {
    let date = crate::words::day(app.day, app.today);
    match relative_day(app) {
        Some("Today") => "Today".to_string(),
        Some(word) => format!("{word} · {date}"),
        None => date,
    }
}

fn relative_day(app: &App) -> Option<&'static str> {
    match (app.day - app.today).num_days() {
        0 => Some("Today"),
        1 => Some("Tomorrow"),
        -1 => Some("Yesterday"),
        _ => None,
    }
}

/// Sum of `:Effort:` (h:mm) over today's unfinished rows.
fn total_effort(app: &App) -> u32 {
    app.rows
        .iter()
        .filter(|row| !row.item.done && row.more == 0)
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
    // Rows come in header order, and a name can repeat (each block's
    // "Other items", the same day in two agenda blocks), so each row goes to
    // the next header with its name.
    let mut header_rows: Vec<Vec<usize>> = vec![Vec::new(); app.headers.len()];
    let mut current = 0;
    for (index, row) in app.rows.iter().enumerate() {
        if let Some(offset) = app.headers[current..]
            .iter()
            .position(|header| header.text == row.section)
        {
            current += offset;
            header_rows[current].push(index);
        }
    }
    // A section holds its own rows and those of the day lines under it.
    let section_rows: Vec<Vec<usize>> = (0..app.headers.len())
        .map(|index| {
            (index..app.headers.len())
                .take_while(|next| *next == index || app.headers[*next].day)
                .flat_map(|next| header_rows[next].iter().copied())
                .collect()
        })
        .collect();
    let columns = RowColumns {
        width,
        wide,
        keyword: keyword_width(app),
        next: next_timed(app),
    };
    for ((header, rows), held) in app
        .headers
        .iter()
        .zip(header_rows.iter())
        .zip(&section_rows)
    {
        if rows.is_empty() && app.screen == Screen::Today && !app.classic {
            continue;
        }
        // A blank line above each section, as in the design.
        if !header.day && !items.is_empty() {
            items.push(ListItem::new(Line::default()));
            line_rows.push(None);
        }
        let held = if header.day { rows } else { held };
        items.push(ListItem::new(section_header(app, header, held, width)));
        line_rows.push(None);
        for &index in rows {
            if index == app.selected {
                selected_line = Some(items.len());
            }
            items.push(ListItem::new(row_line(app, index, &columns)));
            line_rows.push(Some(index));
        }
    }
    if app.rows.is_empty() {
        items.extend(empty_message(app).into_iter().map(ListItem::new));
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
        frame.render_stateful_widget(theme.scrollbar(), scrollbar_track(area), &mut scrollbar);
    }
}

/// Why a list is empty, most fundamental reason first: no files at all,
/// then a filter that matches nothing, then the screen's own state.
pub(super) fn empty_message(app: &App) -> Vec<Line<'static>> {
    let theme = &app.theme;
    let muted = |text: String| Line::from(Span::styled(text, theme.muted()));
    if app.index.documents().next().is_none() {
        let root = app
            .settings
            .roots
            .first()
            .map_or(String::new(), |root| home_relative(root));
        // Two lines, so a long root does not push the advice out of view.
        return vec![
            Line::from(format!("No .org files under {root} yet —")),
            Line::from(vec![
                Span::raw("press "),
                Span::styled("c", theme.strong(theme.accent)),
                Span::raw(" to capture your first task"),
            ]),
        ];
    }
    if !app.filter.is_empty() {
        return vec![muted(format!(
            "Nothing matches /{} · esc clears",
            app.filter
        ))];
    }
    if let Some(context) = &app.context {
        return vec![muted(format!(
            "Nothing in {context} · < shows the next category"
        ))];
    }
    match app.screen {
        Screen::Today if app.day == app.today => {
            let (scheduled, due) = app.tomorrow;
            vec![
                Line::from(Span::styled(
                    "All clear for today.",
                    theme.strong(theme.done),
                )),
                muted(format!("Tomorrow: {scheduled} scheduled, {due} due")),
            ]
        }
        Screen::Today => vec![muted(format!(
            "Nothing on {}.",
            crate::words::day(app.day, app.today)
        ))],
        Screen::Inbox => vec![muted("Inbox is empty.".to_string())],
        _ => vec![muted("Nothing here.".to_string())],
    }
}

/// A path with the home directory written as `~`.
fn home_relative(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && path.starts_with(&home) => {
            format!("~{}", &path[home.len()..])
        }
        _ => path.to_string(),
    }
}

/// A section in its color, uppercase, with how many rows it holds; empty
/// sections and days recede. Day lines carry no count, as in org-agenda.
fn section_header(app: &App, header: &Header, rows: &[usize], width: usize) -> Line<'static> {
    let theme = &app.theme;
    let section = header.text.as_str();
    if header.day {
        // Today's date stands out in accent, underlined, as the CLI's does.
        let style = if section == super::rows::day_label(app.today) {
            theme
                .strong(theme.accent)
                .add_modifier(Modifier::UNDERLINED)
        } else if rows.is_empty() {
            theme.muted()
        } else {
            theme.strong(theme.text)
        };
        return Line::from(Span::styled(section.to_string(), style));
    }
    let color = match (header.color.as_deref(), section) {
        (Some(name), _) => theme.named(name),
        (None, OVERDUE) => theme.overdue,
        (None, DUE) => theme.due,
        (None, DOING) => theme.done,
        (None, _) => theme.accent,
    };
    // A "+N more" row stands for N rows.
    let count: usize = rows.iter().map(|index| app.rows[*index].more.max(1)).sum();
    let style = if count == 0 {
        theme.muted().add_modifier(Modifier::BOLD)
    } else {
        theme.strong(color)
    };
    let mut spans = vec![Span::styled(section.to_uppercase(), style)];
    if count > 0 && header.counted {
        spans.push(Span::styled(format!("  {count}"), theme.muted()));
    }
    if section == OVERDUE {
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

/// Cells before a row's lead: its marker (`*` marked, `▸` next) and a space.
const MARKER: usize = 2;
/// The lead cell, which always says when.
const LEAD: usize = 6;
/// Keywords wider than this do not widen the keyword column.
const KEYWORD_MAX: usize = 9;

/// What every row of one list shares: the row width, whether tags fit
/// (WIDE), the keyword column's width and the next timed row.
pub(super) struct RowColumns {
    pub width: usize,
    pub wide: bool,
    pub keyword: usize,
    pub next: Option<usize>,
}

/// Rows of an org-agenda day list (Week, the classic view, agenda blocks)
/// keep Org's prefix; the rest are sectioned rows with a lead cell.
fn classic_row(row: &DisplayRow) -> bool {
    !matches!(
        row.section.as_str(),
        OVERDUE | SCHEDULE | DUE | DOING | INBOX
    ) && row.occurrence.is_some()
}

/// The keyword column of the sectioned rows: the widest keyword among
/// them, at most KEYWORD_MAX; 0 when none has one.
fn keyword_width(app: &App) -> usize {
    app.rows
        .iter()
        .filter(|row| !classic_row(row))
        .filter_map(|row| row.item.todo.as_deref())
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0)
        .min(KEYWORD_MAX)
}

/// One task line: the marker, then Org's prefix (category, time, leader)
/// or the lead cell (badge, time or countdown), the TODO keyword, the
/// title, priority and tags, and a right-aligned context column.
fn row_line(app: &App, index: usize, columns: &RowColumns) -> Line<'static> {
    let theme = &app.theme;
    let row = &app.rows[index];
    let clock = clock_of(app, index, columns.next);
    if row.more > 0 {
        return Line::from(vec![
            Span::raw(" ".repeat(MARKER + LEAD + 1)),
            Span::styled(format!("+{} more", row.more), theme.strong(theme.overdue)),
            Span::styled("  ⏎ shows them", theme.muted()),
        ]);
    }
    let item = &row.item;
    let mut spans = vec![marker(app, row, clock)];
    if classic_row(row) {
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
        if let Some(todo) = &item.todo {
            spans.push(Span::styled(format!("{todo} "), keyword_style(app, row)));
        }
    } else {
        // The lead is padded to six columns but may fill them (`in 14d`,
        // a rounded badge); the separator keeps it off the keyword.
        spans.extend(lead_cell(app, row, clock));
        spans.push(Span::raw(" "));
        if columns.keyword > 0 {
            let todo = item.todo.as_deref().unwrap_or_default();
            spans.push(Span::styled(
                format!("{todo:<width$} ", width = columns.keyword),
                keyword_style(app, row),
            ));
        }
    }
    let title_style = if item.done {
        theme.done_row()
    } else {
        theme.fg(theme.text)
    };
    let mut priority = Vec::new();
    if let Some(letter) = &item.priority {
        priority.push(Span::raw(" "));
        priority.extend(theme.pill(letter.clone(), theme.due));
    }
    let tags = (columns.wide && !item.tags.is_empty())
        .then(|| Span::styled(format!("  #{}", item.tags.join(" #")), theme.muted()));
    let used = span_width(&spans);
    spans.extend(fit_row(
        columns.width,
        used,
        Span::styled(peek::display_title(item), title_style),
        priority,
        tags,
        right_cell(app, row),
    ));
    let line = Line::from(spans);
    if item.done {
        line.style(Style::default().add_modifier(Modifier::DIM))
    } else {
        line
    }
}

/// The title, priority, tags and right column in the `width - used`
/// columns left: the right column gives way first, then the tags (when
/// they would leave the title under MIN_TITLE columns), and only then is
/// the title cut, ending in "…".
fn fit_row(
    width: usize,
    used: usize,
    title: Span<'static>,
    priority: Vec<Span<'static>>,
    tags: Option<Span<'static>>,
    right: Vec<Span<'static>>,
) -> Vec<Span<'static>> {
    const MIN_TITLE: usize = 12;
    let title_width = title.width();
    let tags_width = tags.as_ref().map_or(0, Span::width);
    let right_width = span_width(&right);
    let base = used + span_width(&priority);
    let keep_right = base + title_width + tags_width + right_width + 2 <= width;
    let keep_tags = keep_right || base + title_width.min(MIN_TITLE) + tags_width <= width;
    let room = width.saturating_sub(base + if keep_tags { tags_width } else { 0 });
    let mut spans = vec![Span::styled(truncate(&title.content, room), title.style)];
    spans.extend(priority);
    if keep_tags {
        spans.extend(tags);
    }
    if keep_right {
        let filled = span_width(&spans) + used;
        spans.push(Span::raw(
            " ".repeat(width.saturating_sub(filled + right_width)),
        ));
        spans.extend(right);
    }
    spans
}

/// Where a timed row stands against the clock on today's list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Clock {
    Past,
    Next,
    Later,
}

/// The clock time a row's lead shows, if it shows one (`16:30`, or the
/// start of `16:30-17:00`).
fn lead_time(row: &DisplayRow) -> Option<NaiveTime> {
    let occurrence = row.occurrence.as_ref()?;
    if matches!(
        occurrence.kind,
        OccurrenceType::PastScheduled | OccurrenceType::Deadline | OccurrenceType::UpcomingDeadline
    ) {
        return None;
    }
    let time = occurrence.time.as_deref()?.split('-').next()?;
    NaiveTime::parse_from_str(time, "%H:%M").ok()
}

/// The next timed task today: the first open row whose time is now or
/// later. None on another day.
pub(super) fn next_timed(app: &App) -> Option<usize> {
    if !app.pane && app.day != app.today {
        return None;
    }
    let now = minute(app.clock);
    app.rows
        .iter()
        .position(|row| !row.item.done && lead_time(row).is_some_and(|time| time >= now))
}

/// The clock to the minute, as Org writes times: a task at 15:00 is next
/// all through 15:00.
fn minute(time: NaiveTime) -> NaiveTime {
    NaiveTime::from_hms_opt(time.hour(), time.minute(), 0).unwrap_or(time)
}

/// Where row `index` stands against the clock, `next` being next_timed.
pub(super) fn clock_of(app: &App, index: usize, next: Option<usize>) -> Clock {
    let today = app.pane || app.day == app.today;
    match lead_time(&app.rows[index]) {
        _ if next == Some(index) => Clock::Next,
        Some(time) if today && time < minute(app.clock) => Clock::Past,
        _ => Clock::Later,
    }
}

/// Six-character lead: overdue days as a red badge, clock time, a
/// deadline countdown, or how long a WAIT has waited. A time already past
/// is muted and the next one bold.
pub(super) fn lead_cell(app: &App, row: &DisplayRow, clock: Clock) -> Vec<Span<'static>> {
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
            // The age in days: `12x` would read as "twelve times" beside
            // "rescheduled 3x". The classic view keeps Org's `Sched.12x:`.
            badge(theme, format!("{days}d"))
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
                let style = match clock {
                    _ if row.item.done => theme.muted(),
                    Clock::Past => theme.muted(),
                    Clock::Next => theme.strong(theme.accent),
                    Clock::Later => theme.fg(theme.accent),
                };
                vec![Span::styled(format!("{time:<6}"), style)]
            }
            None => vec![Span::raw(" ".repeat(6))],
        },
    }
}

/// A red overdue badge filling the 6-column lead cell: a 4-column label,
/// right-aligned so the days line up, between its caps or padding.
fn badge(theme: &Theme, text: String) -> Vec<Span<'static>> {
    theme.pill(format!("{text:>4}"), theme.overdue)
}

fn lead_style(theme: &Theme, kind: OccurrenceType, extra: &str) -> Style {
    match kind {
        OccurrenceType::PastScheduled => theme.fg(theme.overdue),
        OccurrenceType::Deadline if extra.ends_with("ago:") => theme.fg(theme.overdue),
        OccurrenceType::Deadline | OccurrenceType::UpcomingDeadline => theme.fg(theme.due),
        _ => theme.muted(),
    }
}

/// Done keywords green, the first keyword of a sequence amber, middle
/// states (NEXT, WAIT, …) in accent.
pub(super) fn keyword_style(app: &App, row: &DisplayRow) -> Style {
    let name = keyword_color(&app.index, &row.item);
    let color = app.theme.named(&name);
    // Done keywords and a sequence's first keyword (TODO) are not bold.
    if row.item.done || name == "orange" {
        app.theme.fg(color)
    } else {
        app.theme.strong(color)
    }
}

/// The marker cell: `*` on a marked row, `▸` on the next timed task.
pub(super) fn marker(app: &App, row: &DisplayRow, clock: Clock) -> Span<'static> {
    if app.marks.contains(&row.item.item_ref) {
        Span::styled("* ", app.theme.strong(app.theme.accent))
    } else if clock == Clock::Next {
        Span::styled("▸ ", app.theme.strong(app.theme.accent))
    } else {
        Span::raw(" ".repeat(MARKER))
    }
}

/// Category for overdue rows, the repeat interval for repeating tasks, and
/// who captured an inbox item; at most RIGHT cells and a closing space.
/// Done rows show their keyword, so they need no ✓.
fn right_cell(app: &App, row: &DisplayRow) -> Vec<Span<'static>> {
    let theme = &app.theme;
    let item = &row.item;
    if item.done {
        return Vec::new();
    }
    if row.section == INBOX {
        return match app.provenance.get(&super::stable_ref(&item.item_ref)) {
            Some((actor, _)) if actor != "you" => {
                let mut spans = agent_label(theme, actor, RIGHT);
                spans.push(Span::raw(" "));
                spans
            }
            Some(_) => vec![Span::styled("you ", theme.muted())],
            None => Vec::new(),
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
        return vec![Span::styled(
            format!("↻ {} ", interval.split('/').next().unwrap_or(interval)),
            theme.muted(),
        )];
    }
    if row.section == OVERDUE || row.section == DOING {
        return vec![Span::styled(
            format!("{} ", truncate(&item.category, RIGHT)),
            theme.muted(),
        )];
    }
    Vec::new()
}

/// `● codex` as provenance: the dot in the agent's hue, the name muted
/// like all metadata, cut to `width` cells with "…".
fn agent_label(theme: &Theme, agent: &str, width: usize) -> Vec<Span<'static>> {
    vec![
        Span::styled("● ", theme.fg(theme.agent(agent))),
        Span::styled(truncate(agent, width.saturating_sub(2)), theme.muted()),
    ]
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

/// What a prompt is for, and examples of what it takes.
fn prompt_text(app: &App, kind: PromptKind) -> (&'static str, String) {
    let date = actions::example_date(app.today);
    let range = app.prompt_priority_range();
    match kind {
        PromptKind::Schedule => ("Schedule", format!("tomorrow · fri · +2d · {date} 14:00")),
        PromptKind::Deadline => ("Deadline", format!("fri · +2d · {date} -3d")),
        PromptKind::Priority => (
            "Priority",
            format!("{} to {}; empty clears", range.highest, range.lowest),
        ),
        PromptKind::Tags => ("Tags", "+tag adds · -tag removes".to_string()),
        PromptKind::Filter => ("Filter", "text or tag; empty clears".to_string()),
        PromptKind::Note => (
            "Note",
            "one line, logged with the change; empty logs it alone".to_string(),
        ),
    }
}

/// A prompt: the input, the exact change Enter writes (or why it would
/// write nothing), examples and keys.
fn draw_prompt(frame: &mut Frame<'_>, app: &App, prompt: &actions::Prompt, area: Rect) {
    let theme = &app.theme;
    let (title, hint) = prompt_text(app, prompt.kind);
    let target = if app.marks.is_empty() {
        app.selected_row()
            .map(|row| app.short_location(&row.item))
            .unwrap_or_default()
    } else {
        format!("{} marked rows", app.marks.len())
    };
    let title = if prompt.kind == PromptKind::Filter || target.is_empty() {
        title.to_string()
    } else if prompt.kind == PromptKind::Note {
        format!("{title} for {} · {target}", prompt.state)
    } else {
        format!("{title} · {target}")
    };
    let preview: Vec<Line<'static>> = match &prompt.preview {
        Ok(lines) => lines
            .iter()
            .map(|line| {
                let style = match line.chars().next() {
                    Some('+') => theme.fg(theme.done),
                    Some('-') => theme.fg(theme.overdue),
                    _ => theme.muted(),
                };
                Line::from(Span::styled(line.clone(), style))
            })
            .collect(),
        Err(reason) => vec![Line::from(Span::styled(
            reason.clone(),
            theme.fg(theme.overdue),
        ))],
    };
    let preview_rows = preview.len() as u16;
    let gap = u16::from(preview_rows > 0);
    // border + top padding, input, preview, gap, hint, gap, footer, border
    let height = 2 + 3 + preview_rows + gap + 1 + 1 + 1 + 1;
    let popup = centered(area, area.width.saturating_sub(4).clamp(60, 84), height);
    frame.render_widget(Clear, popup);
    let block = theme.popup(title);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([3, preview_rows, gap, 1, 1, 1].map(Constraint::Length))
        .split(inner);
    frame.render_widget(
        Paragraph::new(input_line(theme, &prompt.input)).block(theme.input(true)),
        parts[0],
    );
    frame.render_widget(Paragraph::new(preview), parts[1]);
    frame.render_widget(Paragraph::new(Span::styled(hint, theme.muted())), parts[3]);
    frame.render_widget(
        Paragraph::new(Line::from(
            [
                theme.key_hint("⏎", "apply"),
                theme.key_hint("esc", "cancel"),
            ]
            .concat(),
        )),
        parts[5],
    );
}

/// `/` as a minibuffer on the bottom row: the text, a count of what
/// matches, and its keys.
fn draw_filter_line(frame: &mut Frame<'_>, app: &App, prompt: &actions::Prompt, area: Rect) {
    let theme = &app.theme;
    let row = Rect {
        y: area.y + area.height.saturating_sub(1),
        height: 1,
        ..area
    };
    let mut spans = vec![Span::styled("/", theme.strong(theme.accent))];
    spans.extend(prompt.input.spans(theme, true));
    spans.push(Span::styled(
        format!("  {} rows  ", app.rows.len()),
        theme.muted(),
    ));
    spans.extend(theme.key_hint("⏎", "keep"));
    spans.extend(theme.key_hint("esc", "restore"));
    frame.render_widget(Clear, row);
    frame.render_widget(Paragraph::new(Line::from(spans)), row);
}

/// A focused one-line input after its `›` marker.
fn input_line(theme: &Theme, input: &TextInput) -> Line<'static> {
    let mut spans = vec![Span::styled("› ", theme.strong(theme.accent))];
    spans.extend(input.spans(theme, true));
    Line::from(spans)
}

/// `B`: the bulk menu, one row per key from keys::BULK_KEYS.
fn draw_bulk(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = &app.theme;
    let bindings = keys::BULK_KEYS[0].bindings;
    let rows: Vec<Line<'static>> = bindings
        .iter()
        .map(|binding| {
            Line::from(vec![
                Span::styled(format!("{:<9}", binding.label), theme.strong(theme.accent)),
                Span::raw(binding.help),
            ])
        })
        .collect();
    // border + top padding, rows, border
    let popup = centered(area, 52, 2 + rows.len() as u16 + 1);
    frame.render_widget(Clear, popup);
    let block = theme.popup(format!("Bulk · {} marked rows", app.marks.len()));
    frame.render_widget(Paragraph::new(rows).block(block), popup);
}

/// Prompts, the refile picker and the views dispatcher.
pub(super) fn draw_overlay(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = &app.theme;
    // The filter applies as you type, so it stays out of the list's way:
    // a minibuffer on the footer row, nothing dimmed.
    if let Overlay::Prompt(prompt) = &app.overlay
        && prompt.kind == PromptKind::Filter
    {
        return draw_filter_line(frame, app, prompt, area);
    }
    frame
        .buffer_mut()
        .set_style(area, Style::default().add_modifier(Modifier::DIM));
    let footer = |hints: &[(&str, &str)]| {
        Line::from(
            hints
                .iter()
                .flat_map(|(key, action)| theme.key_hint(key, action))
                .collect::<Vec<_>>(),
        )
    };
    match &app.overlay {
        Overlay::Prompt(prompt) => draw_prompt(frame, app, prompt, area),
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
                Paragraph::new(input_line(theme, &picker.input)).block(theme.input(true)),
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
        Overlay::Bulk => draw_bulk(frame, app, area),
        Overlay::Views { selected } => {
            let views = app.settings.all_views();
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
                Paragraph::new(footer(&[
                    ("key", "opens its view"),
                    ("↑↓ ⏎", "choose"),
                    ("esc q", "close"),
                ])),
                parts[2],
            );
        }
        _ => {}
    }
}
