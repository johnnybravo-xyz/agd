use std::fs;
use std::path::Path;
use std::time::{Duration as StdDuration, Instant};

use agd_core::{Settings, WorkspaceIndex};
use chrono::NaiveDate;
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
fn theme_for(colorterm: Option<&str>) -> super::theme::Theme {
    with_vars(
        [("NO_COLOR", None::<&str>), ("COLORTERM", colorterm)],
        || super::theme::Theme::detect("dark"),
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
                "OVERDUE", "SCHEDULE", "SCHEDULE", "DUE SOON", "DOING", "INBOX"
            ]
        );
        let screen = render(&mut app, 120, 30).join("\n");
        assert!(screen.contains("OVERDUE"), "{screen}");
        assert!(screen.contains("12x"), "{screen}");
        assert!(screen.contains("in 3d"), "{screen}");
        assert!(screen.contains("Compare jiff and chrono"), "{screen}");
    });
}

#[test]
#[serial]
fn grouped_view_shows_each_block_its_own_groups() {
    with_home(|root| {
        fs::write(root.join("tasks.org"), TASKS).unwrap();
        let mut app = test_app(root);
        app.settings.views = toml::from_str::<ViewsOnly>(
            r#"[[view]]
key = "g"
desc = "Grouped"
blocks = [
  { kind = "alltodo", header = "First", groups = [{ name = "Next", match = "/NEXT" }] },
  { kind = "alltodo", header = "Second", groups = [{ name = "Waiting", match = "/WAIT" }] },
]"#,
        )
        .unwrap()
        .view;
        app.switch_screen(Screen::View("g".to_string())).unwrap();
        let screen = render(&mut app, 100, 30).join("\n");
        // Each block's "Other items" holds its own rows, drawn once.
        assert_eq!(screen.matches("OTHER ITEMS").count(), 2, "{screen}");
        assert_eq!(screen.matches("Migrate cron jobs").count(), 2, "{screen}");
        assert_eq!(screen.matches("Vendor reply").count(), 2, "{screen}");
        assert!(screen.contains("WAITING  1"), "{screen}");
    });
}

#[test]
#[serial]
fn peek_on_the_agents_tab_shows_the_task_an_agent_wrote() {
    with_home(|root| {
        fs::write(root.join("inbox.org"), "* Inbox\n").unwrap();
        let mut app = test_app(root);
        app.settings.captures[0].file = Some(root.join("inbox.org").to_string_lossy().into_owned());
        let draft = agd_core::parse_capture_line("Agent task #A", today()).unwrap();
        let context = agd_core::WriteContext {
            actor: "mcp:test-agent".to_string(),
            project: root.to_string_lossy().into_owned(),
            now: None,
        };
        agd_core::WriteEngine::with_default_journal(app.settings.clone())
            .unwrap()
            .capture("t", &draft, "", &Default::default(), false, &context)
            .unwrap();
        app.index.refresh_path(&root.join("inbox.org")).unwrap();
        app.switch_screen(Screen::Review).unwrap();
        app.peek = true;
        let screen = render(&mut app, 120, 30).join("\n");
        assert!(screen.contains("Peek"), "{screen}");
        assert!(screen.contains("p peek"), "{screen}");
        // The peek panel names the task and where it lives.
        assert!(screen.matches("Agent task").count() >= 2, "{screen}");
        assert!(screen.contains("inbox.org"), "{screen}");
    });
}

#[derive(serde::Deserialize)]
struct ViewsOnly {
    view: Vec<agd_core::ViewSettings>,
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
        assert!(screen.contains("1 overdue"), "{screen}");
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
        for key in "tRenew passport due:fri on:2027-01-04 +personal #A".chars() {
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
        // Chips name the month, and the year when it is not this one.
        assert!(screen.contains("deadline Fri 16 Oct"), "{screen}");
        assert!(screen.contains("scheduled Mon 4 Jan 2027"), "{screen}");
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

fn key(app: &mut App, code: crossterm::event::KeyCode) {
    assert!(!super::edit::handle_key(app, crossterm::event::KeyEvent::from(code)).unwrap());
}

#[test]
#[serial]
fn enter_edits_title_and_state_in_place_and_keeps_untouched_dates() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        let path = root.join("work.org");
        fs::write(
            &path,
            "* TODO Draft report\nSCHEDULED: <2026-10-13 Tue +1w>\n",
        )
        .unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        app.overlay = Overlay::Edit(Box::new(super::edit::EditForm::open(&app).unwrap()));
        for _ in "Draft report".chars() {
            key(&mut app, KeyCode::Backspace);
        }
        for character in "Final report".chars() {
            key(&mut app, KeyCode::Char(character));
        }
        key(&mut app, KeyCode::Tab);
        key(&mut app, KeyCode::Right);

        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains("-* TODO Draft report"), "{screen}");
        assert!(screen.contains("+* NEXT Final report"), "{screen}");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "* TODO Draft report\nSCHEDULED: <2026-10-13 Tue +1w>\n",
            "the preview must not write"
        );

        key(&mut app, KeyCode::Enter);
        assert!(matches!(app.overlay, Overlay::None));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "* NEXT Final report\nSCHEDULED: <2026-10-13 Tue +1w>\n"
        );
    });
}

