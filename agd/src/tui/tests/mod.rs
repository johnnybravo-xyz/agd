//! Tests that draw the TUI on Ratatui's TestBackend and check what is
//! drawn and what is written, one file per screen. This file holds the
//! shared fixtures: a sandboxed home, an App over a temporary org tree,
//! rendering to text or cells, and pressing keys as the event loop does.

mod agents;
mod capture;
mod edit;
mod help;
mod pane;
mod prompts;
mod theme;
mod today;
mod triage;

use std::fs;
use std::path::Path;
use std::time::{Duration as StdDuration, Instant};

use agd_core::{Settings, WorkspaceIndex};
use chrono::NaiveDate;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use serial_test::serial;
use temp_env::with_vars;

use crate::tui::{App, Overlay, Screen, draw};

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 13).unwrap()
}

fn test_app(root: &Path) -> App {
    let settings = Settings {
        roots: vec![root.to_string_lossy().into_owned()],
        agenda_files: vec!["*.org".to_string()],
        ..Settings::default()
    };
    let index = WorkspaceIndex::load(settings.clone()).unwrap();
    App::new(settings, index, today())
}

/// Renders the app into a text grid, one string per terminal row.
fn render(app: &mut App, width: u16, height: u16) -> Vec<String> {
    let buffer = render_buffer(app, width, height);
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect()
}

/// Renders the app and returns the cells, for checking styles.
fn render_buffer(app: &mut App, width: u16, height: u16) -> ratatui::buffer::Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| draw(frame, app)).unwrap();
    terminal.backend().buffer().clone()
}

/// The terminal row whose text contains `needle`.
fn row_of(buffer: &ratatui::buffer::Buffer, needle: &str) -> u16 {
    let area = buffer.area;
    (0..area.height)
        .find(|y| {
            (0..area.width)
                .map(|x| buffer[(x, *y)].symbol())
                .collect::<String>()
                .contains(needle)
        })
        .unwrap_or_else(|| panic!("no row contains {needle}"))
}

/// The first column of `needle` on row `y` (one cell per character).
fn column_of(buffer: &ratatui::buffer::Buffer, y: u16, needle: &str) -> u16 {
    let cells: Vec<&str> = (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect();
    let wanted: Vec<String> = needle.chars().map(String::from).collect();
    cells
        .windows(wanted.len())
        .position(|window| window.iter().zip(&wanted).all(|(cell, want)| cell == want))
        .unwrap_or_else(|| panic!("{needle} is not on row {y}")) as u16
}

/// A colour theme as the terminal would pick it, ignoring with_home's
/// NO_COLOR.
fn theme_for(colorterm: Option<&str>) -> crate::tui::theme::Theme {
    with_vars(
        [("NO_COLOR", None::<&str>), ("COLORTERM", colorterm)],
        || crate::tui::theme::Theme::detect("dark", None),
    )
}

fn with_home(body: impl FnOnce(&Path)) {
    let home = tempfile::tempdir().unwrap();
    let directory = tempfile::tempdir().unwrap();
    with_vars(
        [
            ("HOME", home.path().to_str()),
            ("XDG_DATA_HOME", home.path().to_str()),
            ("NO_COLOR", Some("1")),
        ],
        || body(directory.path()),
    );
}

const TASKS: &str = "#+TODO: TODO NEXT WAIT | DONE\n\
* TODO Migrate cron jobs\nSCHEDULED: <2026-10-01 Thu>\n\
* DONE Call with bank\nSCHEDULED: <2026-10-13 Tue 10:00>\n\
* NEXT Review ingestion PR :review:\nSCHEDULED: <2026-10-13 Tue 15:00>\n\
* TODO Quarterly report\nDEADLINE: <2026-10-16 Fri>\n\
* WAIT Vendor reply\n";

/// Where the default capture template lands: inbox.org under Inbox.
const INBOX: &str = "* Inbox\n** Compare jiff and chrono\n";

/// TASKS in tasks.org and an inbox with one entry in inbox.org.
fn write_tasks(root: &Path) {
    fs::write(root.join("tasks.org"), TASKS).unwrap();
    fs::write(root.join("inbox.org"), INBOX).unwrap();
}

#[derive(serde::Deserialize)]
struct ViewsOnly {
    view: Vec<agd_core::ViewSettings>,
}

/// Presses one key as the event loop would; returns the $EDITOR request.
fn press(app: &mut App, code: crossterm::event::KeyCode) -> crate::tui::keys::EditorTarget {
    crate::tui::keys::dispatch(app, crossterm::event::KeyEvent::from(code)).unwrap()
}

/// Presses each character in turn.
fn type_keys(app: &mut App, keys: &str) {
    for character in keys.chars() {
        press(app, crossterm::event::KeyCode::Char(character));
    }
}

/// Captures `title` into inbox.org as an MCP agent would.
fn agent_capture(app: &App, root: &Path, title: &str) {
    let mut settings = app.settings.clone();
    settings.captures[0].file = Some(root.join("inbox.org").to_string_lossy().into_owned());
    let draft = agd_core::parse_capture_line(title, today(), agd_core::Notation::Agd).unwrap();
    let context = agd_core::WriteContext {
        actor: "mcp:test-agent".to_string(),
        project: root.to_string_lossy().into_owned(),
        now: None,
    };
    agd_core::WriteEngine::with_default_journal(settings)
        .unwrap()
        .capture("t", &draft, "", &Default::default(), false, &context)
        .unwrap();
}

const THREE_TODAY: &str = "* TODO One\nSCHEDULED: <2026-10-13 Tue>\n\
* TODO Two\nSCHEDULED: <2026-10-13 Tue>\n\
* TODO Three\nSCHEDULED: <2026-10-13 Tue>\n";

const TWO_OVERDUE: &str = "#+TODO: TODO | DONE CANCELLED\n\
* TODO Old one\nSCHEDULED: <2026-10-01 Thu>\n\
* TODO Old two\nSCHEDULED: <2026-10-05 Mon>\n";

fn press_control(app: &mut App, character: char) {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let key = KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL);
    crate::tui::keys::dispatch(app, key).unwrap();
}

const FIVE_OVERDUE: &str = "* TODO Sched old\nSCHEDULED: <2026-10-01 Thu>\n\
* TODO Sched older\nSCHEDULED: <2026-09-20 Sun>\n\
* TODO Sched recent\nSCHEDULED: <2026-10-10 Sat>\n\
* TODO Missed filing\nDEADLINE: <2026-10-12 Mon>\n\
* TODO Missed renewal\nDEADLINE: <2026-10-08 Thu>\n";
