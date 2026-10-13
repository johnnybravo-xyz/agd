use std::io::Stdout;
use std::time::Instant;

use super::actions::fuzzy_match;
use super::open_selected;
use super::overlay::edit_capture_body;
use super::rows::period_duration;
use super::{AgendaView, App, Overlay, PromptKind, Screen};
use anyhow::Result;
use chrono::Local;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

pub(super) fn handle_key(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    key: KeyEvent,
) -> Result<()> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.quit = true;
        return Ok(());
    }
    if matches!(app.overlay, Overlay::Capture { .. })
        && key.modifiers.contains(KeyModifiers::CONTROL)
        && key.code == KeyCode::Char('e')
    {
        return edit_capture_body(terminal, app);
    }
    if !matches!(app.overlay, Overlay::None) {
        return handle_overlay(app, key);
    }
    if app.pending_view_key {
        app.pending_view_key = false;
        match key.code {
            KeyCode::Char('d') => app.view = AgendaView::Day,
            KeyCode::Char('w') => app.view = AgendaView::Week,
            KeyCode::Char('t') => app.view = AgendaView::Todo,
            KeyCode::Esc => return Ok(()),
            _ => app.set_status("view keys: d day · w week · t todo".to_string()),
        }
        app.reload_rows()?;
        return Ok(());
    }

    match key.code {
        KeyCode::Char('q') => app.quit = true,
        KeyCode::Esc => {
            if app.screen != Screen::Today {
                app.switch_screen(Screen::Today)?;
            } else if app.peek {
                app.peek = false;
            }
        }
        KeyCode::Char('k') if app.screen == Screen::Triage => app.cancel_selected()?,
        KeyCode::Char('p') if app.screen == Screen::Triage => {
            app.overlay = Overlay::Prompt {
                kind: PromptKind::Schedule,
                input: String::new(),
            }
        }
        KeyCode::Char('n') if app.screen == Screen::Triage => app.apply_state("NEXT")?,
        KeyCode::Char('A') if app.screen == Screen::Review => app.review_all()?,
        KeyCode::Char('A') => {
            app.review_diff = false;
            app.switch_screen(Screen::Review)?;
        }
        KeyCode::Char('j') | KeyCode::Down => app.move_selection(1),
        KeyCode::Char('k') | KeyCode::Up => app.move_selection(-1),
        KeyCode::Char('?') => app.overlay = Overlay::Help,
        KeyCode::Char('p') => app.peek = !app.peek,
        KeyCode::Char('T') => {
            app.triage_started = Some(Instant::now());
            app.triage_outcomes = [0; 6];
            app.triage_undo.clear();
            app.switch_screen(Screen::Triage)?;
        }
        KeyCode::Char('c') => {
            app.overlay = Overlay::Capture {
                template: None,
                input: String::new(),
                preview: "type a capture template key".to_string(),
                body: String::new(),
            }
        }
        KeyCode::Char('v') => {
            app.classic = !app.classic;
            app.reload_rows()?;
        }
        KeyCode::Char('V') => app.pending_view_key = true,
        KeyCode::Char('f') => {
            app.day += period_duration(app.view);
            app.reload_rows()?;
        }
        KeyCode::Char('b') => {
            app.day -= period_duration(app.view);
            app.reload_rows()?;
        }
        KeyCode::Char('.') if app.screen == Screen::Triage || !app.marks.is_empty() => {
            app.apply_date_to_targets("today")?;
        }
        KeyCode::Char('.') => {
            app.day = Local::now().date_naive();
            app.reload_rows()?;
        }
        KeyCode::Char('m') if app.screen == Screen::Triage || !app.marks.is_empty() => {
            app.apply_date_to_targets("+1d")?
        }
        KeyCode::Char('w') if app.screen == Screen::Triage || !app.marks.is_empty() => {
            app.apply_date_to_targets("+1w")?
        }
        KeyCode::Char('s') if app.screen == Screen::Triage || !app.marks.is_empty() => {
            app.apply_someday()?
        }
        KeyCode::Char('x') => app.apply_state("DONE")?,
        KeyCode::Char('t') => app.cycle_state()?,
        KeyCode::Char('s') => {
            app.overlay = Overlay::Prompt {
                kind: PromptKind::Schedule,
                input: String::new(),
            }
        }
        KeyCode::Char('d') if app.screen == Screen::Review => {
            app.review_diff = !app.review_diff;
        }
        KeyCode::Char('d') => {
            app.overlay = Overlay::Prompt {
                kind: PromptKind::Deadline,
                input: String::new(),
            }
        }
        KeyCode::Char(',') => {
            app.overlay = Overlay::Prompt {
                kind: PromptKind::Priority,
                input: String::new(),
            }
        }
        KeyCode::Char(':') => {
            app.overlay = Overlay::Prompt {
                kind: PromptKind::Tags,
                input: String::new(),
            }
        }
        KeyCode::Char('r') if app.screen == Screen::Triage => app.open_refile(),
        KeyCode::Char('/') => {
            app.overlay = Overlay::Prompt {
                kind: PromptKind::Filter,
                input: app.filter.clone(),
            }
        }
        KeyCode::Char('<') => {
            app.context = (app.context + 1) % 3;
            app.reload_rows()?;
        }
        KeyCode::Char(' ') if app.screen != Screen::Review => app.toggle_mark(),
        KeyCode::Char('u') if app.screen == Screen::Review => app.undo_review_selected()?,
        KeyCode::Char('u') => app.undo()?,
        KeyCode::Char('a') if app.screen == Screen::Review => app.review_selected()?,
        KeyCode::Enter if app.screen == Screen::Triage && app.rows.is_empty() => {
            app.switch_screen(Screen::Today)?;
        }
        KeyCode::Enter if app.pane => app.pane = false,
        KeyCode::Enter | KeyCode::Char('e') => open_selected(terminal, app)?,
        _ => {}
    }
    Ok(())
}

