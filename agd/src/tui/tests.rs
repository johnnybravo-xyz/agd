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
        || super::theme::Theme::detect("dark", None),
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
        assert!(screen.contains("12d"), "{screen}");
        assert!(!screen.contains("12x"), "{screen}");
        // The classic layout keeps Org's own leader.
        app.classic = true;
        app.reload_rows().unwrap();
        let classic = render(&mut app, 120, 30).join("\n");
        assert!(classic.contains("Sched.12x:"), "{classic}");
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
        assert!(screen.contains("a accept"), "{screen}");
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
        assert!(screen.contains("Inbox 1"), "{screen}");
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
        let status = &app.status.as_ref().unwrap().text;
        assert!(status.contains("changed on disk"), "{status}");
        assert!(!status.contains("stale_ref"), "{status}");
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
        // Tags align to tags_column, as an edit writes them.
        assert!(screen.contains("TODO [#A] Renew passport"), "{screen}");
        assert!(screen.contains(":personal:"), "{screen}");
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
fn ctrl_o_in_the_edit_popup_hands_over_to_the_editor_without_writing() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    with_home(|root| {
        let path = root.join("work.org");
        let original = "* TODO Draft report\nSCHEDULED: <2026-10-13 Tue>\n";
        fs::write(&path, original).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        app.overlay = Overlay::Edit(Box::new(super::edit::EditForm::open(&app).unwrap()));
        key(&mut app, KeyCode::Char('x'));
        // In a field ctrl-e is end of line, as in the minibuffer.
        let control = |character| KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL);
        assert!(!super::edit::handle_key(&mut app, control('e')).unwrap());
        assert!(matches!(app.overlay, Overlay::Edit(_)));
        let open_editor = super::edit::handle_key(&mut app, control('o')).unwrap();
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
                super::theme::Theme::detect("dark", None)
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
        let badge = column_of(&buffer, y, "12d");
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
        type_keys(&mut app, "mm");
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
        press(&mut app, KeyCode::Char('p'));
        assert_eq!(app.selected_row().unwrap().item.title, "One");
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('k'));
        assert_eq!(app.selected_row().unwrap().item.title, "One");
        press(&mut app, KeyCode::Char(' '));
        assert!(app.peek);
        press(&mut app, KeyCode::Char(' '));
        assert!(!app.peek);
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
        assert!(screen.contains("p previous card"), "{screen}");
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

/// Every line help shows at this size, choosing down to its last action
/// (letters type into its filter, so ↓ moves).
fn scrolled_help(app: &mut App, width: u16, height: u16) -> Vec<String> {
    use crossterm::event::KeyCode;
    press(app, KeyCode::Char('?'));
    let mut seen = Vec::new();
    for _ in 0..160 {
        for line in render(app, width, height) {
            // The bar marks the chosen row; compare the text without it.
            let line = line.replace('▌', " ");
            if !seen.contains(&line) {
                seen.push(line);
            }
        }
        press(app, KeyCode::Down);
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
        assert!(text.contains("clear marks or filter,"), "{text}");
        let wrapped = text
            .lines()
            .find(|line| line.contains("leave a view"))
            .unwrap_or_else(|| panic!("{text}"));
        assert!(!wrapped.contains("esc"), "{wrapped}");
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
        assert!(footer.ends_with("? keys "), "{footer}");
        assert!(footer.contains("x done"), "{footer}");
        // A hint that does not fit is left out, not cut.
        for word in ["schedule", "deadline", "triage"] {
            let start = &word[..3];
            assert!(!footer.contains(start) || footer.contains(word), "{footer}");
        }
        app.set_status("work.org:2  TODO → DONE · u undo");
        let footer = render(&mut app, 80, 24).last().unwrap().clone();
        assert!(footer.contains("TODO → DONE"), "{footer}");
        assert!(footer.ends_with("? keys "), "{footer}");
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
fn the_schedule_prompt_previews_the_exact_change_before_writing() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        let path = root.join("work.org");
        fs::write(&path, THREE_TODAY).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        press(&mut app, KeyCode::Char('s'));
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains("Schedule · work.org:1"), "{screen}");
        // Examples come from today (13 Oct), two weeks out.
        assert!(screen.contains("2026-10-27 14:00"), "{screen}");
        assert!(!screen.contains("2026-10-30"), "{screen}");

        type_keys(&mut app, "fri");
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains("-SCHEDULED: <2026-10-13 Tue>"), "{screen}");
        assert!(screen.contains("+SCHEDULED: <2026-10-16 Fri>"), "{screen}");
        assert_eq!(fs::read_to_string(&path).unwrap(), THREE_TODAY);

        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.overlay, Overlay::None));
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .starts_with("* TODO One\nSCHEDULED: <2026-10-16 Fri>\n")
        );
        let status = &app.status.as_ref().unwrap().text;
        assert!(status.contains("scheduled Fri 16 Oct"), "{status}");
    });
}

#[test]
#[serial]
fn a_prompt_that_would_change_nothing_says_so_and_stays_open() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        let path = root.join("work.org");
        fs::write(&path, THREE_TODAY).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        press(&mut app, KeyCode::Char('s'));
        type_keys(&mut app, "today");
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(
            screen.contains("already scheduled for Tue 13 Oct"),
            "{screen}"
        );
        assert!(!screen.contains("no_change"), "{screen}");
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.overlay, Overlay::Prompt(_)));
        assert_eq!(fs::read_to_string(&path).unwrap(), THREE_TODAY);
    });
}

#[test]
#[serial]
fn the_priority_prompt_refuses_values_outside_the_range() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        let path = root.join("work.org");
        fs::write(&path, THREE_TODAY).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        press(&mut app, KeyCode::Char(','));
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains("A to C; empty clears"), "{screen}");
        type_keys(&mut app, "z");
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains("priority is one of A to C"), "{screen}");
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.overlay, Overlay::Prompt(_)));
        assert_eq!(fs::read_to_string(&path).unwrap(), THREE_TODAY);

        press(&mut app, KeyCode::Backspace);
        type_keys(&mut app, "b");
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains("+* TODO [#B] One"), "{screen}");
        press(&mut app, KeyCode::Enter);
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .starts_with("* TODO [#B] One\n")
        );
    });
}

#[test]
#[serial]
fn cancel_uses_the_last_done_keyword_of_the_files_workflow() {
    with_home(|root| {
        let path = root.join("work.org");
        fs::write(
            &path,
            "#+TODO: TODO | FINISHED DROPPED\n* TODO Old plan\nSCHEDULED: <2026-10-13 Tue>\n",
        )
        .unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        app.cancel().unwrap();
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains("* DROPPED Old plan"), "{written}");

        let single = "#+TODO: TODO | DONE\n* TODO Plan\nSCHEDULED: <2026-10-13 Tue>\n";
        fs::write(&path, single).unwrap();
        app.index.refresh_path(&path).unwrap();
        app.reload_rows().unwrap();
        let error = app.cancel().unwrap_err().to_string();
        assert!(error.contains("no cancel state, only DONE"), "{error}");
        assert_eq!(fs::read_to_string(&path).unwrap(), single);
    });
}

const FIVE_OVERDUE: &str = "* TODO Sched old\nSCHEDULED: <2026-10-01 Thu>\n\
* TODO Sched older\nSCHEDULED: <2026-09-20 Sun>\n\
* TODO Sched recent\nSCHEDULED: <2026-10-10 Sat>\n\
* TODO Missed filing\nDEADLINE: <2026-10-12 Mon>\n\
* TODO Missed renewal\nDEADLINE: <2026-10-08 Thu>\n";

