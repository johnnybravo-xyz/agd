//! Terminal workflows from the TUI design: Today (with Week, Inbox and saved
//! views as tabs), Triage, Capture, Agent review, and the narrow pane.
//! `mod.rs` owns state, the event loop and terminal handling; each screen
//! draws itself from `App`, and every write goes through `actions`.

mod actions;
mod capture;
mod edit;
mod help;
mod keys;
mod pane;
mod peek;
mod review;
mod rows;
mod theme;
mod today;
mod triage;

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Stdout, Write};
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration as StdDuration, Instant};

use anyhow::{Context, Result};
use chrono::{Local, NaiveDate};
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, MouseEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use notify::{
    Config as NotifyConfig, Event as NotifyEvent, RecommendedWatcher, RecursiveMode, Watcher,
};
use org_core::{
    AgdError, Item, JournalRecord, Occurrence, Settings, WorkspaceIndex, WriteContext,
    load_settings,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;

use theme::Theme;

/// Screens and tabs. Today, Week, Inbox and saved views share the list
/// layout; Triage and Review have their own.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Screen {
    Today,
    Week,
    Inbox,
    View(String),
    Triage,
    Review,
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
    Capture(capture::Draft),
    Edit(Box<edit::EditForm>),
    Prompt {
        kind: PromptKind,
        input: String,
    },
    Refile(actions::RefilePicker),
    /// org-agenda's dispatcher: saved views by key.
    Views {
        selected: usize,
    },
}

/// A list header, matched to rows by `text`. Sections (Today's, a view's
/// blocks and groups) stand out; day lines under them stay plain, as in
/// org-agenda.
#[derive(Clone, Debug)]
struct Header {
    text: String,
    day: bool,
}

impl Header {
    fn section(text: String) -> Self {
        Self { text, day: false }
    }
}

/// One selectable row. `section` groups rows under a header: a Today
/// section (SLIPPED, SCHEDULE, …), a day in Week, or a view block title.
#[derive(Clone, Debug)]
struct DisplayRow {
    section: String,
    item: Item,
    occurrence: Option<Occurrence>,
}

const SLIPPED: &str = "SLIPPED";
const SCHEDULE: &str = "SCHEDULE";
const DUE: &str = "DUE SOON";
const DOING: &str = "DOING";
const INBOX: &str = "INBOX";

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

/// Status line text; writes confirm for three seconds, errors stay a
/// little longer.
#[derive(Debug)]
struct Status {
    text: String,
    error: bool,
    at: Instant,
}

/// Cross-screen signals shown in tabs, the footer and the pane.
#[derive(Debug, Default)]
struct Counts {
    slipped: usize,
    inbox: usize,
    agents: usize,
}

/// Progress of one triage pass; outcomes index TriageOutcome.
#[derive(Debug, Default)]
struct TriageState {
    started: Option<Instant>,
    outcomes: [usize; 7],
    undo: Vec<(Vec<String>, usize)>,
}

struct App {
    settings: Settings,
    index: WorkspaceIndex,
    theme: Theme,
    today: NaiveDate,
    /// First day of the Today/Week agenda; f, b and . move it.
    day: NaiveDate,
    screen: Screen,
    /// Where esc returns to from Triage, Review or a view.
    back: Screen,
    rows: Vec<DisplayRow>,
    /// Headers in display order, including days without rows, so Week
    /// shows every day as org-agenda does.
    headers: Vec<Header>,
    journals: Vec<JournalRecord>,
    selected: usize,
    marks: BTreeSet<String>,
    /// Context filter cycled with `<`: None is all categories.
    context: Option<String>,
    filter: String,
    classic: bool,
    log_mode: bool,
    peek: bool,
    /// The narrow pane is showing: asked for with --pane, or the terminal
    /// is under 60 columns.
    pane: bool,
    pane_requested: bool,
    capture_only: bool,
    overlay: Overlay,
    status: Option<Status>,
    list_area: Rect,
    /// (screen row, row index) of the drawn list, for mouse clicks.
    click_rows: Vec<(u16, usize)>,
    counts: Counts,
    /// Reschedules per item ref: LOGBOOK "Rescheduled from" notes, else
    /// agd's own journal.
    reschedules: BTreeMap<String, usize>,
    /// item ref -> (actor, HH:MM) of the capture that created it.
    provenance: BTreeMap<String, (String, String)>,
    recent_targets: Vec<String>,
    triage: TriageState,
    review_diff: bool,
    agent_filter: Option<String>,
    quit: bool,
}

