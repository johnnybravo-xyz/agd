mod actions;
mod help;
mod keys;
mod overlay;
mod pane;
mod review;
mod rows;
mod theme;
mod today;
mod triage;

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Stdout};
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
use keys::handle_key;
use notify::{
    Config as NotifyConfig, Event as NotifyEvent, RecommendedWatcher, RecursiveMode, Watcher,
};
use org_core::{
    AgdError, Item, JournalRecord, Occurrence, Settings, WorkspaceIndex, WriteContext,
    load_settings,
};
use overlay::draw_overlay;
use pane::draw_pane;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use today::draw_full;

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

fn error_status(error: &anyhow::Error) -> String {
    match error.downcast_ref::<AgdError>() {
        Some(error) => format!("✗ {}: {error}", error.code()),
        None => format!("✗ {error:#}"),
    }
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
                    if let Err(error) = app.reload_rows() {
                        app.set_status(error_status(&error));
                    }
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
                Event::Key(key) => {
                    // A refused write (stale_ref, editor_dirty, a bad date) is
                    // reported in place; only terminal failures end the TUI.
                    if let Err(error) = handle_key(terminal, app, key) {
                        app.set_status(error_status(&error));
                    }
                }
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

fn draw(frame: &mut ratatui::Frame<'_>, app: &mut App) {
    let area = frame.area();
    if app.pane || area.width < 60 {
        draw_pane(frame, app, area);
    } else {
        draw_full(frame, app, area);
    }
    draw_overlay(frame, app, area);
}

fn open_selected(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> Result<()> {
    let Some(row) = app.selected_row() else {
        return Ok(());
    };
    let file = row.item.file.clone();
    let args = app.settings.editor.args_for(&file, row.item.line, 1);
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
    use std::fs;

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
    #[serial]
    fn external_change_keeps_the_cursor_on_the_same_heading() {
        let directory = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let path = directory.path().join("tasks.org");
        let today = "SCHEDULED: <2026-10-13 Tue>";
        fs::write(&path, format!("* TODO One\n{today}\n* TODO Two\n{today}\n")).unwrap();
        with_vars([("HOME", home.path().to_str())], || {
            let mut app = test_app(&path, directory.path());
            app.reload_rows().unwrap();
            app.selected = 1;
            fs::write(
                &path,
                format!("* TODO Zero\n{today}\n* TODO One\n{today}\n* TODO Two\n{today}\n"),
            )
            .unwrap();
            app.index.refresh_path(&path).unwrap();
            app.reload_rows().unwrap();
            assert_eq!(app.selected_row().unwrap().item.title, "Two");
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