#[test]
#[serial]
fn today_folds_overdue_behind_a_more_row_with_missed_deadlines_first() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        let path = root.join("work.org");
        fs::write(&path, FIVE_OVERDUE).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        let titles: Vec<&str> = app.rows.iter().map(|row| row.item.title.as_str()).collect();
        assert_eq!(
            titles[..3],
            ["Missed renewal", "Missed filing", "Sched older"]
        );
        assert_eq!(app.rows[3].more, 2);
        let screen = render(&mut app, 100, 20).join("\n");
        assert!(screen.contains("OVERDUE  5"), "{screen}");
        assert!(screen.contains("+2 more"), "{screen}");
        assert!(!screen.contains("Sched recent"), "{screen}");

        // On the "+N more" row, change keys have no task to act on.
        app.selected = 3;
        let _refused = super::keys::dispatch(
            &mut app,
            crossterm::event::KeyEvent::from(KeyCode::Char('x')),
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), FIVE_OVERDUE);

        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.overlay, Overlay::None));
        assert_eq!(app.rows.len(), 5);
        assert!(app.rows.iter().all(|row| row.more == 0));
        let screen = render(&mut app, 100, 20).join("\n");
        assert!(screen.contains("OVERDUE  5"), "{screen}");
        assert!(screen.contains("Sched recent"), "{screen}");
    });
}

#[test]
#[serial]
fn status_lines_name_what_happened_and_never_blame_the_config() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        fs::write(root.join("work.org"), "* Notes\n").unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        press(&mut app, KeyCode::Char('x'));
        assert_eq!(app.status.as_ref().unwrap().text, "nothing to mark done");
        press(&mut app, KeyCode::Char('X'));
        assert_eq!(app.status.as_ref().unwrap().text, "nothing to cancel");

        // An empty Triage says the inbox is clear and offers no undo.
        press(&mut app, KeyCode::Char('T'));
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains("Nothing to triage"), "{screen}");
        assert!(screen.contains("inbox empty"), "{screen}");
        assert!(!screen.contains("undoes"), "{screen}");
    });
}

#[test]
#[serial]
fn a_state_change_on_one_marked_row_names_that_rows_state() {
    with_home(|root| {
        fs::write(
            root.join("work.org"),
            "* NEXT One\nSCHEDULED: <2026-10-13 Tue>\n* TODO Two\nSCHEDULED: <2026-10-13 Tue>\n",
        )
        .unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        type_keys(&mut app, "m");
        assert_eq!(app.selected_row().unwrap().item.title, "Two");
        app.set_state("DONE").unwrap();
        let status = &app.status.as_ref().unwrap().text;
        assert!(status.contains("NEXT → DONE"), "{status}");
    });
}

#[test]
#[serial]
fn shift_b_opens_a_bulk_menu_for_the_marked_rows_and_shift_u_unmarks_all() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        let path = root.join("work.org");
        fs::write(&path, THREE_TODAY).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        press(&mut app, KeyCode::Char('B'));
        assert!(matches!(app.overlay, Overlay::None));
        assert!(
            app.status
                .as_ref()
                .unwrap()
                .text
                .contains("mark rows with m")
        );

        type_keys(&mut app, "mm");
        press(&mut app, KeyCode::Char('U'));
        assert!(app.marks.is_empty());

        app.selected = 0;
        type_keys(&mut app, "mmB");
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains("Bulk · 2 marked rows"), "{screen}");
        assert!(screen.contains("schedule tomorrow"), "{screen}");
        press(&mut app, KeyCode::Char('m'));
        assert!(matches!(app.overlay, Overlay::None));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "* TODO One\nSCHEDULED: <2026-10-14 Wed>\n\
* TODO Two\nSCHEDULED: <2026-10-14 Wed>\n\
* TODO Three\nSCHEDULED: <2026-10-13 Tue>\n"
        );
    });
}

#[test]
#[serial]
fn agent_cards_name_the_changed_headings_line_and_enter_opens_it() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        let path = root.join("work.org");
        let filler: String = (1..20).map(|line| format!("line {line}\n")).collect();
        fs::write(&path, format!("{filler}* TODO Pay rent\n")).unwrap();
        let mut app = test_app(root);
        let item = app.index.items().next().unwrap().clone();
        assert_eq!(item.line, 20);
        let intent = agd_core::EditIntent {
            state: agd_core::Change::Set("DONE".to_string()),
            ..Default::default()
        };
        let context = agd_core::WriteContext {
            actor: "mcp:test-agent".to_string(),
            project: root.to_string_lossy().into_owned(),
            now: None,
        };
        agd_core::WriteEngine::with_default_journal(app.settings.clone())
            .unwrap()
            .edit(&item.item_ref, &intent, false, &context)
            .unwrap();
        app.index.refresh_path(&path).unwrap();
        app.switch_screen(Screen::Review).unwrap();
        let screen = render(&mut app, 120, 30).join("\n");
        assert!(screen.contains("work.org:20"), "{screen}");
        let (file, line) = press(&mut app, KeyCode::Enter).unwrap();
        assert!(file.ends_with("work.org"), "{file}");
        assert_eq!(line, 20);
    });
}

#[test]
#[serial]
fn the_list_title_follows_f_and_b_and_week_opens_on_today() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        fs::write(
            root.join("work.org"),
            "* TODO Monday task\nSCHEDULED: <2026-10-12 Mon>\n\
* TODO Tuesday task\nSCHEDULED: <2026-10-13 Tue>\n\
* TODO Thursday task\nSCHEDULED: <2026-10-15 Thu>\n",
        )
        .unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        assert!(render(&mut app, 100, 20)[1].contains(" Today "));
        press(&mut app, KeyCode::Char('f'));
        let title = render(&mut app, 100, 20)[1].clone();
        assert!(title.contains("Tomorrow · Wed 14 Oct"), "{title}");
        press(&mut app, KeyCode::Char('f'));
        let title = render(&mut app, 100, 20)[1].clone();
        assert!(title.contains("Thu 15 Oct"), "{title}");
        press(&mut app, KeyCode::Char('.'));

        press(&mut app, KeyCode::Tab);
        assert_eq!(app.screen, Screen::Week);
        let row = app.selected_row().unwrap();
        assert_eq!(row.occurrence.as_ref().unwrap().date, "2026-10-13");
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(
            screen.contains("Monday task"),
            "the week still starts Monday"
        );
    });
}

#[test]
#[serial]
fn a_narrow_header_keeps_the_open_views_tab_and_drops_pills_first() {
    with_home(|root| {
        fs::write(root.join("tasks.org"), TASKS).unwrap();
        let mut app = test_app(root);
        app.settings.views = toml::from_str::<ViewsOnly>(
            r#"[[view]]
key = "d"
desc = "Dashboard"
blocks = [{ kind = "alltodo" }]

[[view]]
key = "w"
desc = "Weekly review"
blocks = [{ kind = "alltodo" }]"#,
        )
        .unwrap()
        .view;
        app.settings.start_view = "d".to_string();
        app.reload_rows().unwrap();
        // No context is set, so no context pill.
        let wide = render(&mut app, 120, 20)[0].clone();
        assert!(!wide.contains("ctx"), "{wide}");
        assert!(wide.contains("Tue 13 Oct"), "{wide}");

        app.switch_screen(Screen::View("w".to_string())).unwrap();
        app.context = Some("tasks".to_string());
        let narrow = render(&mut app, 60, 20)[0].clone();
        assert!(narrow.contains("w Weekly review"), "{narrow}");
        assert!(narrow.contains("Agenda"), "{narrow}");
        assert!(!narrow.contains("Tue 13 Oct"), "{narrow}");
    });
}

#[test]
#[serial]
fn peek_keeps_its_keys_under_wrapped_text_and_shows_link_text() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        let long = "word ".repeat(60);
        fs::write(
            root.join("work.org"),
            format!(
                "* TODO Read [[https://example.com/spec][the spec]]\n\
SCHEDULED: <2026-10-13 Tue>\n{long}\n{long}\n{long}\n"
            ),
        )
        .unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        press(&mut app, KeyCode::Char(' '));
        let screen = render(&mut app, 80, 24).join("\n");
        assert!(screen.contains("Read the spec"), "{screen}");
        assert!(!screen.contains("[[https"), "{screen}");
        assert!(screen.contains("⏎ edit"), "{screen}");
        assert!(screen.contains("y copy link"), "{screen}");
    });
}

