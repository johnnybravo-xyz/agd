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
use notify::{
    Config as NotifyConfig, Event as NotifyEvent, RecommendedWatcher, RecursiveMode, Watcher,
};
use org_core::{
    AgendaEngine, AgendaEntry, AgendaQuery, AgendaSpan, Change, EditIntent, Item, JournalRecord,
    JournalStatus, Occurrence, OccurrenceType, Settings, WorkspaceIndex, WriteContext, WriteEngine,
    load_settings, parse_capture_line, parse_date_input,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Scrollbar,
    ScrollbarOrientation, ScrollbarState, Wrap,
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
struct SelectionAnchor {
    id: Option<String>,
    file: String,
    line: usize,
    olpath: Vec<String>,
}

impl SelectionAnchor {
    fn from_item(item: &Item) -> Self {
        Self {
            id: item.id.clone(),
            file: item.file.clone(),
            line: item.line,
            olpath: item.olpath.clone(),
        }
    }

    fn find(&self, rows: &[DisplayRow]) -> Option<usize> {
        if let Some(id) = &self.id {
            return rows.iter().position(|row| row.item.id.as_ref() == Some(id));
        }
        rows.iter()
            .position(|row| {
                row.item.file == self.file
                    && row.item.line == self.line
                    && row.item.olpath == self.olpath
            })
            .or_else(|| {
                rows.iter()
                    .position(|row| row.item.file == self.file && row.item.olpath == self.olpath)
            })
    }
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
    capture_only: bool,
    pending_view_key: bool,
    overlay: Overlay,
    status: String,
    status_at: Instant,
    list_area: Rect,
    inbox_count: usize,
    agent_count: usize,
    slipped_count: usize,
    recent_targets: Vec<String>,
    reschedule_counts: BTreeMap<String, usize>,
    provenance: BTreeMap<String, String>,
    triage_started: Option<Instant>,
    triage_outcomes: [usize; 6],
    review_diff: bool,
    triage_undo: Vec<(Vec<String>, usize)>,
    quit: bool,
}

pub fn run(config: Option<String>, root: Option<String>, pane: bool) -> Result<()> {
    run_mode(config, root, pane, false)
}

pub fn run_capture(config: Option<String>, root: Option<String>) -> Result<()> {
    run_mode(config, root, false, true)
}

fn run_mode(
    config: Option<String>,
    root: Option<String>,
    pane: bool,
    capture_only: bool,
) -> Result<()> {
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
        capture_only,
        pending_view_key: false,
        overlay: if capture_only {
            Overlay::Capture {
                template: None,
                input: String::new(),
                preview: "type a capture template key".to_string(),
                body: String::new(),
            }
        } else {
            Overlay::None
        },
        status: String::new(),
        status_at: Instant::now(),
        list_area: Rect::default(),
        inbox_count: 0,
        agent_count: 0,
        slipped_count: 0,
        recent_targets: Vec::new(),
        triage_started: None,
        provenance: BTreeMap::new(),
        reschedule_counts: BTreeMap::new(),
        triage_outcomes: [0; 6],
        review_diff: false,
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
                    MouseEventKind::Down(_)
                        if app.list_area.contains((mouse.column, mouse.row).into()) =>
                    {
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
            app.screen = Screen::Review;
            app.review_diff = false;
            app.reload_rows()?;
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

impl App {
    fn reload_rows(&mut self) -> Result<()> {
        self.selected = 0;
        self.inbox_count = self
            .index
            .items()
            .filter(|item| item.todo.is_some() && !item.done && is_inbox(item))
            .count();
        let journals = WriteEngine::with_default_journal(self.settings.clone())?.journal()?;
        self.provenance.clear();
        for record in &journals {
            if matches!(record.status, JournalStatus::Committed)
                && let Some(item_ref) = &record.item_ref
            {
                self.provenance
                    .insert(item_ref.clone(), record.actor.clone());
            }
        }
        self.agent_count = journals
            .iter()
            .filter(|record| {
                matches!(record.status, JournalStatus::Committed)
                    && record.actor.starts_with("mcp:")
            })
            .count();
        self.reschedule_counts.clear();
        for record in &journals {
            if matches!(record.status, JournalStatus::Committed)
                && record.diff.contains("SCHEDULED:")
                && let Some(item_ref) = &record.item_ref
            {
                *self.reschedule_counts.entry(item_ref.clone()).or_default() += 1;
            }
        }
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
                    section: if is_inbox(&entry.item) {
                        "INBOX"
                    } else {
                        "SLIPPED"
                    }
                    .to_string(),
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
                    || row
                        .item
                        .title
                        .to_lowercase()
                        .contains(&self.filter.to_lowercase())
                    || row
                        .item
                        .tags
                        .iter()
                        .any(|tag| tag.eq_ignore_ascii_case(&self.filter)))
        });
        if self.screen == Screen::Today {
            self.slipped_count = rows.iter().filter(|row| row.section == "SLIPPED").count();
        }
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
        let selected = self.selected;
        let anchor = self
            .selected_row()
            .map(|row| SelectionAnchor::from_item(&row.item));
        let targets = self.targets();
        let mut journals = Vec::new();
        let is_done = self.settings.todo.sequences.iter().any(|sequence| {
            sequence
                .iter()
                .skip_while(|keyword| keyword.as_str() != "|")
                .skip(1)
                .any(|keyword| keyword.split(['(', '!', '@']).next().unwrap_or(keyword) == state)
        });
        for item_ref in &targets {
            journals.push(self.edit_ref(
                item_ref,
                EditIntent {
                    state: Change::Set(state.to_string()),
                    ..EditIntent::default()
                },
            )?);
        }
        if is_done {
            self.record_triage(journals, 4);
        }
        self.marks.clear();
        self.set_status(format!("{} item(s) → {state} · u undo", targets.len()));
        self.reload_rows()?;
        self.selected = anchor
            .and_then(|anchor| anchor.find(&self.rows))
            .unwrap_or_else(|| selected.min(self.row_count().saturating_sub(1)));
        Ok(())
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
            .map(|keyword| keyword.split(['(', '!', '@']).next().unwrap_or(keyword))
            .collect();
        if active.is_empty() {
            return Ok(());
        }
        let current = row.item.todo.as_deref();
        let index = active
            .iter()
            .position(|keyword| Some(*keyword) == current)
            .unwrap_or(0);
        let next = active[(index + 1) % active.len()].to_string();
        self.apply_state(&next)
    }

    fn cancel_selected(&mut self) -> Result<()> {
        let state = self
            .settings
            .todo
            .sequences
            .iter()
            .flat_map(|sequence| {
                sequence
                    .iter()
                    .skip_while(|keyword| keyword.as_str() != "|")
                    .skip(1)
            })
            .map(|keyword| keyword.split(['(', '!', '@']).next().unwrap_or(keyword))
            .find(|keyword| keyword.contains("CANCEL"))
            .map(str::to_string);
        if let Some(state) = state {
            self.apply_state(&state)
        } else {
            self.set_status("no cancellation state is configured".to_string());
            Ok(())
        }
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
        self.set_status(format!(
            "scheduled {} item(s) {value} · u undo",
            targets.len()
        ));
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
        self.set_status(format!(
            "moved {} item(s) to someday · u undo",
            targets.len()
        ));
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

    fn review_all(&mut self) -> Result<()> {
        let ids: Vec<String> = self
            .journals
            .iter()
            .map(|record| record.id.clone())
            .collect();
        let engine = WriteEngine::with_default_journal(self.settings.clone())?;
        for id in &ids {
            engine.review(id)?;
        }
        self.set_status(format!("reviewed {} agent changes", ids.len()));
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
        let Some(source_ref) = self.selected_row().map(|row| row.item.item_ref.clone()) else {
            return;
        };
        let mut targets: Vec<RefileTarget> = self
            .index
            .items()
            .filter(|item| item.item_ref != source_ref)
            .map(|item| RefileTarget {
                item_ref: item.item_ref.clone(),
                label: format!("{}  {}:{}", item.olpath.join(" / "), item.file, item.line),
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
        self.recent_targets
            .retain(|item_ref| item_ref != &target.item_ref);
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
        *preview = result
            .map(|mutation| added_text(&mutation.diff))
            .unwrap_or_else(|error| error.to_string());
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
        if self.capture_only {
            self.quit = true;
            return Ok(());
        }
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
                for tag in input
                    .split(',')
                    .map(str::trim)
                    .filter(|tag| !tag.is_empty())
                {
                    if let Some(tag) = tag.strip_prefix('-') {
                        intent.tags_remove.insert(tag.to_string());
                    } else {
                        intent
                            .tags_add
                            .insert(tag.trim_start_matches('+').to_string());
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

fn draw_header(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
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
            Span::styled(" agendum ", tab_style(true, accent())),
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

fn tab_style(selected: bool, color: Color) -> Style {
    if selected {
        if no_color() {
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            Style::default()
                .fg(color)
                .bg(Color::Rgb(20, 35, 48))
                .add_modifier(Modifier::BOLD)
        }
    } else {
        Style::default().fg(muted())
    }
}

fn draw_rows(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
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

fn draw_triage(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
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

fn key_chip(key: &str) -> Span<'_> {
    Span::styled(
        format!(" {key} "),
        if no_color() {
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            Style::default()
                .fg(Color::White)
                .bg(Color::Rgb(30, 41, 59))
                .add_modifier(Modifier::BOLD)
        },
    )
}

fn draw_review(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let items: Vec<ListItem<'_>> = app
        .journals
        .iter()
        .map(|record| {
            let agent = record.actor.strip_prefix("mcp:").unwrap_or(&record.actor);
            let time = record.timestamp.get(11..16).unwrap_or(&record.timestamp);
            let kind = if record.diff.lines().any(|line| line.starts_with("+* ")) {
                "captured"
            } else if record.diff.contains("TODO") || record.diff.contains("DONE") {
                "state"
            } else {
                "edited"
            };
            let mut lines = vec![Line::from(vec![
                Span::styled("● ", Style::default().fg(agent_color(agent))),
                Span::styled(
                    agent.to_string(),
                    Style::default()
                        .fg(agent_color(agent))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!("   {}   {time}   ", record.project)),
                Span::styled(kind, Style::default().fg(section_color("IN PROGRESS"))),
            ])];
            lines.extend(
                record
                    .diff
                    .lines()
                    .filter(|line| {
                        (line.starts_with('+') && !line.starts_with("+++"))
                            || (line.starts_with('-') && !line.starts_with("---"))
                    })
                    .take(2)
                    .map(|line| {
                        Line::from(Span::styled(
                            format!("  {line}"),
                            Style::default().fg(if line.starts_with('+') {
                                Color::Green
                            } else {
                                Color::Red
                            }),
                        ))
                    }),
            );
            ListItem::new(lines)
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(app.selected));
    let title = format!("{} unreviewed", app.journals.len());
    let list = List::new(items)
        .block(rounded(&title))
        .highlight_symbol("▌")
        .highlight_style(selection_style());
    frame.render_stateful_widget(list, area, &mut state);
    if app.review_diff
        && let Some(record) = app.journals.get(app.selected)
    {
        let popup = centered(area, 86, 70);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(record.diff.clone())
                .block(rounded("Raw diff · d close · a accept · u undo"))
                .wrap(Wrap { trim: false }),
            popup,
        );
    }
}

fn agent_color(agent: &str) -> Color {
    if no_color() {
        return Color::Reset;
    }
    const COLORS: [Color; 4] = [Color::Magenta, Color::Green, Color::Cyan, Color::Yellow];
    let index = agent
        .bytes()
        .fold(0usize, |value, byte| value.wrapping_add(byte as usize))
        % COLORS.len();
    COLORS[index]
}

fn draw_peek(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
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
    frame.render_widget(Paragraph::new(lines).block(rounded("agendum")), area);
}

fn help_heading(title: &'static str) -> Line<'static> {
    Line::from(Span::styled(
        title,
        Style::default().fg(accent()).add_modifier(Modifier::BOLD),
    ))
}

fn help_shortcut(key: &'static str, description: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!(" {key:<9}"),
            if no_color() {
                Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
            } else {
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Rgb(30, 41, 59))
                    .add_modifier(Modifier::BOLD)
            },
        ),
        Span::raw("  "),
        Span::styled(description, Style::default().fg(Color::White)),
    ])
}

fn draw_help(frame: &mut ratatui::Frame<'_>, area: Rect) {
    let popup = centered_fixed(area, 92, 22);
    frame.render_widget(Clear, popup);
    let panel = rounded("Keyboard shortcuts")
        .style(Style::default().bg(surface()).fg(Color::White))
        .border_style(Style::default().fg(accent()));
    let inner = panel.inner(popup);
    frame.render_widget(panel, popup);
    let content = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(12),
            Constraint::Length(1),
        ])
        .margin(1)
        .split(inner);
    frame.render_widget(
        Paragraph::new("Move quickly, edit safely, and keep context.")
            .style(Style::default().fg(muted())),
        content[0],
    );

    let navigation = vec![
        help_heading("NAVIGATE"),
        help_shortcut("j / k", "Move selection"),
        help_shortcut("enter / e", "Open in editor"),
        help_shortcut("f / b", "Next / previous period"),
        help_shortcut(".", "Return to today"),
        help_shortcut("/", "Filter visible tasks"),
        Line::default(),
        help_heading("VIEWS"),
        help_shortcut("V d/w/t", "Day, week, or todo"),
        help_shortcut("<", "Change context"),
        help_shortcut("p", "Toggle details"),
        help_shortcut("v", "Toggle classic view"),
    ];
    let actions = vec![
        help_heading("EDIT"),
        help_shortcut("t", "Cycle TODO state"),
        help_shortcut("x", "Mark complete"),
        help_shortcut("s / d", "Schedule / deadline"),
        help_shortcut(", / :", "Priority / tags"),
        help_shortcut("space", "Mark for bulk edit"),
        help_shortcut("u", "Undo latest change"),
        Line::default(),
        help_heading("WORKFLOWS"),
        help_shortcut("c", "Capture a task"),
        help_shortcut("T", "Start triage"),
        help_shortcut("A", "Review agent changes"),
        help_shortcut("q", "Quit"),
    ];
    if content[1].width >= 70 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(50),
                Constraint::Length(2),
                Constraint::Percentage(50),
            ])
            .split(content[1]);
        frame.render_widget(Paragraph::new(navigation), columns[0]);
        frame.render_widget(Paragraph::new(actions), columns[2]);
    } else {
        let compact = vec![
            help_heading("NAVIGATE"),
            help_shortcut("j/k", "Move"),
            help_shortcut("enter/e", "Open in editor"),
            help_shortcut("f/b", "Change period"),
            help_shortcut("V d/w/t", "Change view"),
            help_shortcut("/", "Filter"),
            Line::default(),
            help_heading("EDIT & WORKFLOWS"),
            help_shortcut("t / x", "Cycle / complete"),
            help_shortcut("s / d", "Schedule / deadline"),
            help_shortcut("space", "Mark"),
            help_shortcut("u", "Undo"),
            help_shortcut("c / T / A", "Capture / triage / review"),
        ];
        frame.render_widget(Paragraph::new(compact), content[1]);
    }
    frame.render_widget(
        Paragraph::new("Any key closes this panel")
            .style(Style::default().fg(muted()))
            .alignment(ratatui::layout::Alignment::Center),
        content[2],
    );
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

fn open_selected(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> Result<()> {
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
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
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
        "SCHEDULE"
    } else if matches!(
        entry.occurrence.kind,
        OccurrenceType::Deadline | OccurrenceType::UpcomingDeadline
    ) {
        "DUE SOON"
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
        "SCHEDULE" => 1,
        "DUE SOON" => 2,
        "IN PROGRESS" => 3,
        "INBOX" => 4,
        _ => 5,
    }
}

fn is_inbox(item: &Item) -> bool {
    item.olpath
        .iter()
        .any(|heading| heading.eq_ignore_ascii_case("inbox"))
        || item.category.eq_ignore_ascii_case("inbox")
}

fn added_text(diff: &str) -> String {
    let mut text = diff
        .lines()
        .filter_map(|line| {
            if line.starts_with("+++") {
                None
            } else {
                line.strip_prefix('+')
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    text
}

fn occurrence_prefix(occurrence: Option<&Occurrence>) -> String {
    let Some(occurrence) = occurrence else {
        return String::new();
    };
    if !occurrence.extra.is_empty() {
        return occurrence.extra.clone();
    }
    match occurrence.kind {
        OccurrenceType::PastScheduled => "Sched.:",
        OccurrenceType::Scheduled => "Scheduled:",
        OccurrenceType::Deadline => "Deadline:",
        OccurrenceType::UpcomingDeadline => "In:",
        OccurrenceType::Timestamp => "",
        OccurrenceType::Block => "Block:",
        OccurrenceType::Closed => "Closed:",
        OccurrenceType::Todo => "",
    }
    .to_string()
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
        "DUE SOON" => Color::Yellow,
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

fn muted() -> Color {
    if no_color() {
        Color::Reset
    } else {
        Color::Rgb(148, 163, 184)
    }
}

fn surface() -> Color {
    if no_color() {
        Color::Reset
    } else {
        Color::Rgb(15, 23, 32)
    }
}

fn selection_style() -> Style {
    if no_color() {
        Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        Style::default()
            .bg(Color::Rgb(20, 35, 48))
            .fg(Color::White)
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
        .border_style(Style::default().fg(muted()))
        .title(Span::styled(
            format!(" {title} "),
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ))
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

fn centered_fixed(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(2));
    let height = height.min(area.height.saturating_sub(2));
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn write_context() -> WriteContext {
    WriteContext {
        actor: "tui".to_string(),
        project: std::env::current_dir()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "unknown".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use temp_env::with_vars;

    fn test_app(path: &Path, root: &Path) -> App {
        let mut settings = Settings::default();
        settings.files.roots = vec![root.to_string_lossy().into_owned()];
        settings.files.agenda = vec![path.to_string_lossy().into_owned()];
        let index = WorkspaceIndex::load(settings.clone()).unwrap();
        App {
            settings,
            index,
            rows: Vec::new(),
            journals: Vec::new(),
            screen: Screen::Today,
            view: AgendaView::Day,
            day: NaiveDate::from_ymd_opt(2026, 10, 13).unwrap(),
            selected: 0,
            marks: BTreeSet::new(),
            context: 0,
            filter: String::new(),
            classic: false,
            peek: false,
            pane: false,
            capture_only: false,
            pending_view_key: false,
            overlay: Overlay::None,
            status: String::new(),
            status_at: Instant::now(),
            list_area: Rect::default(),
            inbox_count: 0,
            agent_count: 0,
            slipped_count: 0,
            recent_targets: Vec::new(),
            reschedule_counts: BTreeMap::new(),
            provenance: BTreeMap::new(),
            triage_started: None,
            triage_outcomes: [0; 6],
            review_diff: false,
            triage_undo: Vec::new(),
            quit: false,
        }
    }

    #[test]
    #[serial]
    fn cycling_state_keeps_current_position() {
        let directory = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let path = directory.path().join("tasks.org");
        fs::write(
            &path,
            "* TODO One\nSCHEDULED: <2026-10-13 Tue>\n\
             * TODO Two\nSCHEDULED: <2026-10-13 Tue>\n\
             * TODO Three\nSCHEDULED: <2026-10-13 Tue>\n",
        )
        .unwrap();
        with_vars([("HOME", home.path().to_str())], || {
            let mut app = test_app(&path, directory.path());
            app.reload_rows().unwrap();
            app.selected = 1;
            let selected_title = app.selected_row().unwrap().item.title.clone();

            app.cycle_state().unwrap();

            assert_eq!(app.selected_row().unwrap().item.title, selected_title);
            assert_eq!(
                app.selected_row().unwrap().item.todo.as_deref(),
                Some("NEXT")
            );
        });
    }

    #[test]
    fn renders_five_thousand_headings_under_budget() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tasks.org");
        let source = (0..5_000)
            .map(|index| format!("* TODO Task {index}\nSCHEDULED: <2026-10-13 Tue>\n"))
            .collect::<String>();
        fs::write(&path, source).unwrap();
        let mut app = test_app(&path, directory.path());
        app.reload_rows().unwrap();
        assert_eq!(app.rows.len(), 5_000);
        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let started = Instant::now();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let elapsed = started.elapsed();
        eprintln!("5,000-heading render: {elapsed:?}");
        assert!(elapsed < StdDuration::from_millis(100), "{elapsed:?}");
    }
}
