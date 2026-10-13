use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Stdout, Write};
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration as StdDuration, Instant};

use anyhow::{Context, Result};
use chrono::{Duration, Local, NaiveDate};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
    MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use notify::{Config as NotifyConfig, Event as NotifyEvent, RecommendedWatcher, RecursiveMode, Watcher};
use org_core::{
    AgendaEngine, AgendaEntry, AgendaQuery, AgendaSpan, Change, EditIntent, Item,
    JournalRecord, JournalStatus, Occurrence, OccurrenceType, Settings, WorkspaceIndex, WriteContext,
    WriteEngine, load_settings, parse_capture_line, parse_date_input,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Scrollbar,
    ScrollbarOrientation, ScrollbarState, Tabs, Wrap,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Screen {
    Today,
    Triage,
    Review,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AgendaView {
    Day,
    Week,
    Todo,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PromptKind {
    Schedule,
    Deadline,
    Priority,
    Tags,
    Filter,
}

#[derive(Debug)]
enum Overlay {
    None,
    Help,
    Capture {
        template: Option<String>,
        input: String,
        preview: String,
        body: String,
    },
    Prompt {
        kind: PromptKind,
        input: String,
    },
    Refile {
        input: String,
        selected: usize,
        targets: Vec<RefileTarget>,
    },
}

#[derive(Clone, Debug)]
struct DisplayRow {
    section: String,
    item: Item,
    occurrence: Option<Occurrence>,
}

#[derive(Clone, Debug)]
struct RefileTarget {
    item_ref: String,
    label: String,
}

struct App {
    settings: Settings,
    index: WorkspaceIndex,
    rows: Vec<DisplayRow>,
    journals: Vec<JournalRecord>,
    screen: Screen,
    view: AgendaView,
    day: NaiveDate,
    selected: usize,
    marks: BTreeSet<String>,
    context: usize,
    filter: String,
    classic: bool,
    peek: bool,
    pane: bool,
    pending_view_key: bool,
    overlay: Overlay,
    status: String,
    status_at: Instant,
    list_area: Rect,
    inbox_count: usize,
    agent_count: usize,
    slipped_count: usize,
    recent_targets: Vec<String>,
    triage_started: Option<Instant>,
    triage_outcomes: [usize; 6],
    triage_undo: Vec<(Vec<String>, usize)>,
    quit: bool,
}

pub fn run(config: Option<String>, root: Option<String>, pane: bool) -> Result<()> {
    let loaded = load_settings(
        config.as_deref().map(Path::new),
        root.as_deref().map(Path::new),
    )?;
    let settings = loaded.settings;
    let index = WorkspaceIndex::load(settings.clone())?;
    let (events, _watcher) = watch_roots(&index)?;
    let mut app = App {
        settings,
        index,
        rows: Vec::new(),
        journals: Vec::new(),
        screen: Screen::Today,
        view: AgendaView::Day,
        day: Local::now().date_naive(),
        selected: 0,
        marks: BTreeSet::new(),
        context: 0,
        filter: String::new(),
        classic: false,
        peek: false,
        pane,
        pending_view_key: false,
        overlay: Overlay::None,
        status: String::new(),
        status_at: Instant::now(),
        list_area: Rect::default(),
        inbox_count: 0,
        agent_count: 0,
        slipped_count: 0,
        recent_targets: Vec::new(),
        triage_started: None,
        triage_outcomes: [0; 6],
        triage_undo: Vec::new(),
        quit: false,
    };
    app.reload_rows()?;

    let mut terminal = start_terminal()?;
    let result = event_loop(&mut terminal, &mut app, &events);
    stop_terminal(&mut terminal)?;
    result
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    changes: &Receiver<notify::Result<NotifyEvent>>,
) -> Result<()> {
    while !app.quit {
        for event in changes.try_iter() {
            match event {
                Ok(event) => {
                    for path in event.paths {
                        if let Err(error) = app.index.refresh_path(&path) {
                            app.set_status(format!("reload failed: {error}"));
                        }
                    }
                    app.reload_rows()?;
                }
                Err(error) => app.set_status(format!("watch failed: {error}")),
            }
        }
        if app.status_at.elapsed() > StdDuration::from_secs(3) {
            app.status.clear();
        }
        terminal.draw(|frame| draw(frame, app))?;
        if event::poll(StdDuration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => handle_key(terminal, app, key)?,
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollDown => app.move_selection(1),
                    MouseEventKind::ScrollUp => app.move_selection(-1),
                    MouseEventKind::Down(_) if app.list_area.contains((mouse.column, mouse.row).into()) => {
                        let offset = mouse.row.saturating_sub(app.list_area.y + 1) as usize;
                        if app.pane {
                            if let Some((index, _)) = app
                                .rows
                                .iter()
                                .enumerate()
                                .filter(|(_, row)| row.section != "SLIPPED")
                                .nth(offset)
                            {
                                app.selected = index;
                                app.pane = false;
                            }
                        } else if offset < app.row_count() {
                            app.selected = offset;
                        }
                    }
                    _ => {}
                },
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
    }
    Ok(())
}

fn handle_key(
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
                app.screen = Screen::Today;
                app.reload_rows()?;
            } else if app.peek {
                app.peek = false;
            }
        }
        KeyCode::Char('j') | KeyCode::Down => app.move_selection(1),
        KeyCode::Char('k') | KeyCode::Up => app.move_selection(-1),
        KeyCode::Char('?') => app.overlay = Overlay::Help,
        KeyCode::Char('p') => app.peek = !app.peek,
        KeyCode::Char('T') => {
            app.screen = Screen::Triage;
            app.triage_started = Some(Instant::now());
            app.triage_outcomes = [0; 6];
            app.triage_undo.clear();
            app.reload_rows()?;
        }
        KeyCode::Char('c') => {
            app.overlay = Overlay::Capture {
                template: None,
                input: String::new(),
                preview: "type a capture template key".to_string(),
                body: String::new(),
            }
        }
        KeyCode::Char('v') => app.pending_view_key = true,
        KeyCode::Char('V') => {
            app.classic = !app.classic;
            app.reload_rows()?;
        }
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
        KeyCode::Char('s') => app.overlay = Overlay::Prompt {
            kind: PromptKind::Schedule,
            input: String::new(),
        },
        KeyCode::Char('d') => app.overlay = Overlay::Prompt {
            kind: PromptKind::Deadline,
            input: String::new(),
        },
        KeyCode::Char(',') => app.overlay = Overlay::Prompt {
            kind: PromptKind::Priority,
            input: String::new(),
        },
        KeyCode::Char(':') => app.overlay = Overlay::Prompt {
            kind: PromptKind::Tags,
            input: String::new(),
        },
        KeyCode::Char('r') if app.screen == Screen::Triage => app.open_refile(),
        KeyCode::Char('/') => app.overlay = Overlay::Prompt {
            kind: PromptKind::Filter,
            input: app.filter.clone(),
        },
        KeyCode::Char('<') => {
            app.context = (app.context + 1) % 3;
            app.reload_rows()?;
        }
        KeyCode::Char(' ') if app.screen != Screen::Review => app.toggle_mark(),
        KeyCode::Char('u') if app.screen == Screen::Review => app.undo_review_selected()?,
        KeyCode::Char('u') => app.undo()?,
        KeyCode::Char('a') if app.screen == Screen::Review => app.review_selected()?,
        KeyCode::Enter if app.screen == Screen::Triage && app.rows.is_empty() => {
            app.screen = Screen::Today;
            app.reload_rows()?;
        }
        KeyCode::Enter if app.pane => app.pane = false,
        KeyCode::Enter | KeyCode::Char('e') => open_selected(terminal, app)?,
        _ => {}
    }
    Ok(())
}