#[test]
#[serial]
fn long_titles_end_in_an_ellipsis_and_keep_their_tags() {
    with_home(|root| {
        let title = "Prepare the quarterly infrastructure capacity review with every team lead \
                     and the finance partners before the planning offsite";
        fs::write(
            root.join("work.org"),
            format!("* TODO {title} :ops:\nSCHEDULED: <2026-10-13 Tue>\n"),
        )
        .unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        let screen = render(&mut app, 100, 12);
        let row = screen
            .iter()
            .find(|line| line.contains("Prepare the"))
            .unwrap_or_else(|| panic!("{screen:?}"));
        assert!(row.contains('…'), "{row}");
        assert!(row.contains("#ops"), "{row}");
        assert!(!row.contains(title), "{row}");
    });
}

#[test]
#[serial]
fn the_pane_truncates_wide_characters_by_display_width() {
    with_home(|root| {
        fs::write(
            root.join("work.org"),
            "* TODO 四半期のインフラ容量レビューを準備する 🚀🚀\nSCHEDULED: <2026-10-13 Tue 09:00>\n",
        )
        .unwrap();
        let mut app = test_app(root);
        app.pane = true;
        app.reload_rows().unwrap();
        let buffer = render_buffer(&mut app, 34, 10);
        let y = (0..10)
            .find(|y| buffer[(10, *y)].symbol() == "四" || buffer[(11, *y)].symbol() == "四")
            .expect("the title row");
        let row: String = (0..34).map(|x| buffer[(x, y)].symbol()).collect();
        assert!(row.contains('…'), "{row}");
        // The border stays in the last column: nothing spilled past it.
        assert_eq!(buffer[(33, y)].symbol(), "│", "{row}");
    });
}

#[test]
#[serial]
fn the_views_popup_takes_j_and_k_as_view_keys_and_q_closes_it() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        fs::write(root.join("tasks.org"), TASKS).unwrap();
        let mut app = test_app(root);
        app.settings.views = toml::from_str::<ViewsOnly>(
            r#"[[view]]
key = "k"
desc = "Keep going"
blocks = [{ kind = "alltodo" }]

[[view]]
key = "j"
desc = "Journal"
blocks = [{ kind = "alltodo" }]"#,
        )
        .unwrap()
        .view;
        app.reload_rows().unwrap();
        press(&mut app, KeyCode::Char('a'));
        let screen = render(&mut app, 100, 30).join("\n");
        assert_eq!(screen.matches("open").count(), 1, "{screen}");
        press(&mut app, KeyCode::Char('q'));
        assert!(matches!(app.overlay, Overlay::None));
        assert_eq!(app.screen, Screen::Today);

        press(&mut app, KeyCode::Char('a'));
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.screen, Screen::View("j".to_string()));
    });
}

#[test]
#[serial]
fn empty_lists_say_why_they_are_empty() {
    with_home(|root| {
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        let screen = render(&mut app, 100, 20).join("\n");
        assert!(screen.contains("No .org files under"), "{screen}");
        assert!(
            screen.contains("press c to capture your first task"),
            "{screen}"
        );

        // Nothing today; tomorrow one scheduled task and one deadline
        // without a warning period.
        fs::write(
            root.join("work.org"),
            "* TODO Later\nSCHEDULED: <2026-10-14 Wed>\n* TODO Filing\nDEADLINE: <2026-10-14 Wed -0d>\n",
        )
        .unwrap();
        let mut app = test_app(root);
        app.filter = "zzz".to_string();
        app.reload_rows().unwrap();
        let screen = render(&mut app, 100, 20).join("\n");
        assert!(
            screen.contains("Nothing matches /zzz · esc clears"),
            "{screen}"
        );

        app.filter.clear();
        app.reload_rows().unwrap();
        let screen = render(&mut app, 100, 20).join("\n");
        assert!(screen.contains("All clear for today."), "{screen}");
        assert!(screen.contains("Tomorrow: 1 scheduled, 1 due"), "{screen}");

        app.switch_screen(Screen::Review).unwrap();
        let screen = render(&mut app, 100, 20).join("\n");
        assert!(screen.contains("No agent changes to review."), "{screen}");
    });
}

#[test]
#[serial]
fn who_captured_an_inbox_item_survives_lines_added_above_it() {
    with_home(|root| {
        let inbox = root.join("inbox.org");
        fs::write(&inbox, "* Notes\n* Inbox\n").unwrap();
        let mut app = test_app(root);
        agent_capture(&app, root, "Agent task");
        app.index = WorkspaceIndex::load(app.settings.clone()).unwrap();
        app.reload_rows().unwrap();
        let screen = render(&mut app, 120, 20).join("\n");
        assert!(screen.contains("test-agent"), "{screen}");

        // An earlier heading grows, as `x` adding CLOSED and LOGBOOK would.
        let grown = fs::read_to_string(&inbox).unwrap().replacen(
            "* Notes\n",
            "* Notes\nCLOSED: [2026-10-13 Tue 09:00]\nmore\n",
            1,
        );
        fs::write(&inbox, grown).unwrap();
        app.index = WorkspaceIndex::load(app.settings.clone()).unwrap();
        app.reload_rows().unwrap();
        let screen = render(&mut app, 120, 20).join("\n");
        assert!(screen.contains("Agent task"), "{screen}");
        assert!(screen.contains("test-agent"), "{screen}");
    });
}

#[test]
#[serial]
fn a_state_that_logs_a_note_asks_for_it_before_writing() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        let path = root.join("work.org");
        let original =
            "#+TODO: TODO WAIT(w@/!) | DONE\n* TODO Vendor contract\nSCHEDULED: <2026-10-13 Tue>\n";
        fs::write(&path, original).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        press(&mut app, KeyCode::Char('t'));
        assert!(matches!(app.overlay, Overlay::Prompt(_)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);

        type_keys(&mut app, "waiting on legal");
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains("Note for WAIT"), "{screen}");
        assert!(screen.contains("waiting on legal"), "{screen}");
        assert_eq!(fs::read_to_string(&path).unwrap(), original);

        press(&mut app, KeyCode::Enter);
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains("* WAIT Vendor contract"), "{written}");
        assert!(written.contains("- State \"WAIT\""), "{written}");
        assert!(written.contains("  waiting on legal\n"), "{written}");
    });
}

#[test]
#[serial]
fn typing_into_a_focused_edit_field_replaces_it_and_end_keeps_it() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        let path = root.join("work.org");
        let original = "* TODO Draft report\nSCHEDULED: <2026-10-13 Tue 10:00>\n";
        fs::write(&path, original).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        app.overlay = Overlay::Edit(Box::new(super::edit::EditForm::open(&app).unwrap()));
        for _ in 0..4 {
            key(&mut app, KeyCode::Tab);
        }
        for character in "fri".chars() {
            key(&mut app, KeyCode::Char(character));
        }
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains("+SCHEDULED: <2026-10-16 Fri>"), "{screen}");
        assert!(!screen.contains("10:00fri"), "{screen}");

        // Back on Title, End keeps the text and typing appends.
        for _ in 0..2 {
            key(&mut app, KeyCode::Tab);
        }
        key(&mut app, KeyCode::End);
        for character in " v2".chars() {
            key(&mut app, KeyCode::Char(character));
        }
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains("+* TODO Draft report v2"), "{screen}");
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    });
}

#[test]
#[serial]
fn prompts_and_the_refile_picker_take_minibuffer_keys() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    with_home(|root| {
        fs::write(root.join("work.org"), THREE_TODAY).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        let control = |app: &mut App, character| {
            let key = KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL);
            super::keys::dispatch(app, key).unwrap();
        };
        press(&mut app, KeyCode::Char('s'));
        type_keys(&mut app, "next fri");
        control(&mut app, 'w');
        control(&mut app, 'a');
        type_keys(&mut app, "+2d");
        control(&mut app, 'e');
        let Overlay::Prompt(prompt) = &app.overlay else {
            panic!("the prompt closed");
        };
        assert_eq!(prompt.input.text(), "+2dnext ");
        control(&mut app, 'u');
        control(&mut app, 'g');
        assert!(matches!(app.overlay, Overlay::None));

        press(&mut app, KeyCode::Char('R'));
        type_keys(&mut app, "two");
        control(&mut app, 'g');
        assert!(matches!(app.overlay, Overlay::None));
        assert_eq!(
            fs::read_to_string(root.join("work.org")).unwrap(),
            THREE_TODAY
        );
    });
}