#[test]
#[serial]
fn an_invalid_date_keeps_the_popup_open_with_the_reason() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        let path = root.join("work.org");
        let original = "* TODO Draft report\nSCHEDULED: <2026-10-13 Tue>\n";
        fs::write(&path, original).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        app.overlay = Overlay::Edit(Box::new(super::edit::EditForm::open(&app).unwrap()));
        for _ in 0..4 {
            key(&mut app, KeyCode::Tab);
        }
        for _ in "2026-10-13".chars() {
            key(&mut app, KeyCode::Backspace);
        }
        for character in "someday".chars() {
            key(&mut app, KeyCode::Char(character));
        }
        key(&mut app, KeyCode::Enter);
        assert!(matches!(app.overlay, Overlay::Edit(_)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    });
}

#[test]
#[serial]
fn ctrl_e_in_the_edit_popup_hands_over_to_the_editor_without_writing() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    with_home(|root| {
        let path = root.join("work.org");
        let original = "* TODO Draft report\nSCHEDULED: <2026-10-13 Tue>\n";
        fs::write(&path, original).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        app.overlay = Overlay::Edit(Box::new(super::edit::EditForm::open(&app).unwrap()));
        key(&mut app, KeyCode::Char('x'));
        let open_editor = super::edit::handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
        )
        .unwrap();
        assert!(open_editor);
        assert!(matches!(app.overlay, Overlay::None));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    });
}

#[test]
#[serial]
fn nerd_font_rounds_pills_and_the_default_does_not() {
    with_home(|root| {
        fs::write(
            root.join("work.org"),
            "* TODO Draft\nSCHEDULED: <2026-10-13 Tue>\n",
        )
        .unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        // with_home sets NO_COLOR, which never rounds; build colour themes.
        let colour = || {
            temp_env::with_var("NO_COLOR", None::<&str>, || {
                super::theme::Theme::detect("dark")
            })
        };
        app.theme = colour();
        let square = render(&mut app, 100, 10).join("\n");
        assert!(!square.contains('\u{e0b6}'), "{square}");

        app.theme = colour().rounded_pills(true);
        let round = render(&mut app, 100, 10).join("\n");
        assert!(
            round.contains('\u{e0b6}') && round.contains('\u{e0b4}'),
            "{round}"
        );
    });
}

#[test]
#[serial]
fn a_full_width_lead_keeps_a_space_before_the_keyword() {
    with_home(|root| {
        // Today is 2026-10-13, so this deadline is "in 14d", six columns.
        fs::write(
            root.join("work.org"),
            "* TODO File taxes\nDEADLINE: <2026-10-27 Tue>\n",
        )
        .unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        let screen = render(&mut app, 100, 12).join("\n");
        assert!(screen.contains("in 14d TODO File taxes"), "{screen}");
    });
}

#[test]
#[serial]
fn capture_opens_on_a_list_of_templates_with_their_destinations() {
    with_home(|root| {
        let mut app = test_app(root);
        app.overlay = Overlay::Capture(capture::Draft::default());
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains("choose a template"), "{screen}");
        assert!(screen.contains("t  Todo"), "{screen}");
        assert!(screen.contains("→ inbox.org › Inbox"), "{screen}");
        assert!(screen.contains("esc cancel"), "{screen}");
    });
}

#[test]
#[serial]
fn due_answers_a_deadline_prompt_so_it_is_not_asked() {
    with_home(|root| {
        fs::write(root.join("inbox.org"), "* Inbox\n").unwrap();
        let mut app = test_app(root);
        app.settings.captures[0].file = Some(root.join("inbox.org").to_string_lossy().into_owned());
        app.settings.captures[0].template = "* TODO %?\nDEADLINE: %^{deadline}t".to_string();
        app.overlay = Overlay::Capture(capture::Draft::default());
        for key in "tRenew passport".chars() {
            capture::type_key(&mut app, key);
        }
        let asking = render(&mut app, 100, 30).join("\n");
        assert!(asking.contains("or due: above"), "{asking}");

        for key in " due:fri".chars() {
            capture::type_key(&mut app, key);
        }
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(!screen.contains("or due: above"), "{screen}");
        assert!(screen.contains("DEADLINE: <2026-10-16 Fri>"), "{screen}");
    });
}

