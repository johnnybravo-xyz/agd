use std::io::Stdout;
use std::time::Instant;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use super::{App, Overlay, PromptKind, Screen, TriageState, capture, open_in_editor};

type Term = Terminal<CrosstermBackend<Stdout>>;

/// A file and line a key asks to open in $EDITOR.
pub(super) type EditorTarget = Option<(String, usize)>;

/// Handles one key, running $EDITOR when it asks for it.
pub(super) fn handle_key(terminal: &mut Term, app: &mut App, key: KeyEvent) -> Result<()> {
    if matches!(app.overlay, Overlay::Capture(_)) && !is_quit_key(key) {
        // The capture popup runs $EDITOR for the body itself.
        return capture::handle_key(terminal, app, key);
    }
    if let Some((file, line)) = dispatch(app, key)? {
        open_in_editor(terminal, app, &file, line)?;
    }
    Ok(())
}

fn is_quit_key(key: KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c')
}

/// Every key outside the capture popup, without a terminal, so tests can
/// press keys; returns where to open $EDITOR when the key asks for it.
pub(super) fn dispatch(app: &mut App, key: KeyEvent) -> Result<EditorTarget> {
    if key.kind == KeyEventKind::Release {
        return Ok(None);
    }
    if is_quit_key(key) {
        app.quit = true;
        return Ok(None);
    }
    match &app.overlay {
        Overlay::None => {}
        Overlay::Capture(_) => return Ok(None),
        Overlay::Edit(_) => {
            let editor = super::edit::handle_key(app, key)?;
            return Ok(if editor { selected_location(app) } else { None });
        }
        _ => {
            handle_overlay(app, key)?;
            return Ok(None);
        }
    }
    if app.pane {
        return pane_key(app, key);
    }
    match app.screen {
        Screen::Triage => triage_key(app, key).map(|()| None),
        Screen::Review => review_key(app, key),
        _ => list_key(app, key),
    }
}

/// Keys shared by every screen; returns true when handled.
fn common_key(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => app.move_selection(1),
        KeyCode::Char('k') | KeyCode::Up => app.move_selection(-1),
        KeyCode::PageDown => app.move_selection(10),
        KeyCode::PageUp => app.move_selection(-10),
        KeyCode::Home => app.selected = 0,
        KeyCode::End => app.selected = app.row_count().saturating_sub(1),
        KeyCode::Char('?') => app.overlay = Overlay::Help,
        KeyCode::Char('c') => app.overlay = Overlay::Capture(capture::Draft::default()),
        KeyCode::Char('g') => {
            app.index = agd_core::WorkspaceIndex::load(app.settings.clone())?;
            app.reload_rows()?;
            app.set_status("refreshed");
        }
        KeyCode::Tab => next_tab(app, 1)?,
        KeyCode::BackTab => next_tab(app, -1)?,
        KeyCode::Char(digit @ '1'..='9') => {
            let index = digit as usize - '1' as usize;
            if let Some(screen) = app.tabs().get(index).cloned() {
                if screen == Screen::Review {
                    app.review_diff = false;
                }
                app.switch_screen(screen)?;
            }
        }
        _ => return Ok(false),
    }
    Ok(true)
}

fn next_tab(app: &mut App, delta: isize) -> Result<()> {
    let tabs = app.tabs();
    let current = tabs.iter().position(|tab| *tab == app.screen).unwrap_or(0) as isize;
    let next = (current + delta).rem_euclid(tabs.len() as isize) as usize;
    app.switch_screen(tabs[next].clone())
}

/// ctrl-e: open in $EDITOR, everywhere (lists, pane, Agent review, the edit
/// and capture popups), as org-agenda's RET opens the entry's file.
fn is_editor_key(key: KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('e')
}