#[test]
#[serial]
fn the_filter_narrows_rows_as_you_type_and_esc_restores_them() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        fs::write(root.join("work.org"), THREE_TODAY).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        press(&mut app, KeyCode::Char('/'));
        type_keys(&mut app, "tw");
        assert_eq!(app.rows.len(), 1);
        assert_eq!(app.rows[0].item.title, "Two");
        let screen = render(&mut app, 100, 20);
        assert!(screen.last().unwrap().starts_with("/tw"), "{screen:?}");
        assert!(screen.join("\n").contains("Two"), "{screen:?}");
        press(&mut app, KeyCode::Esc);
        assert!(app.filter.is_empty());
        assert_eq!(app.rows.len(), 3);

        press(&mut app, KeyCode::Char('/'));
        type_keys(&mut app, "one");
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.overlay, Overlay::None));
        assert_eq!(app.filter, "one");
        assert_eq!(app.rows.len(), 1);
    });
}

#[test]
#[serial]
fn org_agenda_block_titles_carry_no_count_but_groups_do() {
    with_home(|root| {
        fs::write(root.join("tasks.org"), TASKS).unwrap();
        let mut app = test_app(root);
        app.settings.views = toml::from_str::<ViewsOnly>(
            r#"[[view]]
key = "d"
desc = "Day"
blocks = [
  { kind = "agenda", span = "day" },
  { kind = "alltodo", header = "Waiting", groups = [{ name = "Vendors", match = "/WAIT" }] },
]"#,
        )
        .unwrap()
        .view;
        app.switch_screen(Screen::View("d".to_string())).unwrap();
        let screen = render(&mut app, 100, 40).join("\n");
        let title = screen
            .lines()
            .find(|line| line.contains("DAY-AGENDA"))
            .unwrap_or_else(|| panic!("{screen}"));
        let after = title.split("):").nth(1).unwrap_or_default();
        assert!(!after.chars().any(|c| c.is_ascii_digit()), "{title}");
        assert!(screen.contains("VENDORS  1"), "{screen}");
    });
}

#[test]
#[serial]
fn the_footer_offers_what_the_selected_row_needs() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        fs::write(root.join("tasks.org"), TASKS).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        let footer_for = |app: &mut App, title: &str| {
            app.selected = app
                .rows
                .iter()
                .position(|row| row.item.title == title)
                .unwrap();
            render(app, 120, 30).last().unwrap().clone()
        };
        let overdue = footer_for(&mut app, "Migrate cron jobs");
        assert!(
            overdue.contains("s reschedule   x done   T triage"),
            "{overdue}"
        );
        let done = footer_for(&mut app, "Call with bank");
        assert!(done.contains("t reopen   u undo"), "{done}");
        let inbox = footer_for(&mut app, "Compare jiff and chrono");
        assert!(inbox.contains("R refile   x done"), "{inbox}");
        assert!(inbox.ends_with("? keys "), "{inbox}");

        press(&mut app, KeyCode::Tab);
        let week = footer_for(&mut app, "Review ingestion PR");
        assert!(week.contains("f/b week   . today"), "{week}");
    });
}

#[test]
#[serial]
fn triage_advances_after_every_decision_with_a_fixed_counter() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        let path = root.join("work.org");
        fs::write(
            &path,
            "* TODO Book conference travel\nDEADLINE: <2026-10-10 Sat>\n\
* TODO Old chore\nSCHEDULED: <2026-10-01 Thu>\n\
* Inbox\n** Compare jiff and chrono\n",
        )
        .unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        press(&mut app, KeyCode::Char('T'));
        assert_eq!(app.triage.queue.len(), 3);
        while app.selected_row().unwrap().item.title != "Book conference travel" {
            press(&mut app, KeyCode::Char('j'));
        }
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains("d  move deadline"), "{screen}");
        let before = app.triage.position;

        // Scheduling leaves the deadline missed; the card still moves on.
        press(&mut app, KeyCode::Char('m'));
        assert_eq!(app.triage.position, before + 1);
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains("SCHEDULED: <2026-10-14 Wed>"), "{written}");
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains(&format!("{} / 3", before + 2)), "{screen}");
        assert!(!screen.contains("no_change"), "{screen}");

        // Inbox cards lead with refile.
        app.triage.position = 2;
        app.reload_rows().unwrap();
        assert_eq!(
            app.selected_row().unwrap().item.title,
            "Compare jiff and chrono"
        );
        let screen = render(&mut app, 100, 30).join("\n");
        let chips = screen
            .lines()
            .find(|line| line.contains("refile"))
            .unwrap_or_else(|| panic!("{screen}"));
        assert!(chips.find("refile") < chips.find("today"), "{chips}");

        // k goes back to the decided card and u returns to it after undo.
        press(&mut app, KeyCode::Char('u'));
        assert_eq!(app.triage.position, before);
        assert!(
            !fs::read_to_string(&path)
                .unwrap()
                .contains("SCHEDULED: <2026-10-14")
        );

        while !app.triage.finished() {
            press(&mut app, KeyCode::Char('j'));
        }
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains("0 decided"), "{screen}");
        assert!(screen.contains("3 / 3"), "{screen}");
    });
}

#[test]
#[serial]
fn only_captures_say_who_captured_a_task() {
    with_home(|root| {
        fs::write(root.join("inbox.org"), "* Inbox\n** TODO Written by hand\n").unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        let row = app
            .rows
            .iter()
            .position(|row| row.item.title == "Written by hand")
            .unwrap();
        app.selected = row;
        app.set_state("DONE").unwrap();
        app.undo().unwrap();
        let screen = render(&mut app, 120, 20).join("\n");
        assert!(screen.contains("Written by hand"), "{screen}");
        assert!(
            !screen.contains("you "),
            "an edit is not a capture: {screen}"
        );
    });
}

#[test]
#[serial]
fn a_full_pane_title_keeps_a_space_before_its_mark() {
    with_home(|root| {
        fs::write(
            root.join("work.org"),
            "* TODO [#A] Submit expense claims for the offsite\nDEADLINE: <2026-10-22 Thu>\n",
        )
        .unwrap();
        let mut app = test_app(root);
        app.pane = true;
        app.reload_rows().unwrap();
        let screen = render(&mut app, 34, 10).join("\n");
        // The countdown leads, as on Today; the priority keeps a space
        // before it however long the title.
        assert!(screen.contains("in 9d  Submit"), "{screen}");
        assert!(screen.contains("… A│"), "{screen}");
    });
}