#[test]
#[serial]
fn sixteen_colours_keep_every_role_readable_on_the_cursor_row() {
    use ratatui::style::{Color, Modifier};
    with_home(|root| {
        fs::write(root.join("tasks.org"), TASKS).unwrap();
        let mut app = test_app(root);
        app.theme = theme_for(None);
        app.reload_rows().unwrap();
        assert_eq!(app.selected_row().unwrap().item.title, "Migrate cron jobs");
        let buffer = render_buffer(&mut app, 120, 30);
        let y = row_of(&buffer, "Migrate cron jobs");
        let mut red = false;
        for x in 0..120 {
            let cell = &buffer[(x, y)];
            let (fg, bg) = if cell.modifier.contains(Modifier::REVERSED) {
                (cell.bg, cell.fg)
            } else {
                (cell.fg, cell.bg)
            };
            red |= fg == Color::Red || bg == Color::Red;
            if cell.symbol() != " " && fg != Color::Reset {
                assert_ne!(fg, bg, "unreadable cell {:?} at {x}", cell.symbol());
            }
        }
        assert!(red, "the overdue badge keeps its red on the cursor row");
    });
}

#[test]
#[serial]
fn no_color_badges_stand_out_on_the_cursor_row() {
    use ratatui::style::Modifier;
    with_home(|root| {
        fs::write(root.join("tasks.org"), TASKS).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        let buffer = render_buffer(&mut app, 120, 30);
        let y = row_of(&buffer, "Migrate cron jobs");
        let badge = column_of(&buffer, y, "12x");
        let title = column_of(&buffer, y, "Migrate");
        let reversed = |x: u16| buffer[(x, y)].modifier.contains(Modifier::REVERSED);
        assert!(reversed(badge));
        assert!(!reversed(title));
    });
}

/// Presses one key as the event loop would; returns the $EDITOR request.
fn press(app: &mut App, code: crossterm::event::KeyCode) -> super::keys::EditorTarget {
    super::keys::dispatch(app, crossterm::event::KeyEvent::from(code)).unwrap()
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
    let draft = agd_core::parse_capture_line(title, today()).unwrap();
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

#[test]
#[serial]
fn shift_a_on_the_agents_tab_accepts_every_change() {
    with_home(|root| {
        fs::write(root.join("inbox.org"), "* Inbox\n").unwrap();
        let mut app = test_app(root);
        agent_capture(&app, root, "First agent task");
        agent_capture(&app, root, "Second agent task");
        app.index.refresh_path(&root.join("inbox.org")).unwrap();
        press(&mut app, crossterm::event::KeyCode::Char('A'));
        assert_eq!(app.screen, Screen::Review);
        assert_eq!(app.journals.len(), 2);
        press(&mut app, crossterm::event::KeyCode::Char('A'));
        assert_eq!(app.screen, Screen::Review);
        assert!(
            app.journals
                .iter()
                .all(|record| record.status == agd_core::JournalStatus::Reviewed)
        );
        assert_eq!(app.counts.agents, 0);
    });
}

const THREE_TODAY: &str = "* TODO One\nSCHEDULED: <2026-10-13 Tue>\n\
* TODO Two\nSCHEDULED: <2026-10-13 Tue>\n\
* TODO Three\nSCHEDULED: <2026-10-13 Tue>\n";

#[test]
#[serial]
fn change_keys_act_on_every_marked_row_and_moves_never_write() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        let path = root.join("work.org");
        fs::write(&path, THREE_TODAY).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        type_keys(&mut app, "m ");
        assert_eq!(app.marks.len(), 2);
        assert_eq!(app.selected_row().unwrap().item.title, "Three");

        app.day = today() + chrono::Duration::days(2);
        press(&mut app, KeyCode::Char('.'));
        assert_eq!(app.day, today());
        assert_eq!(fs::read_to_string(&path).unwrap(), THREE_TODAY);
        assert_eq!(app.marks.len(), 2);

        press(&mut app, KeyCode::Char('s'));
        assert!(matches!(app.overlay, Overlay::Prompt { .. }));
        type_keys(&mut app, "fri");
        press(&mut app, KeyCode::Enter);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "* TODO One\nSCHEDULED: <2026-10-16 Fri>\n\
* TODO Two\nSCHEDULED: <2026-10-16 Fri>\n\
* TODO Three\nSCHEDULED: <2026-10-13 Tue>\n"
        );
        assert!(app.marks.is_empty());
    });
}