fn handle_overlay(app: &mut App, key: KeyEvent) -> Result<()> {
    match &mut app.overlay {
        Overlay::Help => {
            app.overlay = Overlay::None;
        }
        Overlay::Capture {
            template,
            input,
            preview: _,
            ..
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

impl App {
    fn reload_rows(&mut self) -> Result<()> {
        self.selected = 0;
        self.inbox_count = self
            .index
            .items()
            .filter(|item| !item.done && is_inbox(item))
            .count();
        let journals = WriteEngine::with_default_journal(self.settings.clone())?.journal()?;
        self.agent_count = journals
            .iter()
            .filter(|record| {
                matches!(record.status, JournalStatus::Committed)
                    && record.actor.starts_with("mcp:")
            })
            .count();
        if self.screen == Screen::Review {
            self.journals = journals
                .into_iter()
                .filter(|record| {
                    matches!(record.status, JournalStatus::Committed)
                        && record.actor.starts_with("mcp:")
                })
                .collect();
            return Ok(());
        }
        let today = Local::now().date_naive();
        let engine = AgendaEngine::new(&self.index);
        let mut rows = if self.screen == Screen::Triage {
            let entries = engine.agenda(&AgendaQuery {
                from: today - Duration::days(3650),
                to: today,
                today,
                match_expression: None,
            })?;
            let mut seen = BTreeSet::new();
            let mut rows: Vec<DisplayRow> = entries
                .into_iter()
                .filter(|entry| {
                    matches!(entry.occurrence.kind, OccurrenceType::PastScheduled)
                        || is_inbox(&entry.item)
                })
                .filter(|entry| !entry.item.done && seen.insert(entry.item.item_ref.clone()))
                .map(|entry| DisplayRow {
                    section: if is_inbox(&entry.item) { "INBOX" } else { "SLIPPED" }.to_string(),
                    item: entry.item,
                    occurrence: Some(entry.occurrence),
                })
                .collect();
            for item in engine.todo(None)?.into_iter().filter(is_inbox) {
                if seen.insert(item.item_ref.clone()) {
                    rows.push(DisplayRow {
                        section: "INBOX".to_string(),
                        item,
                        occurrence: None,
                    });
                }
            }
            rows
        } else if self.view == AgendaView::Todo {
            engine
                .todo(None)?
                .into_iter()
                .map(|item| DisplayRow {
                    section: "TODO".to_string(),
                    item,
                    occurrence: None,
                })
                .collect()
        } else {
            let span = if self.view == AgendaView::Week {
                AgendaSpan::Week
            } else {
                AgendaSpan::Day
            };
            let (from, to) = span.bounds(self.day, &self.settings.agenda.week_start);
            let mut seen = BTreeSet::new();
            let mut rows: Vec<DisplayRow> = engine
                .agenda(&AgendaQuery {
                    from,
                    to,
                    today,
                    match_expression: None,
                })?
                .into_iter()
                .map(|entry| DisplayRow {
                    section: section_for(&entry),
                    item: entry.item,
                    occurrence: Some(entry.occurrence),
                })
                .inspect(|row| {
                    seen.insert(row.item.item_ref.clone());
                })
                .collect();
            if self.view == AgendaView::Day {
                for item in engine.todo(None)?.into_iter().filter(is_inbox) {
                    if seen.insert(item.item_ref.clone()) {
                        rows.push(DisplayRow {
                            section: "INBOX".to_string(),
                            item,
                            occurrence: None,
                        });
                    }
                }
            }
            rows
        };
        let context = ["all", "work", "personal"][self.context];
        rows.retain(|row| {
            (context == "all" || row.item.category.eq_ignore_ascii_case(context))
                && (self.filter.is_empty()
                    || row.item.title.to_lowercase().contains(&self.filter.to_lowercase())
                    || row
                        .item
                        .tags
                        .iter()
                        .any(|tag| tag.eq_ignore_ascii_case(&self.filter)))
        });
        self.slipped_count = rows.iter().filter(|row| row.section == "SLIPPED").count();
        if !self.classic && self.screen == Screen::Today {
            let mut slipped = 0;
            rows.retain(|row| {
                if row.section == "SLIPPED" {
                    slipped += 1;
                    slipped <= 3
                } else {
                    true
                }
            });
            rows.sort_by_key(|row| section_order(&row.section));
        }
        self.rows = rows;
        Ok(())
    }

    fn selected_row(&self) -> Option<&DisplayRow> {
        self.rows.get(self.selected)
    }

    fn row_count(&self) -> usize {
        if self.screen == Screen::Review {
            self.journals.len()
        } else {
            self.rows.len()
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let count = self.row_count();
        if count == 0 {
            self.selected = 0;
        } else {
            self.selected = (self.selected as isize + delta).rem_euclid(count as isize) as usize;
        }
    }

    fn set_status(&mut self, status: String) {
        self.status = status;
        self.status_at = Instant::now();
    }

    fn toggle_mark(&mut self) {
        let Some(item_ref) = self.selected_row().map(|row| row.item.item_ref.clone()) else {
            return;
        };
        if !self.marks.remove(&item_ref) {
            self.marks.insert(item_ref);
        }
    }

    fn targets(&self) -> Vec<String> {
        if self.marks.is_empty() {
            self.selected_row()
                .map(|row| vec![row.item.item_ref.clone()])
                .unwrap_or_default()
        } else {
            self.marks.iter().cloned().collect()
        }
    }

    fn apply_state(&mut self, state: &str) -> Result<()> {
        let targets = self.targets();
        let mut journals = Vec::new();
        for item_ref in &targets {
            journals.push(self.edit_ref(
                item_ref,
                EditIntent {
                    state: Change::Set(state.to_string()),
                    ..EditIntent::default()
                },
            )?);
        }
        if state == "DONE" {
            self.record_triage(journals, 4);
        }
        self.marks.clear();
        self.set_status(format!("{} item(s) → {state} · u undo", targets.len()));
        self.reload_rows()
    }

    fn cycle_state(&mut self) -> Result<()> {
        let Some(row) = self.selected_row() else {
            return Ok(());
        };
        let active: Vec<&str> = self
            .settings
            .todo
            .sequences
            .iter()
            .flat_map(|sequence| sequence.iter())
            .take_while(|keyword| keyword.as_str() != "|")
            .map(|keyword| keyword.split(['(', '!','@']).next().unwrap_or(keyword))
            .collect();
        if active.is_empty() {
            return Ok(());
        }
        let current = row.item.todo.as_deref();
        let index = active.iter().position(|keyword| Some(*keyword) == current).unwrap_or(0);
        let next = active[(index + 1) % active.len()].to_string();
        self.apply_state(&next)
    }

    fn apply_date_to_targets(&mut self, value: &str) -> Result<()> {
        let date = parse_date_input(value, Local::now().date_naive())?;
        let targets = self.targets();
        let mut journals = Vec::new();
        for item_ref in &targets {
            journals.push(self.edit_ref(
                item_ref,
                EditIntent {
                    scheduled: Change::Set(date.clone()),
                    ..EditIntent::default()
                },
            )?);
        }
        let outcome = match value {
            "today" => 0,
            "+1d" => 1,
            _ => 2,
        };
        self.record_triage(journals, outcome);
        self.marks.clear();
        self.set_status(format!("scheduled {} item(s) {value} · u undo", targets.len()));
        self.reload_rows()
    }

    fn apply_someday(&mut self) -> Result<()> {
        let targets = self.targets();
        let mut journals = Vec::new();
        for item_ref in &targets {
            journals.push(self.edit_ref(
                item_ref,
                EditIntent {
                    scheduled: Change::Clear,
                    tags_add: BTreeSet::from([self.settings.triage.someday_tag.clone()]),
                    ..EditIntent::default()
                },
            )?);
        }
        self.record_triage(journals, 3);
        self.marks.clear();
        self.set_status(format!("moved {} item(s) to someday · u undo", targets.len()));
        self.reload_rows()
    }

    fn edit_ref(&mut self, item_ref: &str, intent: EditIntent) -> Result<String> {
        let mutation = WriteEngine::with_default_journal(self.settings.clone())?.edit(
            item_ref,
            &intent,
            false,
            &write_context(),
        )?;
        self.index.refresh_path(Path::new(&mutation.item.file))?;
        Ok(mutation.journal_id)
    }

    fn record_triage(&mut self, journals: Vec<String>, outcome: usize) {
        if self.screen == Screen::Triage && !journals.is_empty() {
            self.triage_outcomes[outcome] += journals.len();
            self.triage_undo.push((journals, outcome));
        }
    }

    fn undo(&mut self) -> Result<()> {
        if self.screen == Screen::Triage && !self.triage_undo.is_empty() {
            let undo_all = self.rows.is_empty();
            let mut groups = if undo_all {
                std::mem::take(&mut self.triage_undo)
            } else {
                vec![self.triage_undo.pop().expect("triage undo group")]
            };
            for (journals, outcome) in groups.drain(..).rev() {
                for journal in journals.iter().rev() {
                    let result = WriteEngine::with_default_journal(self.settings.clone())?
                        .undo(Some(journal), &write_context())?;
                    for file in result.file.split(',') {
                        self.index.refresh_path(Path::new(file))?;
                    }
                }
                self.triage_outcomes[outcome] =
                    self.triage_outcomes[outcome].saturating_sub(journals.len());
            }
            self.set_status(if undo_all {
                "undid all triage changes".to_string()
            } else {
                "undid last triage action".to_string()
            });
            return self.reload_rows();
        }
        let result = WriteEngine::with_default_journal(self.settings.clone())?
            .undo(None, &write_context())?;
        for file in result.file.split(',') {
            self.index.refresh_path(Path::new(file))?;
        }
        self.set_status(format!("undid {}", result.journal_id));
        self.reload_rows()
    }
    fn review_selected(&mut self) -> Result<()> {
        let Some(record) = self.journals.get(self.selected) else {
            return Ok(());
        };
        WriteEngine::with_default_journal(self.settings.clone())?.review(&record.id)?;
        self.set_status(format!("reviewed {}", record.id));
        self.reload_rows()
    }

    fn undo_review_selected(&mut self) -> Result<()> {
        let Some(record) = self.journals.get(self.selected) else {
            return Ok(());
        };
        let id = record.id.clone();
        let result = WriteEngine::with_default_journal(self.settings.clone())?
            .undo(Some(&id), &write_context())?;
        for file in result.file.split(',') {
            self.index.refresh_path(Path::new(file))?;
        }
        self.set_status(format!("undid agent change {id}"));
        self.reload_rows()
    }

    fn open_refile(&mut self) {
        let Some(source_ref) = self
            .selected_row()
            .map(|row| row.item.item_ref.clone())
        else {
            return;
        };
        let mut targets: Vec<RefileTarget> = self
            .index
            .items()
            .filter(|item| item.item_ref != source_ref)
            .map(|item| RefileTarget {
                item_ref: item.item_ref.clone(),
                label: format!(
                    "{}  {}:{}",
                    item.olpath.join(" / "),
                    item.file,
                    item.line
                ),
            })
            .collect();
        targets.sort_by_key(|target| {
            (
                self.recent_targets
                    .iter()
                    .position(|item_ref| item_ref == &target.item_ref)
                    .unwrap_or(usize::MAX),
                target.label.to_lowercase(),
            )
        });
        self.overlay = Overlay::Refile {
            input: String::new(),
            selected: 0,
            targets,
        };
    }

    fn commit_refile(&mut self) -> Result<()> {
        let Overlay::Refile {
            input,
            selected,
            targets,
        } = &self.overlay
        else {
            return Ok(());
        };
        let Some(target) = targets
            .iter()
            .filter(|target| fuzzy_match(&target.label, input))
            .nth(*selected)
            .cloned()
        else {
            return Ok(());
        };
        let Some((source_ref, source_file)) = self
            .selected_row()
            .map(|row| (row.item.item_ref.clone(), row.item.file.clone()))
        else {
            return Ok(());
        };
        let result = WriteEngine::with_default_journal(self.settings.clone())?.refile(
            &source_ref,
            &target.item_ref,
            false,
            &write_context(),
        )?;
        self.index.refresh_path(Path::new(&source_file))?;
        self.index.refresh_path(Path::new(&result.item.file))?;
        self.recent_targets.retain(|item_ref| item_ref != &target.item_ref);
        self.recent_targets.insert(0, target.item_ref);
        self.recent_targets.truncate(10);
        self.record_triage(result.journal_ids, 5);
        self.overlay = Overlay::None;
        self.set_status(format!("refiled {} · u undo", result.item.item_ref));
        self.reload_rows()
    }

    fn refresh_capture_preview(&mut self) {
        let Overlay::Capture {
            template,
            input,
            preview,
            body,
        } = &mut self.overlay
        else {
            return;
        };
        let Some(template) = template.clone() else {
            *preview = "type a capture template key".to_string();
            return;
        };
        let result = parse_capture_line(input, Local::now().date_naive()).and_then(|draft| {
            WriteEngine::with_default_journal(self.settings.clone())?.capture(
                &template,
                &draft,
                body,
                &BTreeMap::new(),
                true,
                &write_context(),
            )
        });
        *preview = result.map(|mutation| mutation.diff).unwrap_or_else(|error| error.to_string());
    }

    fn commit_capture(&mut self) -> Result<()> {
        let Overlay::Capture {
            template,
            input,
            body,
            ..
        } = &self.overlay
        else {
            return Ok(());
        };
        let template = template.clone().expect("capture template selected");
        let draft = parse_capture_line(input, Local::now().date_naive())?;
        let mutation = WriteEngine::with_default_journal(self.settings.clone())?.capture(
            &template,
            &draft,
            body,
            &BTreeMap::new(),
            false,
            &write_context(),
        )?;
        self.index.refresh_path(Path::new(&mutation.item.file))?;
        self.overlay = Overlay::None;
        self.set_status(format!("captured {} · u undo", mutation.item.item_ref));
        self.reload_rows()
    }

    fn commit_prompt(&mut self) -> Result<()> {
        let Overlay::Prompt { kind, input } = &self.overlay else {
            return Ok(());
        };
        let kind = *kind;
        let input = input.clone();
        if kind == PromptKind::Filter {
            self.filter = input;
            self.overlay = Overlay::None;
            return self.reload_rows();
        }
        let Some(item_ref) = self.selected_row().map(|row| row.item.item_ref.clone()) else {
            self.overlay = Overlay::None;
            return Ok(());
        };
        let mut triage_outcome = None;
        let mut intent = EditIntent::default();
        match kind {
            PromptKind::Schedule => {
                let date = parse_date_input(&input, Local::now().date_naive())?;
                let planned = date.date.parse::<NaiveDate>()?;
                let today = Local::now().date_naive();
                triage_outcome = Some(if planned <= today {
                    0
                } else if planned == today + Duration::days(1) {
                    1
                } else {
                    2
                });
                intent.scheduled = Change::Set(date);
            }
            PromptKind::Deadline => {
                intent.deadline = Change::Set(parse_date_input(&input, Local::now().date_naive())?)
            }
            PromptKind::Priority => intent.priority = Change::Set(input),
            PromptKind::Tags => {
                for tag in input.split(',').map(str::trim).filter(|tag| !tag.is_empty()) {
                    if let Some(tag) = tag.strip_prefix('-') {
                        intent.tags_remove.insert(tag.to_string());
                    } else {
                        intent.tags_add.insert(tag.trim_start_matches('+').to_string());
                    }
                }
            }
            PromptKind::Filter => unreachable!(),
        }
        let journal = self.edit_ref(&item_ref, intent)?;
        if let Some(outcome) = triage_outcome {
            self.record_triage(vec![journal], outcome);
        }
        self.overlay = Overlay::None;
        self.set_status(format!("updated {item_ref} · u undo"));
        self.reload_rows()
    }
}

fn draw(frame: &mut ratatui::Frame<'_>, app: &mut App) {
    let area = frame.area();
    if app.pane || area.width < 60 {
        draw_pane(frame, app, area);
    } else {
        draw_full(frame, app, area);
    }
    draw_overlay(frame, app, area);
}

fn draw_full(frame: &mut ratatui::Frame<'_>, app: &mut App, area: Rect) {
    let accent = accent();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(3), Constraint::Length(1)])
        .split(area);
    let tabs = Tabs::new(vec![
        format!(" Today · {} inbox ", app.inbox_count),
        format!(" Triage · {} slipped ", app.slipped_count),
        format!(" Agent review · {} ", app.agent_count),
    ])
        .select(match app.screen { Screen::Today => 0, Screen::Triage => 1, Screen::Review => 2 })
        .highlight_style(Style::default().fg(accent).add_modifier(Modifier::BOLD))
        .divider(" ");
    frame.render_widget(tabs, chunks[0]);

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
    } else {
        draw_rows(frame, app, body[0]);
    }
    if body.len() > 1 {
        draw_peek(frame, app, body[1]);
    }
    let selected = app.selected_row().map(|row| format!("{}:{}", row.item.file, row.item.line)).unwrap_or_default();
    let context = ["all", "work", "personal"][app.context];
    let status = if app.status.is_empty() {
        format!(
            "ctx:{context}  {selected}  {} inbox · {} agent   j/k move  t state  s sched  c capture  ? keys",
            app.inbox_count, app.agent_count
        )
    } else {
        app.status.clone()
    };
    frame.render_widget(Paragraph::new(status).style(Style::default().fg(accent)), chunks[2]);
}

fn draw_rows(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let list_area = if app.screen == Screen::Triage {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(3)])
            .split(area);
        let ratio = if app.rows.is_empty() {
            1.0
        } else {
            (app.selected + 1) as f64 / app.rows.len() as f64
        };
        frame.render_widget(
            Gauge::default()
                .block(rounded("Triage progress"))
                .gauge_style(Style::default().fg(accent()))
                .ratio(ratio),
            chunks[0],
        );
        chunks[1]
    } else {
        area
    };
    if app.screen == Screen::Triage && app.rows.is_empty() {
        let elapsed = app
            .triage_started
            .map(|started| started.elapsed().as_secs())
            .unwrap_or_default();
        let total: usize = app.triage_outcomes.iter().sum();
        let summary = format!(
            "Triage done, {total} items in {}m {}s\n{} today · {} tomorrow · {} next week · {} someday · {} done · {} refiled\n\nu undo all · ⏎ Today",
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
            list_area,
        );
        return;
    }
    let mut previous = "";
    let mut items = Vec::new();
    for row in &app.rows {
        let mut lines = Vec::new();
        if !app.classic && row.section != previous {
            lines.push(Line::from(Span::styled(
                format!(" {} ", row.section),
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
        let tags = if area.width >= 100 && !row.item.tags.is_empty() {
            format!("  :{}:", row.item.tags.join(":"))
        } else {
            String::new()
        };
        lines.push(Line::from(format!(
            "{mark} {prefix:<12} {time:<5} {todo:<9} {}{tags}",
            row.item.title
        )));
        if app.screen == Screen::Triage {
            if let Some(occurrence) = &row.occurrence {
                let days = occurrence
                    .date
                    .parse::<NaiveDate>()
                    .ok()
                    .map(|date| {
                        Local::now()
                            .date_naive()
                            .signed_duration_since(date)
                            .num_days()
                            .max(0)
                    })
                    .unwrap_or_default();
                lines.push(Line::from(format!("  {days} days ago · r refile")));
            } else {
                lines.push(Line::from("  inbox · r refile"));
            }
        }
        let style = if row.item.done {
            Style::default().add_modifier(Modifier::DIM)
        } else if row.item.priority.as_deref() == Some("A") {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
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
        let mut scrollbar =
            ScrollbarState::new(app.rows.len()).position(app.selected);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            list_area,
            &mut scrollbar,
        );
    }
}

fn draw_review(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let items: Vec<ListItem<'_>> = app
        .journals
        .iter()
        .map(|record| ListItem::new(format!("{}  {}  {}", record.actor, record.timestamp, record.file)))
        .collect();
    let mut state = ListState::default().with_selected(Some(app.selected));
    let list = List::new(items)
        .block(rounded("Agent review"))
        .highlight_symbol("▌")
        .highlight_style(selection_style());
    frame.render_stateful_widget(list, area, &mut state);
    if let Some(record) = app.journals.get(app.selected) {
        let popup = centered(area, 80, 45);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(record.diff.clone())
                .block(rounded("Change · a accept · u undo"))
                .wrap(Wrap { trim: false }),
            popup,
        );
    }
}

fn draw_peek(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let text = app
        .selected_row()
        .and_then(|row| app.index.resolve(&row.item.item_ref).ok())
        .map(|(document, headline)| document.source()[headline.subtree_span.start..headline.subtree_span.end].to_string())
        .unwrap_or_else(|| "No item selected".to_string());
    frame.render_widget(Paragraph::new(text).block(rounded("Peek")).wrap(Wrap { trim: false }), area);
}

fn draw_pane(frame: &mut ratatui::Frame<'_>, app: &mut App, area: Rect) {
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
    for (index, row) in visible
        .iter()
        .take(area.height.saturating_sub(5) as usize)
    {
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
    frame.render_widget(Paragraph::new(lines).block(rounded("agendum")), area);
}

fn draw_overlay(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    match &app.overlay {
        Overlay::None => {
            if app.peek && area.width < 100 && !app.pane {
                let popup = centered(area, 86, 70);
                frame.render_widget(Clear, popup);
                draw_peek(frame, app, popup);
            }
        }
        Overlay::Help => {
            let popup = centered(area, 76, 70);
            frame.render_widget(Clear, popup);
            frame.render_widget(
                Paragraph::new("j/k move   ⏎/e editor   t cycle   x done\ns/d schedule/deadline   , priority   : tags\nc capture   T triage   A agent review   p peek\n< context   / filter   f/b period   . today\nspace mark   u undo   v then d/w/t view   V classic\nesc back   q quit")
                    .block(rounded("Keys · any key closes"))
                    .wrap(Wrap { trim: false }),
                popup,
            );
        }
        Overlay::Capture {
            template,
            input,
            preview,
            body,
        } => {
            let popup = centered(area, 86, 72);
            frame.render_widget(Clear, popup);
            let text = format!(
                "template: {}\n> {}\nbody: {} bytes · ctrl-e edit\n\nExact write preview\n{}",
                template.as_deref().unwrap_or("_"),
                input,
                body.len(),
                preview
            );
            frame.render_widget(
                Paragraph::new(text)
                    .block(rounded("Capture · Enter save · esc cancel"))
                    .wrap(Wrap { trim: false }),
                popup,
            );
        }
        Overlay::Prompt { kind, input } => {
            let popup = centered(area, 70, 20);
            frame.render_widget(Clear, popup);
            frame.render_widget(
                Paragraph::new(format!("> {input}"))
                    .block(rounded(match kind {
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

fn edit_capture_body(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
) -> Result<()> {
    let Overlay::Capture { body, .. } = &app.overlay else {
        return Ok(());
    };
    let mut file = tempfile::NamedTempFile::new().context("cannot create capture draft")?;
    file.write_all(body.as_bytes())?;
    file.flush()?;
    let path = file.path().to_string_lossy().into_owned();
    let args: Vec<String> = app
        .settings
        .editor
        .args
        .iter()
        .map(|argument| argument.replace("{file}", &path).replace("{line}", "1"))
        .collect();
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

fn open_selected(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
) -> Result<()> {
    let Some(row) = app.selected_row() else {
        return Ok(());
    };
    let file = row.item.file.clone();
    let line = row.item.line.to_string();
    let args: Vec<String> = app
        .settings
        .editor
        .args
        .iter()
        .map(|argument| argument.replace("{file}", &file).replace("{line}", &line))
        .collect();
    stop_terminal(terminal)?;
    let status = Command::new(&app.settings.editor.command)
        .args(args)
        .status()
        .with_context(|| format!("cannot start editor {}", app.settings.editor.command));
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    terminal.clear()?;
    let status = status?;
    if !status.success() {
        app.set_status(format!("editor exited with {status}"));
    }
    app.index.refresh_path(Path::new(&file))?;
    app.reload_rows()
}

fn watch_roots(
    index: &WorkspaceIndex,
) -> Result<(Receiver<notify::Result<NotifyEvent>>, RecommendedWatcher)> {
    let (sender, receiver) = mpsc::channel();
    let mut watcher = RecommendedWatcher::new(
        move |event| {
            let _ = sender.send(event);
        },
        NotifyConfig::default(),
    )?;
    for root in index.roots().roots() {
        watcher.watch(root, RecursiveMode::Recursive)?;
    }
    Ok((receiver, watcher))
}

fn start_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

fn stop_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    Ok(())
}

fn period_duration(view: AgendaView) -> Duration {
    match view {
        AgendaView::Day | AgendaView::Todo => Duration::days(1),
        AgendaView::Week => Duration::weeks(1),
    }
}

fn section_for(entry: &AgendaEntry) -> String {
    if matches!(entry.occurrence.kind, OccurrenceType::PastScheduled) {
        "SLIPPED"
    } else if entry.occurrence.time.is_some() {
        "TIMED"
    } else if matches!(entry.occurrence.kind, OccurrenceType::Deadline | OccurrenceType::UpcomingDeadline) {
        "DUE"
    } else if matches!(entry.item.todo.as_deref(), Some("NEXT" | "WAIT")) {
        "IN PROGRESS"
    } else if is_inbox(&entry.item) {
        "INBOX"
    } else {
        "TODAY"
    }
    .to_string()
}

fn section_order(section: &str) -> u8 {
    match section {
        "SLIPPED" => 0,
        "TIMED" => 1,
        "DUE" => 2,
        "IN PROGRESS" => 3,
        "INBOX" => 4,
        _ => 5,
    }
}

fn is_inbox(item: &Item) -> bool {
    item.olpath.iter().any(|heading| heading.eq_ignore_ascii_case("inbox"))
        || item.category.eq_ignore_ascii_case("inbox")
}

fn occurrence_prefix(occurrence: Option<&Occurrence>) -> &'static str {
    match occurrence.map(|occurrence| occurrence.kind) {
        Some(OccurrenceType::PastScheduled) => "Sched.:",
        Some(OccurrenceType::Scheduled) => "Scheduled:",
        Some(OccurrenceType::Deadline) => "Deadline:",
        Some(OccurrenceType::UpcomingDeadline) => "In:",
        Some(OccurrenceType::Timestamp) => "",
        Some(OccurrenceType::Block) => "Block:",
        Some(OccurrenceType::Closed) => "Closed:",
        Some(OccurrenceType::Todo) | None => "",
    }
}
fn fuzzy_match(value: &str, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let mut query = query.chars().flat_map(char::to_lowercase);
    let mut wanted = query.next();
    for character in value.chars().flat_map(char::to_lowercase) {
        if Some(character) == wanted {
            wanted = query.next();
            if wanted.is_none() {
                return true;
            }
        }
    }
    false
}


fn view_title(app: &App) -> String {
    match app.screen {
        Screen::Triage => format!("Triage · {} remaining", app.rows.len()),
        Screen::Review => "Agent review".to_string(),
        Screen::Today => match app.view {
            AgendaView::Day => format!("Today · {}", app.day.format("%a %d %b %Y")),
            AgendaView::Week => format!("Week · {}", app.day.format("%d %b %Y")),
            AgendaView::Todo => "Todo".to_string(),
        },
    }
}

fn section_color(section: &str) -> Color {
    if no_color() {
        return Color::Reset;
    }
    match section {
        "SLIPPED" => Color::Red,
        "DUE" => Color::Yellow,
        "IN PROGRESS" => Color::Green,
        _ => accent(),
    }
}

fn accent() -> Color {
    if no_color() {
        return Color::Reset;
    }
    let truecolor = std::env::var("COLORTERM")
        .map(|value| value.contains("truecolor") || value.contains("24bit"))
        .unwrap_or(false);
    if !truecolor {
        return Color::Cyan;
    }
    let light_background = std::env::var("COLORFGBG")
        .ok()
        .and_then(|value| value.rsplit(';').next()?.parse::<u8>().ok())
        .is_some_and(|background| background >= 7);
    if light_background {
        Color::Rgb(0, 95, 115)
    } else {
        Color::Rgb(76, 201, 240)
    }
}

fn selection_style() -> Style {
    if no_color() {
        Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        Style::default()
            .bg(Color::DarkGray)
            .fg(accent())
            .add_modifier(Modifier::BOLD)
    }
}

fn no_color() -> bool {
    std::env::var_os("NO_COLOR").is_some()
}

fn rounded(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(title)
}

fn centered(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn write_context() -> WriteContext {
    WriteContext {
        actor: "tui".to_string(),
        project: std::env::current_dir()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "unknown".to_string()),
    }
}