#[test]
#[serial]
fn an_empty_no_color_keeps_colour() {
    let plain = |value: Option<&str>| {
        temp_env::with_var("NO_COLOR", value, || {
            super::theme::Theme::detect("dark", None).plain
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

#[test]
#[serial]
fn marked_rows_are_written_together_or_not_at_all() {
    with_home(|root| {
        let rent = "* TODO Pay rent\nSCHEDULED: <2026-10-13 Tue>\n";
        let plants = "#+TODO: OPEN | CLOSED\n* OPEN Water plants\nSCHEDULED: <2026-10-13 Tue>\n";
        fs::write(root.join("a.org"), rent).unwrap();
        fs::write(root.join("b.org"), plants).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        app.marks = app
            .rows
            .iter()
            .map(|row| row.item.item_ref.clone())
            .collect();
        assert_eq!(app.marks.len(), 2);

        // DONE is no keyword of b.org: neither file is written.
        let error = app.set_state("DONE").unwrap_err();
        app.set_error(&error);
        let status = app.status.as_ref().unwrap().text.clone();
        assert!(status.contains("Water plants"), "{status}");
        assert_eq!(fs::read_to_string(root.join("a.org")).unwrap(), rent);
        assert_eq!(fs::read_to_string(root.join("b.org")).unwrap(), plants);
        assert_eq!(app.marks.len(), 2);

        app.schedule("2026-10-20").unwrap();
        let status = app.status.as_ref().unwrap().text.clone();
        assert!(
            status.starts_with("2 items  scheduled Tue 20 Oct"),
            "{status}"
        );
        assert!(status.contains("Pay rent + 1 more"), "{status}");
        assert!(
            fs::read_to_string(root.join("a.org"))
                .unwrap()
                .contains("<2026-10-20 Tue>")
        );
        assert!(
            fs::read_to_string(root.join("b.org"))
                .unwrap()
                .contains("<2026-10-20 Tue>")
        );

        app.undo().unwrap();
        assert_eq!(fs::read_to_string(root.join("a.org")).unwrap(), rent);
        assert_eq!(fs::read_to_string(root.join("b.org")).unwrap(), plants);
    });
}

#[test]
#[serial]
fn marked_rows_that_already_match_are_left_out_of_the_write() {
    with_home(|root| {
        // Pay rent is overdue; Water plants is already on today.
        let tasks = "* TODO Pay rent\nSCHEDULED: <2026-10-12 Mon>\n* TODO Water plants\nSCHEDULED: <2026-10-13 Tue>\n";
        fs::write(root.join("tasks.org"), tasks).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        let mark_all = |app: &mut App| {
            app.marks = app
                .rows
                .iter()
                .map(|row| row.item.item_ref.clone())
                .collect();
        };
        mark_all(&mut app);
        assert_eq!(app.marks.len(), 2);

        app.schedule("2026-10-13").unwrap();
        let status = app.status.as_ref().unwrap().text.clone();
        assert!(status.contains("· Pay rent ·"), "{status}");
        let written = fs::read_to_string(root.join("tasks.org")).unwrap();
        assert_eq!(written.matches("<2026-10-13 Tue>").count(), 2, "{written}");

        // One undo reverts the whole write.
        app.undo().unwrap();
        assert_eq!(fs::read_to_string(root.join("tasks.org")).unwrap(), tasks);

        // When every marked row already matches, nothing is written.
        app.schedule("2026-10-13").unwrap();
        mark_all(&mut app);
        let before = fs::read_to_string(root.join("tasks.org")).unwrap();
        assert!(app.schedule("2026-10-13").is_err());
        assert_eq!(fs::read_to_string(root.join("tasks.org")).unwrap(), before);
    });
}

#[test]
#[serial]
fn u_undoes_only_this_sessions_changes_and_names_them() {
    with_home(|root| {
        let tasks = root.join("tasks.org");
        fs::write(&tasks, "* TODO Pay rent\nSCHEDULED: <2026-10-13 Tue>\n").unwrap();
        fs::write(root.join("inbox.org"), "* Inbox\n").unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        app.schedule("2026-10-20").unwrap();

        // An agent captures after the TUI's own change.
        app.settings.captures[0].file = Some(root.join("inbox.org").to_string_lossy().into_owned());
        let agent = agd_core::WriteContext {
            actor: "mcp:codex".to_string(),
            project: root.to_string_lossy().into_owned(),
            now: None,
        };
        agd_core::WriteEngine::with_default_journal(app.settings.clone())
            .unwrap()
            .capture(
                "t",
                &agd_core::parse_capture_line("From codex", today()).unwrap(),
                "",
                &Default::default(),
                false,
                &agent,
            )
            .unwrap();
        let inbox = fs::read_to_string(root.join("inbox.org")).unwrap();

        app.undo().unwrap();
        assert_eq!(
            app.status.as_ref().unwrap().text,
            "tasks.org:1  undid scheduled Tue 20 Oct · Pay rent"
        );
        assert!(
            fs::read_to_string(&tasks)
                .unwrap()
                .contains("<2026-10-13 Tue>")
        );
        app.undo().unwrap();
        assert_eq!(
            app.status.as_ref().unwrap().text,
            "nothing to undo from this session"
        );
        assert_eq!(fs::read_to_string(root.join("inbox.org")).unwrap(), inbox);
    });
}

#[test]
#[serial]
fn x_on_a_repeating_task_says_it_repeated_and_when_it_is_next() {
    with_home(|root| {
        let path = root.join("home.org");
        fs::write(
            &path,
            "* TODO Water plants\nSCHEDULED: <2026-10-13 Tue +1w>\n",
        )
        .unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        app.mark_done().unwrap();
        assert_eq!(
            app.status.as_ref().unwrap().text,
            "home.org:1  repeated → Tue 20 Oct · Water plants · u undo"
        );
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains("<2026-10-20 Tue +1w>")
        );
        app.undo().unwrap();
        assert_eq!(
            app.status.as_ref().unwrap().text,
            "home.org:1  undid repeated → Tue 20 Oct · Water plants"
        );
    });
}

#[test]
#[serial]
fn priority_keys_step_through_the_files_own_range() {
    with_home(|root| {
        let path = root.join("work.org");
        fs::write(
            &path,
            "#+PRIORITIES: A E C\n* TODO [#D] Low\nSCHEDULED: <2026-10-13 Tue>\n",
        )
        .unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        app.shift_priority(false).unwrap();
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains("* TODO [#E] Low"), "{written}");
    });
}

#[test]
#[serial]
fn sectioned_rows_line_titles_up_after_the_widest_keyword() {
    use ratatui::style::Color;
    with_home(|root| {
        fs::write(
            root.join("work.org"),
            "#+TODO: TODO WAITING | DONE\n\
* TODO Alpha :ops:\nSCHEDULED: <2026-10-13 Tue>\n\
* WAITING Beta\nSCHEDULED: <2026-10-13 Tue>\n\
* DONE Delta\nSCHEDULED: <2026-10-13 Tue>\n\
* Inbox\n** Gamma\n",
        )
        .unwrap();
        let mut app = test_app(root);
        app.theme = theme_for(Some("truecolor"));
        app.reload_rows().unwrap();
        app.marks.insert(app.rows[1].item.item_ref.clone());
        let buffer = render_buffer(&mut app, 120, 20);
        let title_column = |title: &str| column_of(&buffer, row_of(&buffer, title), title);
        assert_eq!(title_column("Alpha"), title_column("Beta"));
        assert_eq!(title_column("Alpha"), title_column("Gamma"));
        assert_eq!(title_column("Alpha"), title_column("Delta"));
        // Tags are metadata, muted; a bulk mark is `*` in the marker cell.
        let y = row_of(&buffer, "Alpha");
        assert_eq!(
            buffer[(column_of(&buffer, y, "#ops"), y)].fg,
            app.theme.muted
        );
        let y = row_of(&buffer, "Beta");
        let line: String = (0..120).map(|x| buffer[(x, y)].symbol()).collect();
        assert!(line.starts_with("│ * "), "{line}");
        // A done row shows DONE, so no ✓ repeats it.
        let y = row_of(&buffer, "Delta");
        let line: String = (0..120).map(|x| buffer[(x, y)].symbol()).collect();
        assert!(!line.contains('✓'), "{line}");
        assert_ne!(buffer[(column_of(&buffer, y, "Delta"), y)].fg, Color::Reset);
    });
}

#[test]
#[serial]
fn help_fits_80_by_24_with_its_hints_on_the_last_row() {
    use ratatui::style::Modifier;
    with_home(|root| {
        fs::write(root.join("tasks.org"), TASKS).unwrap();
        let mut app = test_app(root);
        app.theme = theme_for(Some("truecolor"));
        app.reload_rows().unwrap();
        press(&mut app, crossterm::event::KeyCode::Char('?'));
        let buffer = render_buffer(&mut app, 80, 24);
        let screen = render(&mut app, 80, 24);
        let text = screen.join("\n");
        let top = row_of(&buffer, "Keys");
        let left = column_of(&buffer, top, "╭ Keys") as usize;
        // Clear of the list's border on both sides, and no org-agenda column.
        assert!(left > 2, "{text}");
        assert!(!text.contains("org-agenda"), "{text}");
        assert!(!screen[top as usize].contains("scroll"), "{text}");
        let bottom = screen.iter().rposition(|line| line.contains('╰')).unwrap();
        assert!(
            screen[bottom - 1].contains("⏎ run   ↑↓ choose   esc close"),
            "{text}"
        );
        let y = row_of(&buffer, "next row");
        let key = &buffer[(column_of(&buffer, y, "n j ↓"), y)];
        assert_eq!(key.fg, app.theme.accent);
        assert!(key.modifier.contains(Modifier::BOLD));
    });
}

#[test]
#[serial]
fn help_is_a_palette_that_runs_the_action_it_finds() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        let path = root.join("work.org");
        fs::write(&path, THREE_TODAY).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        press(&mut app, KeyCode::Char('?'));
        type_keys(&mut app, "sched");
        let screen = render(&mut app, 120, 30).join("\n");
        assert!(screen.contains("› sched"), "{screen}");
        assert!(screen.contains("▌s            schedule"), "{screen}");
        assert!(screen.contains("C-c C-s"), "{screen}");
        assert!(!screen.contains("next row"), "{screen}");
        // Triage's own `s` is listed, naming where it runs.
        assert!(screen.contains("· Triage"), "{screen}");

        // esc clears the filter first, then closes.
        press(&mut app, KeyCode::Esc);
        assert!(matches!(&app.overlay, Overlay::Help(palette) if palette.query.text().is_empty()));
        press(&mut app, KeyCode::Esc);
        assert!(matches!(app.overlay, Overlay::None));

        // ⏎ runs the chosen action as its key would: here, the schedule
        // prompt, and nothing is written yet.
        press(&mut app, KeyCode::Char('?'));
        type_keys(&mut app, "sched");
        press(&mut app, KeyCode::Enter);
        assert!(
            matches!(&app.overlay, Overlay::Prompt(prompt) if prompt.kind == super::PromptKind::Schedule),
            "{:?}",
            app.overlay
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), THREE_TODAY);

        // An action of another screen says where it runs.
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char('?'));
        type_keys(&mut app, "make next");
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.overlay, Overlay::Help(_)));
        let status = &app.status.as_ref().unwrap().text;
        assert!(status.contains("works in Triage"), "{status}");
        assert_eq!(fs::read_to_string(&path).unwrap(), THREE_TODAY);
    });
}