#[test]
#[serial]
fn list_keys_follow_org_agenda_for_moving_refreshing_and_refiling() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        let path = root.join("work.org");
        fs::write(&path, THREE_TODAY).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        press(&mut app, KeyCode::Char('n'));
        assert_eq!(app.selected_row().unwrap().item.title, "Two");
        press(&mut app, KeyCode::Char('k'));
        assert_eq!(app.selected_row().unwrap().item.title, "One");
        press(&mut app, KeyCode::Char('r'));
        assert_eq!(app.status.as_ref().unwrap().text, "refreshed");
        assert!(matches!(app.overlay, Overlay::None));
        press(&mut app, KeyCode::Char('R'));
        assert!(matches!(app.overlay, Overlay::Refile(_)));
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char('S'));
        let written = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = written.lines().collect();
        assert!(lines[0].starts_with("* TODO One "), "{written}");
        assert!(lines[0].ends_with(":someday:"), "{written}");
        assert_eq!(lines[1], "* TODO Two", "{written}");
    });
}

const TWO_OVERDUE: &str = "#+TODO: TODO | DONE CANCELLED\n\
* TODO Old one\nSCHEDULED: <2026-10-01 Thu>\n\
* TODO Old two\nSCHEDULED: <2026-10-05 Mon>\n";

#[test]
#[serial]
fn triage_k_moves_back_a_card_and_shift_x_cancels() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        let path = root.join("work.org");
        fs::write(&path, TWO_OVERDUE).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        press(&mut app, KeyCode::Char('T'));
        assert_eq!(app.screen, Screen::Triage);
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains("k previous card"), "{screen}");
        assert!(screen.contains("X  cancel"), "{screen}");
        assert!(screen.contains("R  refile"), "{screen}");

        let first = app.selected_row().unwrap().item.title.clone();
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('k'));
        assert_eq!(app.selected_row().unwrap().item.title, first);
        assert_eq!(fs::read_to_string(&path).unwrap(), TWO_OVERDUE);

        press(&mut app, KeyCode::Char('X'));
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains(&format!("* CANCELLED {first}")),
            "{}",
            fs::read_to_string(&path).unwrap()
        );
    });
}

fn press_control(app: &mut App, character: char) {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let key = KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL);
    super::keys::dispatch(app, key).unwrap();
}

#[test]
#[serial]
fn ctrl_c_ctrl_w_refiles_and_ctrl_c_twice_quits() {
    with_home(|root| {
        fs::write(root.join("work.org"), THREE_TODAY).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        press_control(&mut app, 'c');
        assert!(!app.quit);
        press_control(&mut app, 'w');
        assert!(matches!(app.overlay, Overlay::Refile(_)));
        assert!(!app.quit);

        app.overlay = Overlay::None;
        press_control(&mut app, 'c');
        press_control(&mut app, 'c');
        assert!(app.quit);

        // The pane has no chords: C-c quits at once.
        let mut pane = test_app(root);
        pane.pane = true;
        press_control(&mut pane, 'c');
        assert!(pane.quit);
    });
}

/// Every line help shows at this size, scrolling to the end with j.
fn scrolled_help(app: &mut App, width: u16, height: u16) -> Vec<String> {
    use crossterm::event::KeyCode;
    press(app, KeyCode::Char('?'));
    let mut seen = Vec::new();
    for _ in 0..80 {
        for line in render(app, width, height) {
            if !seen.contains(&line) {
                seen.push(line);
            }
        }
        press(app, KeyCode::Char('j'));
    }
    seen
}

#[test]
#[serial]
fn help_scrolls_through_every_screens_keys_at_60_by_24() {
    with_home(|root| {
        fs::write(root.join("tasks.org"), TASKS).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        let lines = scrolled_help(&mut app, 60, 24);
        let text = lines.join("\n");
        for title in ["TODAY · WEEK · INBOX · VIEWS", "TRIAGE", "AGENTS", "PANE"] {
            assert!(text.contains(title), "{title} missing:\n{text}");
        }
        for (_, groups) in super::keys::SCREENS {
            for group in groups {
                for binding in group.bindings {
                    assert!(
                        text.contains(&format!(" {} ", binding.label)),
                        "{} missing:\n{text}",
                        binding.label
                    );
                }
            }
        }
        // Too narrow for the org-agenda column: long help wraps instead of
        // being cut.
        assert!(!text.contains("C-c C-s"), "{text}");
        assert!(text.contains("filter, leave "), "{text}");
        assert!(text.contains("            a view "), "{text}");
        press(&mut app, crossterm::event::KeyCode::Esc);
        assert!(matches!(app.overlay, Overlay::None));
    });
}

