use super::review::draw_review;
use super::rows::{occurrence_prefix, view_title};
use super::theme::{accent, no_color, rounded, section_color, selection_style, tab_style};
use super::triage::draw_triage;
use super::{AgendaView, App, Screen};
use chrono::{Local, NaiveDate};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Scrollbar,
    ScrollbarOrientation, ScrollbarState, Wrap,
};

pub(super) fn draw_full(frame: &mut ratatui::Frame<'_>, app: &mut App, area: Rect) {
    let accent = accent();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(area);
    draw_header(frame, app, chunks[0]);

    let body = if app.peek && area.width >= 100 && app.screen != Screen::Review {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
            .split(chunks[1])
    } else {
        vec![chunks[1]].into()
    };
    app.list_area = body[0];
    if app.screen == Screen::Review {
        draw_review(frame, app, body[0]);
    } else if app.screen == Screen::Triage {
        draw_triage(frame, app, body[0]);
    } else {
        draw_rows(frame, app, body[0]);
    }
    if body.len() > 1 {
        draw_peek(frame, app, body[1]);
    }
    let selected = app
        .selected_row()
        .map(|row| format!("{}:{}", row.item.file, row.item.line))
        .unwrap_or_default();
    let context = ["all", "work", "personal"][app.context];
    let status = if app.status.is_empty() {
        if app.screen == Screen::Review {
            "AGENTS   a accept   u undo   d diff   A accept all".to_string()
        } else {
            format!(
                "ctx:{context}  {selected}  {} inbox · {} agent   j/k move  t state  s sched  c capture  ? keys",
                app.inbox_count, app.agent_count
            )
        }
    } else {
        app.status.clone()
    };
    frame.render_widget(
        Paragraph::new(status).style(Style::default().fg(accent)),
        chunks[2],
    );
}

pub(super) fn draw_header(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(30), Constraint::Length(26)])
        .split(area);
    let left = if app.screen == Screen::Triage {
        Line::from(vec![
            Span::styled(" TRIAGE ", tab_style(true, Color::Yellow)),
            Span::styled(
                " overdue, then inbox",
                Style::default().add_modifier(Modifier::DIM),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(" Agenda ", tab_style(true, accent())),
            Span::raw("  "),
            Span::styled(
                " Today ",
                tab_style(
                    app.screen == Screen::Today && app.view == AgendaView::Day,
                    accent(),
                ),
            ),
            Span::raw(" "),
            Span::styled(
                " Week ",
                tab_style(
                    app.screen == Screen::Today && app.view == AgendaView::Week,
                    accent(),
                ),
            ),
            Span::raw(" "),
            Span::styled(
                format!(" Inbox {} ", app.inbox_count),
                tab_style(false, accent()),
            ),
            Span::raw(" "),
            Span::styled(
                format!(" Agents {} ", app.agent_count),
                tab_style(app.screen == Screen::Review, accent()),
            ),
        ])
    };
    frame.render_widget(Paragraph::new(left), columns[0]);
    let right = if app.screen == Screen::Triage {
        let completed: usize = app.triage_outcomes.iter().sum();
        let total = completed + app.rows.len();
        format!("{} / {total}", (completed + 1).min(total))
    } else {
        format!(
            "ctx {}   {}",
            ["all", "work", "personal"][app.context],
            app.day.format("%a %d %b")
        )
    };
    frame.render_widget(
        Paragraph::new(right).alignment(ratatui::layout::Alignment::Right),
        columns[1],
    );
}

pub(super) fn draw_rows(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let list_area = area;
    let mut previous = "";
    let mut items = Vec::new();
    for row in &app.rows {
        let mut lines = Vec::new();
        if !app.classic && row.section != previous {
            let count = if row.section == "SLIPPED" {
                app.slipped_count
            } else {
                app.rows
                    .iter()
                    .filter(|candidate| candidate.section == row.section)
                    .count()
            };
            lines.push(Line::from(Span::styled(
                format!(" {}  {count} ", row.section),
                Style::default()
                    .fg(section_color(&row.section))
                    .add_modifier(Modifier::BOLD),
            )));
            previous = &row.section;
        }
        let mark = if app.marks.contains(&row.item.item_ref) {
            "●"
        } else {
            " "
        };
        let prefix = occurrence_prefix(row.occurrence.as_ref());
        let time = row
            .occurrence
            .as_ref()
            .and_then(|occurrence| occurrence.time.as_deref())
            .unwrap_or("");
        let todo = row.item.todo.as_deref().unwrap_or("");
        let waiting = if todo == "WAIT" {
            row.occurrence
                .as_ref()
                .and_then(|occurrence| occurrence.date.parse::<NaiveDate>().ok())
                .map(|date| {
                    format!(
                        "  waiting {}d",
                        Local::now()
                            .date_naive()
                            .signed_duration_since(date)
                            .num_days()
                            .max(0)
                    )
                })
                .unwrap_or_default()
        } else {
            String::new()
        };
        let detail = if row.section == "INBOX" {
            app.provenance
                .get(&row.item.item_ref)
                .map(|actor| format!("  {}", actor.strip_prefix("mcp:").unwrap_or(actor)))
                .unwrap_or_else(|| "  you".to_string())
        } else if area.width >= 100 && !row.item.tags.is_empty() {
            format!("  #{}", row.item.tags.join(" #"))
        } else {
            String::new()
        };
        lines.push(Line::from(format!(
            "{mark} {prefix:<12} {time:<5} {todo:<9} {}{waiting}{detail}",
            row.item.title
        )));
        let style = if row.item.done {
            Style::default().add_modifier(Modifier::DIM)
        } else if !no_color() && row.item.priority.as_deref() == Some("A") {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        items.push(ListItem::new(lines).style(style));
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(view_title(app));
    let mut state = ListState::default().with_selected(Some(app.selected));
    let list = List::new(items)
        .block(block)
        .highlight_symbol("▌")
        .highlight_style(selection_style());
    frame.render_stateful_widget(list, list_area, &mut state);
    if app.rows.len() > list_area.height.saturating_sub(2) as usize {
        let mut scrollbar = ScrollbarState::new(app.rows.len()).position(app.selected);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            list_area,
            &mut scrollbar,
        );
    }
}

pub(super) fn draw_peek(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let text = app
        .selected_row()
        .and_then(|row| {
            let (document, headline) = app.index.resolve(&row.item.item_ref).ok()?;
            let item = &headline.item;
            let body = document.source()[headline.line_span.end..headline.subtree_span.end]
                .trim();
            Some(format!(
                "{}\n{} › {}\n\nScheduled      {}\nDeadline       {}\nPriority       {}\nTags           {}\n\n────────────────\n{}",
                item.title,
                item.file,
                item.olpath.join(" › "),
                item.planning
                    .scheduled
                    .as_ref()
                    .map(|timestamp| timestamp.date.as_str())
                    .unwrap_or("—"),
                item.planning
                    .deadline
                    .as_ref()
                    .map(|timestamp| timestamp.date.as_str())
                    .unwrap_or("—"),
                item.priority.as_deref().unwrap_or("—"),
                if item.tags.is_empty() {
                    "—".to_string()
                } else {
                    item.tags.join(", ")
                },
                body,
            ))
        })
        .unwrap_or_else(|| "No item selected".to_string());
    frame.render_widget(
        Paragraph::new(text)
            .block(rounded("Peek · e edit"))
            .wrap(Wrap { trim: false }),
        area,
    );
}