#[test]
#[serial]
fn overdue_badges_line_their_days_up_on_the_right() {
    with_home(|root| {
        fs::write(root.join("work.org"), FIVE_OVERDUE).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        let screen = render(&mut app, 100, 20).join("\n");
        assert!(screen.contains("  -1d  TODO Missed filing"), "{screen}");
        assert!(screen.contains("  23d  TODO Sched older"), "{screen}");
    });
}

#[test]
#[serial]
fn today_reads_as_a_timeline_against_the_clock() {
    use ratatui::style::Modifier;
    with_home(|root| {
        fs::write(
            root.join("work.org"),
            "* DONE Call with bank\nSCHEDULED: <2026-10-13 Tue 08:00>\n\
* TODO Standup\nSCHEDULED: <2026-10-13 Tue 10:00>\n\
* NEXT Review PR\nSCHEDULED: <2026-10-13 Tue 15:00>\n\
* TODO One-on-one\nSCHEDULED: <2026-10-13 Tue 16:30>\n\
* TODO Pay rent\nDEADLINE: <2026-10-16 Fri>\n\
* TODO File taxes\nDEADLINE: <2026-10-14 Wed>\n\
* TODO Submit claims\nDEADLINE: <2026-10-18 Sun>\n",
        )
        .unwrap();
        let mut app = test_app(root);
        app.theme = theme_for(Some("truecolor"));
        app.clock = chrono::NaiveTime::from_hms_opt(12, 0, 30).unwrap();
        app.reload_rows().unwrap();
        let titles: Vec<&str> = app.rows.iter().map(|row| row.item.title.as_str()).collect();
        // Done rows last in their section; deadlines soonest first.
        assert_eq!(
            titles,
            [
                "Standup",
                "Review PR",
                "One-on-one",
                "Call with bank",
                "File taxes",
                "Pay rent",
                "Submit claims"
            ]
        );
        let buffer = render_buffer(&mut app, 120, 24);
        let lead = |title: &str| {
            let y = row_of(&buffer, title);
            let cells: Vec<&ratatui::buffer::Cell> = (0..120).map(|x| &buffer[(x, y)]).collect();
            let line: String = cells.iter().map(|cell| cell.symbol()).collect();
            (line, buffer[(4, y)].clone())
        };
        // The next timed task is marked and bold; a past time is muted.
        let (line, time) = lead("Review PR");
        assert!(line.starts_with("│ ▸ 15:00"), "{line}");
        assert!(time.modifier.contains(Modifier::BOLD));
        assert_eq!(time.fg, app.theme.accent);
        let (_, time) = lead("Standup");
        assert_eq!(time.fg, app.theme.muted);
        let (line, time) = lead("One-on-one");
        assert!(!line.contains('▸'), "{line}");
        assert!(!time.modifier.contains(Modifier::BOLD));
        assert_eq!(time.fg, app.theme.accent);

        // Week's today stands out.
        press(&mut app, crossterm::event::KeyCode::Tab);
        let buffer = render_buffer(&mut app, 120, 30);
        let y = row_of(&buffer, "Tuesday");
        let today = &buffer[(column_of(&buffer, y, "Tuesday"), y)];
        assert_eq!(today.fg, app.theme.accent);
        assert!(today.modifier.contains(Modifier::UNDERLINED));
    });
}

#[test]
#[serial]
fn the_capture_preview_colours_org_text_like_its_chips() {
    with_home(|root| {
        fs::write(root.join("inbox.org"), "* Inbox\n").unwrap();
        let mut app = test_app(root);
        app.theme = theme_for(Some("truecolor"));
        app.settings.captures[0].file = Some(root.join("inbox.org").to_string_lossy().into_owned());
        app.overlay = Overlay::Capture(capture::Draft::default());
        // The chooser is a list: the bar and the tint cross the whole row.
        let buffer = render_buffer(&mut app, 100, 30);
        let y = row_of(&buffer, "t  Todo");
        let bar = column_of(&buffer, y, "t  Todo") - 1;
        assert_eq!(buffer[(bar, y)].symbol(), "▌");
        assert_eq!(buffer[(bar + 40, y)].bg, app.theme.selection);

        for key in "tRenew passport due:fri on:+2d +home".chars() {
            capture::type_key(&mut app, key);
        }
        let buffer = render_buffer(&mut app, 100, 30);
        let at = |needle: &str| {
            let y = row_of(&buffer, needle);
            buffer[(column_of(&buffer, y, needle), y)].fg
        };
        assert_eq!(at("DEADLINE:"), app.theme.due);
        assert_eq!(at("SCHEDULED:"), app.theme.accent);
        assert_eq!(at(":home:"), app.theme.muted);
        let y = row_of(&buffer, "* TODO Renew");
        let star = column_of(&buffer, y, "* TODO Renew");
        assert_eq!(buffer[(star, y)].fg, app.theme.muted);
        assert_eq!(buffer[(star + 2, y)].fg, app.theme.named("orange"));
    });
}

#[test]
#[serial]
fn a_triage_card_shows_only_the_facts_it_has_and_earns_its_nudge() {
    with_home(|root| {
        let logged = "- Rescheduled from \"[2026-09-20 Sun]\" on [2026-09-21 Mon]\n".repeat(3);
        fs::write(
            root.join("work.org"),
            format!(
                "* TODO Migrate cron jobs\nSCHEDULED: <2026-10-01 Thu>\n:LOGBOOK:\n{logged}:END:\n"
            ),
        )
        .unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        press(&mut app, crossterm::event::KeyCode::Char('T'));
        let screen = render(&mut app, 100, 30);
        let text = screen.join("\n");
        // The counter ends the gauge's own row.
        let gauge = screen
            .iter()
            .find(|line| line.contains('━') || line.contains('─') && line.contains(" / "))
            .unwrap_or_else(|| panic!("{text}"));
        assert!(gauge.trim_end().ends_with("1 / 1"), "{text}");
        assert!(text.contains("Overdue · 12d"), "{text}");
        assert!(text.contains("scheduled Thu 1 Oct"), "{text}");
        assert!(text.contains("rescheduled 3x"), "{text}");
        assert!(!text.contains("no deadline"), "{text}");
        assert!(text.contains("↳ Someday?  S"), "{text}");
        // One blank row between the card and its keys.
        let bottom = screen
            .iter()
            .position(|line| line.contains('╰'))
            .unwrap_or_else(|| panic!("{text}"));
        assert!(screen[bottom + 1].trim().is_empty(), "{text}");
        assert!(screen[bottom + 2].contains(".  today"), "{text}");
    });
}

