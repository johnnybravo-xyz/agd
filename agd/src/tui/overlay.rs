use std::fs;
use std::io::{self, Stdout, Write};
use std::process::Command;

use super::actions::fuzzy_match;
use super::help::draw_help;
use super::stop_terminal;
use super::theme::{accent, centered, rounded};
use super::today::draw_peek;
use super::{App, Overlay, PromptKind};
use anyhow::{Context, Result};
use chrono::Local;
use crossterm::event::EnableMouseCapture;
use crossterm::execute;
use crossterm::terminal::{EnterAlternateScreen, enable_raw_mode};
use org_core::parse_capture_line;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Clear, List, ListItem, ListState, Paragraph, Wrap};

pub(super) fn draw_overlay(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    match &app.overlay {
        Overlay::None => {
            if app.peek && area.width < 100 && !app.pane {
                let popup = centered(area, 86, 70);
                frame.render_widget(Clear, popup);
                draw_peek(frame, app, popup);
            }
        }
        Overlay::Help => draw_help(frame, area),
        Overlay::Capture {
            template,
            input,
            preview,
            body,
        } => {
            let popup = centered(area, 86, 72);
            frame.render_widget(Clear, popup);
            let draft = parse_capture_line(input, Local::now().date_naive()).ok();
            let mut chips = Vec::new();
            if let Some(draft) = &draft {
                if let Some(deadline) = &draft.deadline {
                    chips.push(format!("deadline {}", deadline.date));
                }
                if let Some(scheduled) = &draft.scheduled {
                    chips.push(format!("scheduled {}", scheduled.date));
                }
                chips.extend(draft.tags.iter().map(|tag| format!("+{tag}")));
                if let Some(priority) = &draft.priority {
                    chips.push(format!("#{priority}"));
                }
            }
            let capture = template
                .as_ref()
                .and_then(|key| app.settings.capture.get(key));
            let destination = capture
                .map(|capture| {
                    format!(
                        "{} › {}",
                        capture.file.as_deref().unwrap_or("unsupported target"),
                        capture
                            .heading
                            .as_deref()
                            .or_else(|| {
                                capture
                                    .outline_path
                                    .as_ref()
                                    .and_then(|path| path.last().map(String::as_str))
                            })
                            .unwrap_or("top level")
                    )
                })
                .unwrap_or_else(|| "choose a template".to_string());
            let title = capture
                .map(|capture| format!("Capture · {}", capture.desc))
                .unwrap_or_else(|| "Capture".to_string());
            let text = format!(
                "> {}\n{}\n→ {}\nbody: {} bytes · ctrl-e editor\n\nExact write preview\n{}\n\n⏎ save   ctrl-e editor   esc cancel",
                input,
                chips.join("   "),
                destination,
                body.len(),
                preview
            );
            frame.render_widget(
                Paragraph::new(text)
                    .block(rounded(&title))
                    .wrap(Wrap { trim: false }),
                popup,
            );
        }
        Overlay::Prompt { kind, input } => {
            let popup = centered(area, 70, 20);
            frame.render_widget(Clear, popup);
            frame.render_widget(
                Paragraph::new(format!("> {input}")).block(rounded(match kind {
                    PromptKind::Schedule => "Schedule (date, +2d, fri)",
                    PromptKind::Deadline => "Deadline (date, +2d, fri)",
                    PromptKind::Priority => "Priority",
                    PromptKind::Tags => "Tags (+tag,-tag)",
                    PromptKind::Filter => "Filter",
                })),
                popup,
            );
        }
        Overlay::Refile {
            input,
            selected,
            targets,
        } => {
            let popup = centered(area, 86, 70);
            frame.render_widget(Clear, popup);
            let items: Vec<ListItem<'_>> = targets
                .iter()
                .filter(|target| fuzzy_match(&target.label, input))
                .map(|target| ListItem::new(target.label.clone()))
                .collect();
            let mut state = ListState::default().with_selected(Some(*selected));
            frame.render_stateful_widget(
                List::new(items)
                    .block(rounded(&format!("Refile · {input}")))
                    .highlight_symbol("▌")
                    .highlight_style(Style::default().fg(accent()).add_modifier(Modifier::BOLD)),
                popup,
                &mut state,
            );
        }
    }
}

pub(super) fn edit_capture_body(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
) -> Result<()> {
    let Overlay::Capture { body, .. } = &app.overlay else {
        return Ok(());
    };
    let mut file = tempfile::NamedTempFile::new().context("cannot create capture draft")?;
    file.write_all(body.as_bytes())?;
    file.flush()?;
    let args = app
        .settings
        .editor
        .args_for(&file.path().to_string_lossy(), 1, 1);
    stop_terminal(terminal)?;
    let status = Command::new(&app.settings.editor.command)
        .args(args)
        .status()
        .with_context(|| format!("cannot start editor {}", app.settings.editor.command));
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    terminal.clear()?;
    let status = status?;
    if status.success() {
        let updated = fs::read_to_string(file.path()).context("cannot read capture draft")?;
        if let Overlay::Capture { body, .. } = &mut app.overlay {
            *body = updated;
        }
        app.refresh_capture_preview();
    } else {
        app.set_status(format!("editor exited with {status}"));
    }
    Ok(())
}