/// Today, Week, Inbox and saved views.
fn list_key(app: &mut App, key: KeyEvent) -> Result<EditorTarget> {
    if is_editor_key(key) {
        return Ok(selected_location(app));
    }
    if common_key(app, key)? {
        return Ok(None);
    }
    let bulk = !app.marks.is_empty();
    match key.code {
        KeyCode::Char('q') => app.quit = true,
        KeyCode::Esc => {
            if app.peek {
                app.peek = false;
            } else if bulk {
                app.marks.clear();
            } else if !app.filter.is_empty() {
                app.filter.clear();
                app.reload_rows()?;
            } else if app.screen != Screen::Today {
                let back = app.back.clone();
                app.switch_screen(if back == app.screen {
                    Screen::Today
                } else {
                    back
                })?;
            }
        }
        KeyCode::Enter | KeyCode::Char('e') => {
            if let Some(form) = super::edit::EditForm::open(app) {
                app.overlay = Overlay::Edit(Box::new(form));
            }
        }
        KeyCode::Char(' ') => app.toggle_mark(),
        // Bulk actions on marked rows, as org-agenda's B menu.
        KeyCode::Char('.') if bulk => app.schedule("today")?,
        KeyCode::Char('m') if bulk => app.schedule("+1d")?,
        KeyCode::Char('w') if bulk => app.schedule("+1w")?,
        KeyCode::Char('s') if bulk => app.someday()?,
        KeyCode::Char('x') => app.mark_done()?,
        KeyCode::Char('X') => app.cancel()?,
        KeyCode::Char('t') => app.cycle_state()?,
        KeyCode::Char('s') => prompt(app, PromptKind::Schedule),
        KeyCode::Char('d') => prompt(app, PromptKind::Deadline),
        KeyCode::Char(',') => prompt(app, PromptKind::Priority),
        KeyCode::Char('+') => app.shift_priority(true)?,
        KeyCode::Char('-') => app.shift_priority(false)?,
        KeyCode::Char(':') => prompt(app, PromptKind::Tags),
        KeyCode::Char('r') => app.open_refile(),
        KeyCode::Char('u') => app.undo()?,
        KeyCode::Char('y') => app.copy_link()?,
        KeyCode::Char('p') => app.peek = !app.peek,
        KeyCode::Char('T') => start_triage(app)?,
        // On the Agents tab `A` accepts every change; here it opens the tab.
        KeyCode::Char('A') => {
            app.review_diff = false;
            app.switch_screen(Screen::Review)?;
        }
        KeyCode::Char('a') => app.overlay = Overlay::Views { selected: 0 },
        KeyCode::Char('/') => {
            app.overlay = Overlay::Prompt {
                kind: PromptKind::Filter,
                input: app.filter.clone(),
            }
        }
        KeyCode::Char('<') => {
            let categories = app.categories();
            let index = app
                .context
                .as_ref()
                .and_then(|context| categories.iter().position(|category| category == context));
            app.context = match index {
                None => categories.first().cloned(),
                Some(index) => categories.get(index + 1).cloned(),
            };
            app.reload_rows()?;
        }
        KeyCode::Char('v') => {
            app.classic = !app.classic;
            app.reload_rows()?;
        }
        KeyCode::Char('l') => {
            app.log_mode = !app.log_mode;
            app.reload_rows()?;
        }
        KeyCode::Char('f') => {
            app.day += period(app);
            app.reload_rows()?;
        }
        KeyCode::Char('b') => {
            app.day -= period(app);
            app.reload_rows()?;
        }
        KeyCode::Char('.') => {
            app.day = app.today;
            app.reload_rows()?;
        }
        _ => {}
    }
    Ok(None)
}

fn period(app: &App) -> chrono::Duration {
    if app.screen == Screen::Week {
        chrono::Duration::weeks(1)
    } else {
        chrono::Duration::days(1)
    }
}

fn prompt(app: &mut App, kind: PromptKind) {
    if app.selected_row().is_some() || !app.marks.is_empty() {
        app.overlay = Overlay::Prompt {
            kind,
            input: String::new(),
        };
    }
}

fn start_triage(app: &mut App) -> Result<()> {
    app.triage = TriageState {
        started: Some(Instant::now()),
        ..TriageState::default()
    };
    app.switch_screen(Screen::Triage)
}

/// The selected row's heading, for $EDITOR.
fn selected_location(app: &App) -> EditorTarget {
    app.selected_row()
        .map(|row| (row.item.file.clone(), row.item.line))
}

/// One key per decision on the current card.
fn triage_key(app: &mut App, key: KeyEvent) -> Result<()> {
    if app.rows.is_empty() {
        return match key.code {
            KeyCode::Char('u') => app.undo(),
            KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q') => app.switch_screen(Screen::Today),
            _ => Ok(()),
        };
    }
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            let back = app.back.clone();
            app.switch_screen(back)?;
        }
        KeyCode::Char('.') => app.schedule("today")?,
        KeyCode::Char('m') => app.schedule("+1d")?,
        KeyCode::Char('w') => app.schedule("+1w")?,
        KeyCode::Char('p') => prompt(app, PromptKind::Schedule),
        KeyCode::Char('x') => app.mark_done()?,
        KeyCode::Char('n') => app.set_state("NEXT")?,
        KeyCode::Char('s') => app.someday()?,
        KeyCode::Char('k') => app.cancel()?,
        KeyCode::Char('r') => app.open_refile(),
        KeyCode::Char('u') => app.undo()?,
        KeyCode::Char('j') | KeyCode::Down | KeyCode::Tab => app.move_selection(1),
        KeyCode::Up | KeyCode::BackTab => app.move_selection(-1),
        KeyCode::Char('?') => app.overlay = Overlay::Help,
        _ => {}
    }
    Ok(())
}

