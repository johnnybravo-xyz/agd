use std::fs;
use std::path::Path;
use std::time::{Duration as StdDuration, Instant};

use chrono::NaiveDate;
use org_core::{Settings, WorkspaceIndex};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use serial_test::serial;
use temp_env::with_vars;

use super::{App, Overlay, Screen, capture, draw};

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
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| draw(frame, app)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect()
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
* WAIT Vendor reply\n\
* Inbox\n** Compare jiff and chrono\n";

#[test]
#[serial]
fn today_groups_sections_in_the_order_you_act_on_them() {
    with_home(|root| {
        fs::write(root.join("tasks.org"), TASKS).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        let sections: Vec<&str> = app.rows.iter().map(|row| row.section.as_str()).collect();
        assert_eq!(
            sections,
            [
                "SLIPPED", "SCHEDULE", "SCHEDULE", "DUE SOON", "DOING", "INBOX"
            ]
        );
        let screen = render(&mut app, 120, 30).join("\n");
        assert!(screen.contains("SLIPPED"), "{screen}");
        assert!(screen.contains("12x"), "{screen}");
        assert!(screen.contains("in 3d"), "{screen}");
        assert!(screen.contains("Compare jiff and chrono"), "{screen}");
    });
}

#[test]
#[serial]
fn pane_sorts_by_clock_time_and_keeps_done_rows_last() {
    with_home(|root| {
        fs::write(root.join("tasks.org"), TASKS).unwrap();
        let mut app = test_app(root);
        app.pane = true;
        app.reload_rows().unwrap();
        let titles: Vec<&str> = app.rows.iter().map(|row| row.item.title.as_str()).collect();
        assert_eq!(
            titles,
            ["Review ingestion PR", "Quarterly report", "Call with bank"]
        );
        let screen = render(&mut app, 35, 12).join("\n");
        assert!(screen.contains("1 slipped"), "{screen}");
        assert!(screen.contains("inbox 1"), "{screen}");
    });
}

#[test]
#[serial]
fn cycling_state_keeps_the_cursor_on_the_same_heading() {
    with_home(|root| {
        let today = "SCHEDULED: <2026-10-13 Tue>";
        fs::write(
            root.join("tasks.org"),
            format!("* TODO One\n{today}\n* TODO Two\n{today}\n* TODO Three\n{today}\n"),
        )
        .unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        app.selected = 1;
        app.cycle_state().unwrap();
        let row = app.selected_row().unwrap();
        assert_eq!(row.item.title, "Two");
        assert_eq!(row.item.todo.as_deref(), Some("NEXT"));
    });
}

#[test]
#[serial]
fn external_change_keeps_the_cursor_on_the_same_heading() {
    with_home(|root| {
        let path = root.join("tasks.org");
        let today = "SCHEDULED: <2026-10-13 Tue>";
        fs::write(&path, format!("* TODO One\n{today}\n* TODO Two\n{today}\n")).unwrap();
        let mut app = test_app(root);
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
#[serial]
fn refused_write_reports_the_error_and_keeps_rows() {
    with_home(|root| {
        let path = root.join("tasks.org");
        fs::write(&path, "* TODO One\nSCHEDULED: <2026-10-13 Tue>\n").unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        fs::write(&path, "* TODO Renamed\nSCHEDULED: <2026-10-13 Tue>\n").unwrap();
        let error = app.mark_done().unwrap_err();
        app.set_error(&error);
        assert!(app.status.as_ref().unwrap().text.contains("stale_ref"));
        assert_eq!(app.rows.len(), 1);
    });
}

#[test]
#[serial]
fn capture_preview_is_the_exact_text_that_will_be_written() {
    with_home(|root| {
        fs::write(root.join("inbox.org"), "* Inbox\n").unwrap();
        let mut app = test_app(root);
        app.settings.captures[0].file = Some(root.join("inbox.org").to_string_lossy().into_owned());
        app.overlay = Overlay::Capture(capture::Draft::default());
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        for key in "tRenew passport due:fri +personal #A".chars() {
            capture::type_key(&mut app, key);
        }
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let screen: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(
            screen.contains("TODO [#A] Renew passport :personal:"),
            "{screen}"
        );
        assert!(screen.contains("DEADLINE: <2026-10-16 Fri>"), "{screen}");
        assert_eq!(
            fs::read_to_string(root.join("inbox.org")).unwrap(),
            "* Inbox\n"
        );
    });
}

#[test]
#[serial]
fn no_color_renders_state_with_modifiers_only() {
    with_home(|root| {
        fs::write(root.join("tasks.org"), TASKS).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let colored = terminal.backend().buffer().content().iter().any(|cell| {
            cell.fg != ratatui::style::Color::Reset || cell.bg != ratatui::style::Color::Reset
        });
        assert!(!colored);
    });
}

#[test]
fn renders_five_thousand_headings_under_budget() {
    let directory = tempfile::tempdir().unwrap();
    let source = (0..5_000)
        .map(|index| format!("* TODO Task {index}\nSCHEDULED: <2026-10-13 Tue>\n"))
        .collect::<String>();
    fs::write(directory.path().join("tasks.org"), source).unwrap();
    let mut app = test_app(directory.path());
    app.screen = Screen::Week;
    app.reload_rows().unwrap();
    assert_eq!(app.rows.len(), 5_000);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let started = Instant::now();
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let elapsed = started.elapsed();
    assert!(elapsed < StdDuration::from_millis(100), "{elapsed:?}");
}