#[test]
#[serial]
fn every_agent_card_has_the_same_two_line_shape() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        // No `* Inbox` yet: the capture creates it, then the task under it.
        fs::write(root.join("inbox.org"), "").unwrap();
        fs::write(root.join("work.org"), "* TODO Rotate staging certs\n").unwrap();
        let mut app = test_app(root);
        agent_capture(&app, root, "Benchmark the agenda +perf");
        let item = app
            .index
            .items()
            .find(|item| item.title.contains("Rotate"))
            .unwrap()
            .clone();
        let context = agd_core::WriteContext {
            actor: "mcp:codex-with-a-very-long-client-name".to_string(),
            project: root.to_string_lossy().into_owned(),
            now: None,
        };
        let intent = agd_core::EditIntent {
            state: agd_core::Change::Set("DONE".to_string()),
            ..Default::default()
        };
        agd_core::WriteEngine::with_default_journal(app.settings.clone())
            .unwrap()
            .edit(&item.item_ref, &intent, false, &context)
            .unwrap();
        app.index = WorkspaceIndex::load(app.settings.clone()).unwrap();
        app.theme = theme_for(Some("truecolor"));
        press(&mut app, KeyCode::Char('A'));
        let screen = render(&mut app, 120, 20);
        let text = screen.join("\n");
        // The capture names the task, not the `* Inbox` it also made.
        assert!(
            app.journals
                .iter()
                .any(|record| record.diff.contains("+* Inbox"))
        );
        let task = screen
            .iter()
            .position(|line| line.contains("Benchmark the agenda"))
            .unwrap_or_else(|| panic!("{text}"));
        assert!(!text.contains("+ * Inbox"), "{text}");
        assert!(!text.contains(":perf:"), "{text}");
        assert!(screen[task].contains("captured  "), "{text}");
        assert!(screen[task].contains("inbox.org:"), "{text}");
        assert!(screen[task - 1].contains("● test-agent"), "{text}");
        // A long name is cut to 13 characters and "…".
        assert!(text.contains("codex-with-a-…"), "{text}");
        assert!(text.contains("TODO → DONE  Rotate staging certs"), "{text}");
        // No raw planning lines; `d` has the diff.
        assert!(!text.contains("CLOSED:"), "{text}");
        // The bar runs down the selected card; the blank row between cards
        // is never selected.
        let selected: Vec<usize> = screen
            .iter()
            .enumerate()
            .filter(|(_, line)| line.starts_with("│▌"))
            .map(|(row, _)| row)
            .collect();
        assert_eq!(selected.len(), 2, "{text}");
        assert_eq!(selected[1], selected[0] + 1, "{text}");
        press(&mut app, KeyCode::Char('j'));
        let screen = render(&mut app, 120, 20);
        let moved: Vec<usize> = screen
            .iter()
            .enumerate()
            .filter(|(_, line)| line.starts_with("│▌"))
            .map(|(row, _)| row)
            .collect();
        assert_eq!(moved[0], selected[1] + 2, "{}", screen.join("\n"));
    });
}

#[test]
#[serial]
fn the_header_shows_triage_as_a_tab_and_alerts_in_accent() {
    use crossterm::event::KeyCode;
    use ratatui::style::Modifier;
    with_home(|root| {
        fs::write(root.join("inbox.org"), "* Inbox\n").unwrap();
        fs::write(root.join("work.org"), TWO_OVERDUE).unwrap();
        let mut app = test_app(root);
        agent_capture(&app, root, "Agent task");
        app.index = WorkspaceIndex::load(app.settings.clone()).unwrap();
        app.theme = theme_for(Some("truecolor"));
        app.reload_rows().unwrap();
        let buffer = render_buffer(&mut app, 120, 20);
        let alert = &buffer[(column_of(&buffer, 0, "● 1"), 0)];
        assert_eq!(alert.fg, app.theme.accent);
        assert!(alert.modifier.contains(Modifier::BOLD));
        assert!(!render(&mut app, 120, 20)[0].contains("ctx"));

        // A context filter is an accent pill while it is on.
        app.context = Some("work".to_string());
        let buffer = render_buffer(&mut app, 120, 20);
        let pill = &buffer[(column_of(&buffer, 0, "ctx work"), 0)];
        assert_eq!(pill.fg, app.theme.accent);

        app.context = None;
        press(&mut app, KeyCode::Char('T'));
        let buffer = render_buffer(&mut app, 120, 20);
        let header = render(&mut app, 120, 20)[0].clone();
        assert!(header.starts_with(" Agenda "), "{header}");
        assert!(!header.contains("TRIAGE"), "{header}");
        let triage = &buffer[(column_of(&buffer, 0, "Triage"), 0)];
        assert!(triage.modifier.contains(Modifier::UNDERLINED), "{header}");
    });
}

#[test]
#[serial]
fn the_peek_overlay_says_when_and_keeps_its_keys_at_80_by_24() {
    use crossterm::event::KeyCode;
    use ratatui::style::Modifier;
    with_home(|root| {
        let notes: String = (1..30).map(|line| format!("note line {line}\n")).collect();
        fs::write(
            root.join("work.org"),
            format!("* 1:1 with Sam\n<2026-10-13 Tue 16:30>\n{notes}"),
        )
        .unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        press(&mut app, KeyCode::Char(' '));
        let buffer = render_buffer(&mut app, 80, 24);
        let screen: String = (0..24)
            .map(|y| (0..80).map(|x| buffer[(x, y)].symbol()).collect::<String>() + "\n")
            .collect();
        assert!(screen.contains("When       Tue 13 Oct 16:30"), "{screen}");
        assert!(!screen.contains("<2026-10-13"), "{screen}");
        assert!(screen.contains("⏎ edit"), "{screen}");
        assert!(screen.contains("y copy link"), "{screen}");
        // The screen behind the popup is dimmed, as behind every popup.
        assert!(buffer[(0, 0)].modifier.contains(Modifier::DIM));
    });
}

#[test]
#[serial]
fn the_scrollbar_reads_as_the_border() {
    with_home(|root| {
        fs::write(root.join("tasks.org"), TASKS).unwrap();
        let mut app = test_app(root);
        app.theme = theme_for(Some("truecolor"));
        app.reload_rows().unwrap();
        let buffer = render_buffer(&mut app, 100, 12);
        let border: Vec<&ratatui::buffer::Cell> = (1..10).map(|y| &buffer[(99, y)]).collect();
        let symbols: String = border.iter().map(|cell| cell.symbol()).collect();
        assert!(
            !symbols.contains('▲') && !symbols.contains('▼'),
            "{symbols}"
        );
        let thumb: Vec<_> = border.iter().filter(|cell| cell.symbol() == "┃").collect();
        assert!(!thumb.is_empty(), "{symbols}");
        assert!(thumb.iter().all(|cell| cell.fg == app.theme.muted));
        let track = border
            .iter()
            .find(|cell| cell.symbol() == "│")
            .expect("track");
        assert_eq!(track.fg, app.theme.border);
    });
}

#[test]
#[serial]
fn an_inbox_item_names_its_agent_after_the_agents_dot() {
    with_home(|root| {
        fs::write(root.join("inbox.org"), "* Inbox\n").unwrap();
        let mut app = test_app(root);
        agent_capture(&app, root, "Agent task");
        app.index = WorkspaceIndex::load(app.settings.clone()).unwrap();
        app.theme = theme_for(Some("truecolor"));
        app.reload_rows().unwrap();
        let buffer = render_buffer(&mut app, 120, 20);
        let y = row_of(&buffer, "Agent task");
        let dot = column_of(&buffer, y, "● test-agent");
        assert_eq!(buffer[(dot, y)].fg, app.theme.agent("test-agent"));
        assert_eq!(buffer[(dot + 2, y)].fg, app.theme.muted);
        // The right column ends one cell before the border.
        assert_eq!(buffer[(dot + 12, y)].symbol(), " ");
        assert_eq!(buffer[(dot + 13, y)].symbol(), "│");
    });
}

