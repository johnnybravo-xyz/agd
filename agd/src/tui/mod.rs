//! Terminal workflows from the TUI design: Today (with Week, Inbox and saved
//! views as tabs), Triage, Capture, Agent review, and the narrow pane.
//! `mod.rs` owns state, the event loop and terminal handling; each screen
//! draws itself from `App`, and every write goes through `actions`.

mod actions;
mod capture;
mod edit;
mod help;
mod input;
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

use agd_core::{
    AgdError, Item, JournalRecord, Occurrence, Settings, WorkspaceIndex, WriteContext,
    load_settings,
};
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
    /// The note Org asks for when a state change is logged with `@`.
    Note,
}

#[derive(Debug)]
enum Overlay {
    None,
    /// Every screen's keys; `scroll` is the first line shown.
    Help {
        scroll: u16,
    },
    Capture(capture::Draft),
    Edit(Box<edit::EditForm>),
    Prompt(actions::Prompt),
    Refile(actions::RefilePicker),
    /// `B`: what to do with every marked row (keys::BULK_KEYS).
    Bulk,
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
    /// A view's configured color name (COLOR_NAMES); None follows the rows.
    color: Option<String>,
    /// agd's own sections show how many rows they hold; an agenda block's
    /// title (`Day-agenda (W42):`) is Org's own text and shows none.
    counted: bool,
}

impl Header {
    fn section(text: String, color: Option<String>) -> Self {
        Self {
            text,
            day: false,
            color,
            counted: true,
        }
    }

    /// An agenda block's title, as Org prints it, with day lines under it.
    fn agenda_block(text: String, color: Option<String>) -> Self {
        Self {
            counted: false,
            ..Self::section(text, color)
        }
    }
}

/// One selectable row. `section` groups rows under a header: a Today
/// section (OVERDUE, SCHEDULE, …), a day in Week, or a view block title.
#[derive(Clone, Debug)]
struct DisplayRow {
    section: String,
    item: Item,
    occurrence: Option<Occurrence>,
    /// Overdue rows folded under this one: when above zero the row is
    /// Today's "+N more" line, which ⏎ expands, and no key acts on `item`.
    more: usize,
}

/// The full layout needs this many columns; under it only the pane fits.
const FULL_WIDTH: u16 = 60;

const OVERDUE: &str = "OVERDUE";
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
    overdue: usize,
    inbox: usize,
    agents: usize,
}

/// Progress of one triage pass; outcomes index the actions::TODAY… slots.
#[derive(Debug, Default)]
struct TriageState {
    started: Option<Instant>,
    /// When the last decision was written, for the pass's duration.
    last_decision: Option<Instant>,
    outcomes: [usize; 7],
    undo: Vec<actions::TriageStep>,
    /// The cards as the pass began. A decision never adds, drops or
    /// reorders them, so a task still overdue after `m` (a missed
    /// deadline) does not come back, and the counter stays `3 / 8`.
    queue: Vec<DisplayRow>,
    /// The card on view; `queue.len()` once the pass is through.
    position: usize,
    /// The position of each decision, in order, for undo to return to.
    decided: Vec<usize>,
}

impl TriageState {
    fn finished(&self) -> bool {
        self.position >= self.queue.len()
    }
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
    /// Today shows every OVERDUE row, not the first few; reset on leaving.
    overdue_expanded: bool,
    /// Tomorrow's open (scheduled, due) counts, which an empty Today shows.
    tomorrow: (usize, usize),
    /// Context filter cycled with `<`: None is all categories.
    context: Option<String>,
    filter: String,
    classic: bool,
    log_mode: bool,
    peek: bool,
    /// The narrow pane is showing: asked for with --pane, or the terminal
    /// is under FULL_WIDTH columns.
    pane: bool,
    pane_requested: bool,
    /// Terminal columns at the last draw.
    width: u16,
    capture_only: bool,
    overlay: Overlay,
    status: Option<Status>,
    list_area: Rect,
    /// (screen row, row index) of the drawn list, for mouse clicks.
    click_rows: Vec<(u16, usize)>,
    counts: Counts,
    /// Reschedules per stable_ref: LOGBOOK "Rescheduled from" notes, else
    /// agd's own journal.
    reschedules: BTreeMap<String, usize>,
    /// stable_ref -> (actor, HH:MM) of the capture that created it.
    provenance: BTreeMap<String, (String, String)>,
    recent_targets: Vec<String>,
    /// Writes this TUI made, newest last, for `u`.
    undo_steps: Vec<actions::UndoStep>,
    triage: TriageState,
    review_diff: bool,
    agent_filter: Option<String>,
    /// When `C-c` started a chord in the lists or Triage: `C-w` next
    /// refiles, as in Org; `C-c` again, or a second without a key, quits.
    chord: Option<Instant>,
    quit: bool,
}

