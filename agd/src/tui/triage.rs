use super::App;
use super::theme::{accent, key_chip, no_color, rounded};
use chrono::{Local, NaiveDate};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Gauge, Paragraph, Wrap};

pub(super) fn draw_triage(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(8),
            Constraint::Length(2),
            Constraint::Length(2),
        ])
        .split(area);
    let completed: usize = app.triage_outcomes.iter().sum();
    let total = completed + app.rows.len();
    frame.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(accent()))
            .ratio(if total == 0 {
                1.0
            } else {
                completed as f64 / total as f64
            }),
        parts[0],
    );
    let Some(row) = app.selected_row() else {
        let elapsed = app
            .triage_started
            .map(|started| started.elapsed().as_secs())
            .unwrap_or_default();
        let summary = format!(
            "Triage done, {completed} items in {}m {}s: {} today, {} tomorrow, {} next week, {} someday, {} done, {} refiled.\n\nu undo all   ⏎ Today",
            elapsed / 60,
            elapsed % 60,
            app.triage_outcomes[0],
            app.triage_outcomes[1],
            app.triage_outcomes[2],
            app.triage_outcomes[3],
            app.triage_outcomes[4],
            app.triage_outcomes[5],
        );
        frame.render_widget(
            Paragraph::new(summary)
                .block(rounded("Triage complete"))
                .wrap(Wrap { trim: false }),
            Rect {
                height: parts[1].height + parts[2].height + parts[3].height,
                ..parts[1]
            },
        );
        return;
    };
    let today = Local::now().date_naive();
    let days = row
        .occurrence
        .as_ref()
        .and_then(|occurrence| occurrence.date.parse::<NaiveDate>().ok())
        .map(|date| today.signed_duration_since(date).num_days().max(0))
        .unwrap_or_default();
    let reschedules = app
        .reschedule_counts
        .get(&row.item.item_ref)
        .copied()
        .unwrap_or_default();
    let age = if row.section == "INBOX" {
        "Inbox".to_string()
    } else {
        format!("Slipped · {days} days")
    };
    let scheduled = row
        .item
        .planning
        .scheduled
        .as_ref()
        .map(|timestamp| timestamp.date.as_str())
        .unwrap_or("none");
    let deadline = row
        .item
        .planning
        .deadline
        .as_ref()
        .map(|timestamp| timestamp.date.as_str())
        .unwrap_or("no deadline");
    let tags = if row.item.tags.is_empty() {
        String::new()
    } else {
        format!("  #{}", row.item.tags.join(" #"))
    };
    let mut card = vec![
        Line::from(vec![
            Span::styled(
                format!("{:<9}", row.item.todo.as_deref().unwrap_or("")),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                row.item.title.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(tags, Style::default().fg(accent())),
        ]),
        Line::from(Span::styled(
            format!("{} · {}", row.item.file, row.item.olpath.join(" › ")),
            Style::default().add_modifier(Modifier::DIM),
        )),
        Line::from(format!(
            "scheduled {scheduled}   rescheduled {reschedules}x   {deadline}"
        )),
    ];
    if reschedules >= app.settings.triage.reschedule_nudge {
        card.push(Line::from(Span::styled(
            format!("↳ Rescheduled {reschedules} times. Someday?  [s]"),
            Style::default().fg(accent()),
        )));
    }
    let mut block = rounded(&age);
    if !no_color() && row.section != "INBOX" {
        block = block.border_style(Style::default().fg(Color::Red));
    }
    frame.render_widget(
        Paragraph::new(card).block(block).wrap(Wrap { trim: false }),
        parts[1],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            key_chip("."),
            Span::raw(" today   "),
            key_chip("m"),
            Span::raw(" tomorrow   "),
            key_chip("w"),
            Span::raw(" next week   "),
            key_chip("p"),
            Span::raw(" pick date   "),
            key_chip("x"),
            Span::raw(" done"),
        ])),
        parts[2],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            key_chip("n"),
            Span::raw(" NEXT   "),
            key_chip("s"),
            Span::raw(" someday   "),
            key_chip("k"),
            Span::raw(" cancel   "),
            key_chip("r"),
            Span::raw(" refile   "),
            key_chip("u"),
            Span::raw(" undo"),
        ])),
        parts[3],
    );
}