fn review_key(app: &mut App, key: KeyEvent) -> Result<EditorTarget> {
    let key = if is_editor_key(key) {
        KeyEvent::from(KeyCode::Enter)
    } else {
        key
    };
    if common_key(app, key)? {
        return Ok(None);
    }
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            if app.review_diff {
                app.review_diff = false;
            } else if app.peek {
                app.peek = false;
            } else {
                let back = app.back.clone();
                app.switch_screen(if back == Screen::Review {
                    Screen::Today
                } else {
                    back
                })?;
            }
        }
        KeyCode::Char('a') => app.accept_selected()?,
        KeyCode::Char('A') => app.accept_all()?,
        KeyCode::Char('u') => app.undo_selected_change()?,
        KeyCode::Char('d') => app.review_diff = !app.review_diff,
        KeyCode::Char('p') => app.peek = !app.peek,
        KeyCode::Char('f') => app.cycle_agent_filter()?,
        KeyCode::Enter => {
            if let Some(record) = app.journals.get(app.selected) {
                let file = record
                    .file
                    .split(',')
                    .next()
                    .unwrap_or_default()
                    .to_string();
                let line = super::review::changed_line(record);
                return Ok(Some((file, line)));
            }
        }
        _ => {}
    }
    Ok(None)
}

/// The pane: glance, mark done, triage, capture, or open the full TUI.
fn pane_key(app: &mut App, key: KeyEvent) -> Result<EditorTarget> {
    if is_editor_key(key) {
        return Ok(selected_location(app));
    }
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => app.move_selection(1),
        KeyCode::Char('k') | KeyCode::Up => app.move_selection(-1),
        KeyCode::Char('x') => app.mark_done()?,
        KeyCode::Char('X') => app.cancel()?,
        KeyCode::Char('u') => app.undo()?,
        KeyCode::Char('c') => app.overlay = Overlay::Capture(capture::Draft::default()),
        KeyCode::Char('T') => {
            app.pane = false;
            start_triage(app)?;
        }
        KeyCode::Enter => {
            app.pane = false;
            app.reload_rows()?;
        }
        KeyCode::Char('q') => app.quit = true,
        _ => {}
    }
    Ok(None)
}

fn handle_overlay(app: &mut App, key: KeyEvent) -> Result<()> {
    match &mut app.overlay {
        Overlay::Help => app.overlay = Overlay::None,
        Overlay::Prompt { input, .. } => match key.code {
            KeyCode::Esc => app.overlay = Overlay::None,
            KeyCode::Backspace => {
                input.pop();
            }
            KeyCode::Enter => app.commit_prompt()?,
            KeyCode::Char(character) => input.push(character),
            _ => {}
        },
        Overlay::Refile(picker) => match key.code {
            KeyCode::Esc => app.overlay = Overlay::None,
            KeyCode::Enter => app.commit_refile()?,
            KeyCode::Up => picker.selected = picker.selected.saturating_sub(1),
            KeyCode::Down => {
                let count = picker.matches().len();
                picker.selected = (picker.selected + 1).min(count.saturating_sub(1));
            }
            KeyCode::Backspace => {
                picker.input.pop();
                picker.selected = 0;
            }
            KeyCode::Char(character) => {
                picker.input.push(character);
                picker.selected = 0;
            }
            _ => {}
        },
        Overlay::Views { selected } => {
            let keys: Vec<String> = app
                .settings
                .views
                .iter()
                .map(|view| view.key.clone())
                .collect();
            match key.code {
                KeyCode::Esc => app.overlay = Overlay::None,
                KeyCode::Up | KeyCode::Char('k') => *selected = selected.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => {
                    *selected = (*selected + 1).min(keys.len().saturating_sub(1));
                }
                KeyCode::Enter => {
                    if let Some(key) = keys.get(*selected).cloned() {
                        app.overlay = Overlay::None;
                        app.switch_screen(Screen::View(key))?;
                    }
                }
                KeyCode::Char(character) => {
                    if let Some(key) = keys.iter().find(|key| **key == character.to_string()) {
                        let key = key.clone();
                        app.overlay = Overlay::None;
                        app.switch_screen(Screen::View(key))?;
                    }
                }
                _ => {}
            }
        }
        Overlay::None | Overlay::Capture(_) | Overlay::Edit(_) => {}
    }
    Ok(())
}