impl App {
    fn new(settings: Settings, index: WorkspaceIndex, today: NaiveDate) -> Self {
        let theme = Theme::detect(&settings.theme, None).rounded_pills(settings.nerd_font);
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
            overdue_expanded: false,
            tomorrow: (0, 0),
            context: None,
            filter: String::new(),
            classic: false,
            log_mode: false,
            peek: false,
            pane: false,
            pane_requested: false,
            width: 0,
            capture_only: false,
            overlay: Overlay::None,
            status: None,
            list_area: Rect::default(),
            click_rows: Vec::new(),
            counts: Counts::default(),
            reschedules: BTreeMap::new(),
            provenance: BTreeMap::new(),
            recent_targets: Vec::new(),
            undo_steps: Vec::new(),
            triage: TriageState::default(),
            review_diff: false,
            agent_filter: None,
            chord: None,
            quit: false,
        }
    }

    /// Leaves the pane for the full view, or says why it cannot: under
    /// FULL_WIDTH columns the event loop would switch straight back.
    fn leave_pane(&mut self) -> Result<bool> {
        if self.width < FULL_WIDTH {
            self.set_status(format!("widen to {FULL_WIDTH}+ columns for the full view"));
            return Ok(false);
        }
        self.pane = false;
        // From now on a narrow terminal brings the pane back.
        self.pane_requested = false;
        self.reload_rows()?;
        Ok(true)
    }

    /// The palette for the background the terminal reported.
    fn set_background(&mut self, light: bool) {
        self.theme =
            Theme::detect(&self.settings.theme, Some(light)).rounded_pills(self.settings.nerd_font);
    }

    /// A lone `C-c` still quits, a second after it was pressed.
    fn chord_expired(&self) -> bool {
        self.chord
            .is_some_and(|started| started.elapsed() > StdDuration::from_secs(1))
    }

    fn set_status(&mut self, text: impl Into<String>) {
        self.status = Some(Status {
            text: text.into(),
            error: false,
            at: Instant::now(),
        });
    }

    /// A refused write or a failure, as a sentence without error codes.
    fn set_error(&mut self, error: &anyhow::Error) {
        let text = match error.downcast_ref::<AgdError>() {
            // Here `r` re-reads the files, which settles a stale heading.
            Some(error) if error.code() == "stale_ref" => {
                format!("✗ {}; r refreshes", crate::words::error_sentence(error))
            }
            Some(error) => format!("✗ {}", crate::words::error_sentence(error)),
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
        crate::words::location(&item.file, item.line, self.index.roots().roots())
    }
}

pub fn run(config: Option<String>, root: Option<String>, pane: bool) -> Result<()> {
    run_mode(config, root, pane, false)
}

pub fn run_capture(config: Option<String>, root: Option<String>) -> Result<()> {
    run_mode(config, root, false, true)
}

/// The TUI's colour roles for each palette, so other clients draw with the
/// same colours (theme.rs is their one definition).
pub fn palettes() -> serde_json::Value {
    theme::palettes_json()
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
    let (light, reply, typed) = theme::ask_background(&settings.theme);
    let mut app = App::new(settings, index, Local::now().date_naive());
    if let Some(light) = light {
        app.set_background(light);
    }
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
    let result = event_loop(&mut terminal, &mut app, &events, reply, typed);
    stop_terminal(&mut terminal)?;
    result
}

/// `reply` is the terminal's background answer if it is still on its way;
/// `typed` holds what arrived while agd waited for it.
fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    changes: &Receiver<notify::Result<NotifyEvent>>,
    mut reply: theme::BackgroundReply,
    typed: Vec<Event>,
) -> Result<()> {
    for event in typed {
        handle_event(terminal, app, event);
    }
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
        // Under FULL_WIDTH columns the full layout cannot fit; switch to the
        // pane (and back when the terminal grows again).
        let narrow = terminal.size()?.width < FULL_WIDTH;
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
        if app.chord_expired() {
            break;
        }
        terminal.draw(|frame| draw(frame, app))?;
        if !event::poll(StdDuration::from_millis(200))? {
            continue;
        }
        // Handle every event already waiting before drawing again, so a
        // pasted line or fast typing costs one redraw, not one per key.
        handle_input(terminal, app, &mut reply, event::read()?);
        while !app.quit && event::poll(StdDuration::ZERO)? {
            handle_input(terminal, app, &mut reply, event::read()?);
        }
    }
    Ok(())
}

/// A key of a late background answer is dropped, and the whole answer sets
/// the palette; every other event is handled.
fn handle_input(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    reply: &mut theme::BackgroundReply,
    event: Event,
) {
    if let Event::Key(key) = &event {
        match reply.feed(key) {
            theme::ReplyKey::Part => return,
            theme::ReplyKey::End(light) => {
                if let Some(light) = light {
                    app.set_background(light);
                }
                return;
            }
            theme::ReplyKey::Other => {}
        }
    }
    handle_event(terminal, app, event);
}

fn handle_event(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App, event: Event) {
    match event {
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
                    if app.pane
                        && already
                        && let Err(error) = app.leave_pane()
                    {
                        app.set_error(&error);
                    }
                }
            }
            _ => {}
        },
        _ => {}
    }
}

fn draw(frame: &mut ratatui::Frame<'_>, app: &mut App) {
    // A popup's preview is a dry-run write: build it once per draw, not
    // once per key.
    capture::refresh_stale(app);
    edit::refresh_stale(app);
    actions::refresh_stale_prompt(app);
    let area = frame.area();
    app.width = area.width;
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
        Overlay::Help { .. } => help::draw(frame, app, area),
        Overlay::Capture(_) => capture::draw(frame, app, area),
        Overlay::Edit(_) => edit::draw(frame, app, area),
        Overlay::Prompt(_) | Overlay::Refile(_) | Overlay::Bulk | Overlay::Views { .. } => {
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

/// An item ref without its line hint (`file#sha256:digest`), or the ID
/// itself: what stays the same while lines above the heading come and go,
/// so labels keyed by it survive edits elsewhere in the file.
fn stable_ref(item_ref: &str) -> String {
    match item_ref.rsplit_once("#sha256:") {
        Some((location, digest)) => {
            let file = location.rsplit_once(':').map_or(location, |(file, _)| file);
            format!("{file}#sha256:{digest}")
        }
        None => item_ref.to_string(),
    }
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