pub(super) fn handle_overlay(app: &mut App, key: KeyEvent) -> Result<()> {
    match &mut app.overlay {
        Overlay::Help => {
            app.overlay = Overlay::None;
        }
        Overlay::Capture {
            template, input, ..
        } => match key.code {
            KeyCode::Esc => app.overlay = Overlay::None,
            KeyCode::Backspace => {
                if input.is_empty() {
                    *template = None;
                } else {
                    input.pop();
                }
                app.refresh_capture_preview();
            }
            KeyCode::Enter if template.is_some() && !input.trim().is_empty() => {
                app.commit_capture()?;
            }
            KeyCode::Char(character) => {
                if template.is_none() && input.is_empty() {
                    let key = character.to_string();
                    if app.settings.capture.contains_key(&key) {
                        *template = Some(key);
                    } else {
                        app.set_status(format!("unknown capture template: {character}"));
                    }
                } else {
                    input.push(character);
                }
                app.refresh_capture_preview();
            }
            _ => {}
        },
        Overlay::Prompt { kind: _, input } => match key.code {
            KeyCode::Esc => app.overlay = Overlay::None,
            KeyCode::Backspace => {
                input.pop();
            }
            KeyCode::Enter => app.commit_prompt()?,
            KeyCode::Char(character) => input.push(character),
            _ => {}
        },
        Overlay::Refile {
            input,
            selected,
            targets,
        } => match key.code {
            KeyCode::Esc => app.overlay = Overlay::None,
            KeyCode::Backspace => {
                input.pop();
                *selected = 0;
            }
            KeyCode::Up => {
                let count = targets
                    .iter()
                    .filter(|target| fuzzy_match(&target.label, input))
                    .count();
                if count > 0 {
                    *selected = (*selected + count - 1) % count;
                }
            }
            KeyCode::Down => {
                let count = targets
                    .iter()
                    .filter(|target| fuzzy_match(&target.label, input))
                    .count();
                if count > 0 {
                    *selected = (*selected + 1) % count;
                }
            }
            KeyCode::Enter => return app.commit_refile(),
            KeyCode::Char(character) => {
                input.push(character);
                *selected = 0;
            }
            _ => {}
        },
        Overlay::None => {}
    }
    Ok(())
}