impl App {
    fn new(settings: Settings, index: WorkspaceIndex, today: NaiveDate) -> Self {
        let theme = Theme::detect(&settings.theme).rounded_pills(settings.nerd_font);
        Self {
            settings,
            index,
            theme,
            today,
            day: today,
            screen: Screen::Today,
            back: Screen::Today,
            rows: Vec::new(),
            headers: Vec::new(),
            journals: Vec::new(),
            selected: 0,
            marks: BTreeSet::new(),
            context: None,
            filter: String::new(),
            classic: false,
            log_mode: false,
            peek: false,
            pane: false,
            pane_requested: false,
            capture_only: false,
            overlay: Overlay::None,
            status: None,
            list_area: Rect::default(),
            click_rows: Vec::new(),
            counts: Counts::default(),
            reschedules: BTreeMap::new(),
            provenance: BTreeMap::new(),
            recent_targets: Vec::new(),
            triage: TriageState::default(),
            review_diff: false,
            agent_filter: None,
            quit: false,
        }
    }

    fn set_status(&mut self, text: impl Into<String>) {
        self.status = Some(Status {
            text: text.into(),
            error: false,
            at: Instant::now(),
        });
    }

    fn set_error(&mut self, error: &anyhow::Error) {
        let text = match error.downcast_ref::<AgdError>() {
            Some(error) => format!("✗ {}: {error}", error.code()),
            None => format!("✗ {error:#}"),
        };
        self.status = Some(Status {
            text,
            error: true,
            at: Instant::now(),
        });
    }

    fn row_at(&self, column: u16, row: u16) -> Option<usize> {
        if !self.list_area.contains((column, row).into()) {
            return None;
        }
        self.click_rows
            .iter()
            .find(|(line, _)| *line == row)
            .map(|(_, index)| *index)
    }