#[test]
#[serial]
fn help_shows_the_org_agenda_column_only_where_it_fits() {
    with_home(|root| {
        fs::write(root.join("tasks.org"), TASKS).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        let text = scrolled_help(&mut app, 160, 40).join("\n");
        assert!(text.contains("org-agenda"), "{text}");
        let schedule = text
            .lines()
            .find(|line| line.contains(" s ") && line.contains("C-c C-s"))
            .unwrap_or_else(|| panic!("{text}"));
        // The org-agenda key sits apart from the help text.
        assert!(schedule.contains("schedule   "), "{schedule}");
    });
}

#[test]
#[serial]
fn help_starts_with_the_screen_on_view() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        fs::write(root.join("work.org"), TWO_OVERDUE).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        press(&mut app, KeyCode::Char('T'));
        press(&mut app, KeyCode::Char('?'));
        let screen = render(&mut app, 100, 30).join("\n");
        let triage = screen.find("TRIAGE\n").or_else(|| screen.find("TRIAGE "));
        let lists = screen.find("TODAY · WEEK");
        assert!(triage.is_some(), "{screen}");
        assert!(
            lists.is_none_or(|lists| triage.unwrap() < lists),
            "{screen}"
        );
        assert!(screen.contains("previous card"), "{screen}");
    });
}

#[test]
#[serial]
fn the_footer_keeps_help_visible_at_80_columns_with_whole_hints() {
    with_home(|root| {
        fs::write(root.join("tasks.org"), TASKS).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        let screen = render(&mut app, 80, 24);
        let footer = screen.last().unwrap();
        assert!(footer.ends_with("? help "), "{footer}");
        assert!(footer.contains("x done"), "{footer}");
        // A hint that does not fit is left out, not cut.
        for word in ["schedule", "deadline", "triage"] {
            let start = &word[..3];
            assert!(!footer.contains(start) || footer.contains(word), "{footer}");
        }
        app.set_status("work.org:2  TODO → DONE · u undo");
        let footer = render(&mut app, 80, 24).last().unwrap().clone();
        assert!(footer.contains("TODO → DONE"), "{footer}");
        assert!(footer.ends_with("? help "), "{footer}");
    });
}

#[test]
#[serial]
fn a_narrow_pane_says_to_widen_instead_of_opening_the_full_view() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        fs::write(root.join("tasks.org"), TASKS).unwrap();
        let mut app = test_app(root);
        app.pane = true;
        app.pane_requested = true;
        app.reload_rows().unwrap();
        let screen = render(&mut app, 34, 14).join("\n");
        assert!(screen.contains("1 overdue"), "{screen}");
        assert!(!screen.contains("triage"), "{screen}");

        for code in [KeyCode::Enter, KeyCode::Char('T')] {
            press(&mut app, code);
            assert!(app.pane);
            assert_eq!(app.screen, Screen::Today);
        }
        let screen = render(&mut app, 34, 14).join("\n");
        assert!(screen.contains("widen to 60+ columns"), "{screen}");
        assert!(screen.contains("full view"), "{screen}");

        // Wide enough: Enter opens the full view, and a narrow terminal
        // brings the pane back from then on.
        render(&mut app, 100, 30);
        press(&mut app, KeyCode::Enter);
        assert!(!app.pane);
        assert!(!app.pane_requested);
    });
}

#[test]
#[serial]
fn an_empty_no_color_keeps_colour() {
    let plain = |value: Option<&str>| {
        temp_env::with_var("NO_COLOR", value, || {
            super::theme::Theme::detect("dark").plain
        })
    };
    assert!(plain(Some("1")));
    assert!(!plain(Some("")));
    assert!(!plain(None));
}

#[test]
#[serial]
fn shift_x_cancels_with_the_files_own_keyword() {
    with_home(|root| {
        let path = root.join("work.org");
        fs::write(
            &path,
            "#+TODO: TODO | DONE CANCELED\n* TODO Old plan\nSCHEDULED: <2026-10-13 Tue>\n",
        )
        .unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        app.cancel().unwrap();
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains("* CANCELED Old plan"),
            "{}",
            fs::read_to_string(&path).unwrap()
        );
    });
}
