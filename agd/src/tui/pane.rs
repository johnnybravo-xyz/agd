use super::theme::{accent, rounded};
use super::{App, DisplayRow};
use chrono::{Local, NaiveDate};
use org_core::OccurrenceType;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub(super) fn draw_pane(frame: &mut ratatui::Frame<'_>, app: &mut App, area: Rect) {
    app.list_area = area;
    let today = Local::now().date_naive();
    let overdue = app.slipped_count;
    let mut lines = vec![Line::from(Span::styled(
        format!(" TODAY  {}", today.format("%a %d %b")),
        Style::default().fg(accent()).add_modifier(Modifier::BOLD),
    ))];
    let visible: Vec<(usize, &DisplayRow)> = app
        .rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.section != "SLIPPED")
        .collect();
    for (index, row) in visible.iter().take(area.height.saturating_sub(5) as usize) {
        let marker = if *index == app.selected { "▸" } else { " " };
        let when = row
            .occurrence
            .as_ref()
            .and_then(|occurrence| {
                occurrence.time.clone().or_else(|| {
                    if matches!(
                        occurrence.kind,
                        OccurrenceType::Deadline | OccurrenceType::UpcomingDeadline
                    ) {
                        let days = occurrence
                            .date
                            .parse::<NaiveDate>()
                            .ok()
                            .map(|date| date.signed_duration_since(today).num_days())
                            .unwrap_or_default();
                        Some(if days == 0 {
                            "due".to_string()
                        } else {
                            format!("in {days}d")
                        })
                    } else {
                        None
                    }
                })
            })
            .unwrap_or_default();
        let style = if row.item.done {
            Style::default().add_modifier(Modifier::DIM)
        } else {
            Style::default()
        };
        lines.push(Line::from(format!("{marker} {when:<5} {}", row.item.title)).style(style));
    }
    if visible.is_empty() {
        lines.push(Line::from(format!(
            "Nothing scheduled today. {} in inbox.",
            app.inbox_count
        )));
    }
    lines.push(Line::from(format!(
        "── {overdue} overdue · {} inbox · {} agent",
        app.inbox_count, app.agent_count
    )));
    lines.push(Line::from("j/k  x done  c capture  T triage  ⏎ full"));
    frame.render_widget(Paragraph::new(lines).block(rounded("Agenda")), area);
}