const THREE_OVERDUE: &str = "#+TODO: TODO NEXT | DONE\n\
* TODO Old one\nSCHEDULED: <2026-10-01 Thu>\n\
* TODO Old two\nSCHEDULED: <2026-10-05 Mon>\n\
* TODO Old three\nSCHEDULED: <2026-10-09 Fri>\n";

#[test]
#[serial]
fn triage_keys_mean_what_they_mean_in_the_lists() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        let path = root.join("work.org");
        fs::write(&path, THREE_OVERDUE).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        press(&mut app, KeyCode::Char('T'));
        assert_eq!(app.screen, Screen::Triage);
        // n and j go to the next card, p and k back, and none writes.
        for (key, position) in [('n', 1), ('j', 2), ('p', 1), ('k', 0)] {
            press(&mut app, KeyCode::Char(key));
            assert_eq!(app.triage.position, position, "after {key}");
        }
        assert_eq!(fs::read_to_string(&path).unwrap(), THREE_OVERDUE);

        // s asks for a date, as in the lists.
        press(&mut app, KeyCode::Char('s'));
        assert!(
            matches!(&app.overlay, Overlay::Prompt(prompt) if prompt.kind == super::PromptKind::Schedule)
        );
        press(&mut app, KeyCode::Esc);
        assert_eq!(fs::read_to_string(&path).unwrap(), THREE_OVERDUE);

        // S is someday and N makes it NEXT; each moves on to the next card.
        let first = app.selected_row().unwrap().item.title.clone();
        press(&mut app, KeyCode::Char('S'));
        let written = fs::read_to_string(&path).unwrap();
        let line = written.lines().find(|line| line.contains(&first)).unwrap();
        assert!(line.ends_with(":someday:"), "{written}");
        let second = app.selected_row().unwrap().item.title.clone();
        press(&mut app, KeyCode::Char('N'));
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains(&format!("* NEXT {second}")), "{written}");

        // Help and the chips name the same keys.
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains("N  make NEXT"), "{screen}");
        assert!(screen.contains("S  someday"), "{screen}");
        assert!(screen.contains("s  pick date"), "{screen}");
    });
}

#[test]
#[serial]
fn ctrl_n_and_ctrl_j_move_in_pickers_where_letters_are_typed() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        fs::write(root.join("work.org"), THREE_TODAY).unwrap();
        let mut app = test_app(root);
        app.settings.views = toml::from_str::<ViewsOnly>(
            r#"[[view]]
key = "k"
desc = "Keep going"
blocks = [{ kind = "alltodo" }]

[[view]]
key = "j"
desc = "Journal"
blocks = [{ kind = "alltodo" }]"#,
        )
        .unwrap()
        .view;
        app.reload_rows().unwrap();
        press(&mut app, KeyCode::Char('a'));
        press_control(&mut app, 'n');
        assert!(matches!(app.overlay, Overlay::Views { selected: 1 }));
        press_control(&mut app, 'k');
        assert!(matches!(app.overlay, Overlay::Views { selected: 0 }));
        press_control(&mut app, 'j');
        assert!(matches!(app.overlay, Overlay::Views { selected: 1 }));
        press_control(&mut app, 'p');
        assert!(matches!(app.overlay, Overlay::Views { selected: 0 }));
        assert_eq!(app.screen, Screen::Today, "moving opens no view");
        press(&mut app, KeyCode::Esc);

        press(&mut app, KeyCode::Char('R'));
        press_control(&mut app, 'n');
        press_control(&mut app, 'k');
        press_control(&mut app, 'j');
        let Overlay::Refile(picker) = &app.overlay else {
            panic!("the picker closed");
        };
        assert_eq!(picker.selected, 1);
        assert_eq!(picker.input.text(), "", "ctrl keys type nothing");
        press_control(&mut app, 'p');
        let Overlay::Refile(picker) = &app.overlay else {
            panic!("the picker closed");
        };
        assert_eq!(picker.selected, 0);
    });
}

#[test]
fn wait_yellow_and_agent_hues_stay_clear_of_due_and_done() {
    use super::theme::Theme;
    for theme in [Theme::dark(), Theme::light(), Theme::basic()] {
        assert_ne!(theme.named("yellow"), theme.due);
        for agent in ["claude-code", "codex", "opencode", "gemini", "cursor"] {
            let hue = theme.agent(agent);
            for meaning in [theme.due, theme.done, theme.overdue, theme.named("teal")] {
                assert_ne!(hue, meaning, "{agent} looks like a meaning colour");
            }
        }
    }
    // Hue angles: WAIT yellow sits well apart from due amber.
    let hue = |color: ratatui::style::Color| {
        let ratatui::style::Color::Rgb(r, g, b) = color else {
            panic!("truecolor")
        };
        let (r, g, b) = (f64::from(r), f64::from(g), f64::from(b));
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let degrees = if max == r {
            60.0 * ((g - b) / (max - min))
        } else if max == g {
            60.0 * ((b - r) / (max - min) + 2.0)
        } else {
            60.0 * ((r - g) / (max - min) + 4.0)
        };
        degrees.rem_euclid(360.0)
    };
    let dark = Theme::dark();
    assert!(hue(dark.named("yellow")) - hue(dark.due) > 18.0);
}

#[test]
#[serial]
fn auto_follows_the_background_the_terminal_reports() {
    use super::theme::Theme;
    let theme = |setting: &str, reported: Option<bool>, colorfgbg: Option<&str>| {
        with_vars(
            [
                ("NO_COLOR", None),
                ("COLORTERM", Some("truecolor")),
                ("COLORFGBG", colorfgbg),
            ],
            || Theme::detect(setting, reported).text,
        )
    };
    let (light, dark) = (Theme::light().text, Theme::dark().text);
    assert_eq!(theme("auto", Some(true), None), light);
    // The terminal's own answer wins over COLORFGBG, which wins over dark.
    assert_eq!(theme("auto", Some(false), Some("0;15")), dark);
    assert_eq!(theme("auto", None, Some("0;15")), light);
    assert_eq!(theme("auto", None, None), dark);
    // An explicit setting is never second-guessed.
    assert_eq!(theme("dark", Some(true), None), dark);
    assert_eq!(theme("light", Some(false), None), light);
}

#[test]
#[serial]
fn sixteen_colours_draw_no_grey_bars_and_muted_text_dims() {
    use ratatui::style::{Color, Modifier};
    with_home(|root| {
        fs::write(root.join("tasks.org"), TASKS).unwrap();
        let mut app = test_app(root);
        app.theme = theme_for(None);
        app.context = Some("tasks".to_string());
        app.reload_rows().unwrap();
        let buffer = render_buffer(&mut app, 120, 30);
        for cell in buffer.content() {
            assert_ne!(cell.fg, Color::DarkGray, "{:?}", cell.symbol());
            assert_ne!(cell.bg, Color::DarkGray, "{:?}", cell.symbol());
        }
        // A pill with a meaning keeps its hue reversed, so the cursor row's
        // reverse video leaves it filled.
        let y = row_of(&buffer, "Migrate cron jobs");
        let badge = &buffer[(column_of(&buffer, y, "12d"), y)];
        assert_eq!(badge.fg, Color::Red);
        assert!(badge.modifier.contains(Modifier::REVERSED));
        // The count after a section header is muted: dimmed, not grey.
        let y = row_of(&buffer, "SCHEDULE");
        let count = &buffer[(column_of(&buffer, y, "SCHEDULE") + 10, y)];
        assert!(count.modifier.contains(Modifier::DIM), "{count:?}");
    });
}

#[test]
#[serial]
fn no_color_reverses_only_pills_that_carry_state() {
    use ratatui::style::Modifier;
    with_home(|root| {
        fs::write(root.join("tasks.org"), TASKS).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        let buffer = render_buffer(&mut app, 120, 30);
        let reversed = |needle: &str| {
            let y = row_of(&buffer, needle);
            buffer[(column_of(&buffer, y, needle), y)]
                .modifier
                .contains(Modifier::REVERSED)
        };
        assert!(reversed("12d"), "an overdue badge carries state");
        assert!(!reversed("Agenda"), "the badge is chrome");
        assert!(!reversed("TODAY"), "the mode pill is chrome");
    });
}
