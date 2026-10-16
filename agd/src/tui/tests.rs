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