    /// `file:line` relative to the files root it lives in.
    fn short_location(&self, item: &Item) -> String {
        let path = Path::new(&item.file);
        let relative = self
            .index
            .roots()
            .roots()
            .iter()
            .find_map(|root| path.strip_prefix(root).ok())
            .unwrap_or(path);
        format!("{}:{}", relative.display(), item.line)
    }
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
    let settings = load_settings(
        config.as_deref().map(Path::new),
        root.as_deref().map(Path::new),
    )?
    .settings;
    let index = WorkspaceIndex::load(settings.clone())?;
    let (events, _watcher) = watch_roots(&index)?;
    let mut app = App::new(settings, index, Local::now().date_naive());
    app.pane = pane;
    app.pane_requested = pane;
    app.capture_only = capture_only;
    if capture_only {
        app.overlay = Overlay::Capture(capture::Draft::default());
    } else if !pane && !app.settings.start_view.is_empty() {
        // esc from the start view goes to Today, as from any view.
        app.screen = Screen::View(app.settings.start_view.clone());
    }
    app.reload_rows()?;
    let skipped: Vec<String> = app
        .index
        .skipped_files()
        .map(|(_, reason)| format!("skipped {reason}"))
        .collect();
    if let Some(reason) = skipped.into_iter().next() {
        app.set_status(reason);
    }

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
        let mut changed = false;
        for event in changes.try_iter() {
            match event {
                Ok(event) => {
                    for path in event.paths {
                        if let Err(error) = app.index.refresh_path(&path) {
                            app.set_status(format!("reload failed: {error}"));
                        }
                    }
                    changed = true;
                }
                Err(error) => app.set_status(format!("watch failed: {error}")),
            }
        }
        let today = Local::now().date_naive();
        if today != app.today {
            // Past midnight: done items leave and the agenda moves on.
            app.day += today - app.today;
            app.today = today;
            changed = true;
        }
        // Under 60 columns the full layout cannot fit; switch to the pane
        // (and back when the terminal grows again).
        let narrow = terminal.size()?.width < 60;
        if !app.pane_requested && app.pane != narrow {
            app.pane = narrow;
            changed = true;
        }
        if changed && let Err(error) = app.reload_rows() {
            app.set_error(&error);
        }
        let expired = app.status.as_ref().is_some_and(|status| {
            status.at.elapsed() > StdDuration::from_secs(if status.error { 6 } else { 3 })
        });
        if expired {
            app.status = None;
        }
        terminal.draw(|frame| draw(frame, app))?;
        if !event::poll(StdDuration::from_millis(200))? {
            continue;
        }
        match event::read()? {
            Event::Key(key) => {
                // A refused write (stale_ref, editor_dirty, a bad date) is
                // reported in place; only terminal failures end the TUI.
                if let Err(error) = keys::handle_key(terminal, app, key) {
                    app.set_error(&error);
                }
            }
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollDown => app.move_selection(1),
                MouseEventKind::ScrollUp => app.move_selection(-1),
                MouseEventKind::Down(_) => {
                    if let Some(index) = app.row_at(mouse.column, mouse.row) {
                        let already = index == app.selected;
                        app.selected = index;
                        // Clicking the selected pane row opens the full TUI there.
                        if app.pane && already {
                            app.pane = false;
                            if let Err(error) = app.reload_rows() {
                                app.set_error(&error);
                            }
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    Ok(())
}

fn draw(frame: &mut ratatui::Frame<'_>, app: &mut App) {
    let area = frame.area();
    if app.capture_only {
        capture::draw(frame, app, area);
        return;
    }
    if app.pane {
        pane::draw(frame, app, area);
    } else {
        today::draw(frame, app, area);
    }
    match &app.overlay {
        Overlay::None => {}
        Overlay::Help => help::draw(frame, app, area),
        Overlay::Capture(_) => capture::draw(frame, app, area),
        Overlay::Edit(_) => edit::draw(frame, app, area),
        Overlay::Prompt { .. } | Overlay::Refile(_) | Overlay::Views { .. } => {
            today::draw_overlay(frame, app, area)
        }
    }
}

/// Leaves the alternate screen, runs the configured editor directly (never
/// through a shell), and re-reads the file on return.
fn open_in_editor(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    file: &str,
    line: usize,
) -> Result<()> {
    let (program, args) = app.settings.editor_command(file, line, 1);
    stop_terminal(terminal)?;
    let status = Command::new(&program)
        .args(args)
        .status()
        .with_context(|| format!("cannot start editor {program}"));
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    terminal.clear()?;
    let status = status?;
    if !status.success() {
        app.set_status(format!("editor exited with {status}"));
    }
    if Path::new(file).exists() {
        app.index.refresh_path(Path::new(file))?;
    }
    app.reload_rows()
}

/// Copies text through the terminal (OSC 52), which works over SSH and
/// inside tmux or herdr without a clipboard helper.
fn copy_to_clipboard(text: &str) -> Result<()> {
    let encoded = base64_encode(text.as_bytes());
    let sequence = if std::env::var_os("TMUX").is_some() {
        format!("\x1bPtmux;\x1b\x1b]52;c;{encoded}\x07\x1b\\")
    } else {
        format!("\x1b]52;c;{encoded}\x07")
    };
    let mut stdout = io::stdout();
    stdout.write_all(sequence.as_bytes())?;
    stdout.flush()?;
    Ok(())
}

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let value = chunk.iter().enumerate().fold(0u32, |value, (index, byte)| {
            value | (u32::from(*byte) << (16 - 8 * index))
        });
        for index in 0..4 {
            if index <= chunk.len() {
                output.push(ALPHABET[((value >> (18 - 6 * index)) & 63) as usize] as char);
            } else {
                output.push('=');
            }
        }
    }
    output
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

fn write_context() -> WriteContext {
    WriteContext {
        actor: "tui".to_string(),
        project: std::env::current_dir()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "unknown".to_string()),
        now: None,
    }
}

#[cfg(test)]
mod tests;
