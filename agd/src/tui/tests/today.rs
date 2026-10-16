//! Today, Week, Inbox and saved views: rows, sections, the header and
//! footer, marks, the change keys, undo and peek.

use super::*;

#[test]
#[serial]
fn today_groups_sections_in_the_order_you_act_on_them() {
    with_home(|root| {
        write_tasks(root);
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
        write_tasks(root);
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
        assert!(!status.contains("ref_conflict"), "{status}");
        assert_eq!(app.rows.len(), 1);
    });
}

#[test]
#[serial]
fn y_copies_the_link_org_store_link_would_make() {
    with_home(|root| {
        fs::write(
            root.join("home.org"),
            "* TODO [#A] Pay rent :money:\n\
             * TODO Water plants\n:PROPERTIES:\n:ID: 5f1c-plants\n:END:\n",
        )
        .unwrap();
        let app = test_app(root);
        let link = |title: &str| {
            let item = app
                .index
                .items()
                .find(|item| item.title.contains(title))
                .unwrap();
            crate::tui::actions::org_link(item)
        };
        // By ID when the heading has one; else by file and heading text,
        // without its keyword, priority or tags.
        assert_eq!(link("Water"), "[[id:5f1c-plants][Water plants]]");
        let by_file = link("Pay");
        assert!(by_file.starts_with("[[file:"), "{by_file}");
        assert!(
            by_file.ends_with("home.org::*Pay rent][Pay rent]]"),
            "{by_file}"
        );
    });
}

#[test]
#[serial]
fn someday_drops_each_marked_rows_missed_deadline_as_it_does_one_rows() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        let path = root.join("work.org");
        fs::write(&path, FIVE_OVERDUE).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        // The two missed deadlines lead OVERDUE.
        type_keys(&mut app, "mm");
        press(&mut app, KeyCode::Char('S'));
        let written = fs::read_to_string(&path).unwrap();
        assert!(!written.contains("DEADLINE"), "{written}");
        assert_eq!(written.matches(":someday:").count(), 2, "{written}");
        assert!(written.contains("SCHEDULED: <2026-10-01 Thu>"), "{written}");
        let status = app.status.as_ref().unwrap().text.clone();
        assert!(status.contains("2 items  → #someday"), "{status}");
    });
}

#[test]
#[serial]
fn a_heading_without_a_keyword_reads_as_none_with_one_glyph() {
    with_home(|root| {
        write_tasks(root);
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        app.selected = app
            .rows
            .iter()
            .position(|row| row.item.title == "Compare jiff and chrono")
            .unwrap();
        app.cycle_state().unwrap();
        let status = app.status.as_ref().unwrap().text.clone();
        assert!(status.contains("  — → TODO · Compare jiff"), "{status}");
    });
}

#[test]
#[serial]
fn the_status_line_is_green_only_for_a_finished_write() {
    with_home(|root| {
        fs::write(root.join("work.org"), THREE_TODAY).unwrap();
        let mut app = test_app(root);
        app.theme = theme_for(Some("truecolor"));
        app.reload_rows().unwrap();
        let colour = |app: &mut App, text: &str| {
            let buffer = render_buffer(app, 100, 20);
            let y = buffer.area.height - 1;
            buffer[(column_of(&buffer, y, text), y)].fg
        };
        app.undo().unwrap();
        assert_eq!(colour(&mut app, "nothing to undo"), app.theme.text);
        app.schedule("fri").unwrap();
        assert_eq!(colour(&mut app, "scheduled Fri 16 Oct"), app.theme.done);
        app.set_error(&anyhow::anyhow!("editor exited with 1"));
        assert_eq!(colour(&mut app, "✗ editor exited"), app.theme.overdue);
    });
}

#[test]
#[serial]
fn a_refused_write_names_its_file_relative_to_the_root() {
    with_home(|root| {
        let mut app = test_app(root);
        // The core names files absolutely, as JSON wants them.
        let file = app.index.roots().roots()[0].join("projects/alpha.org");
        let refused = agd_core::AgdError::new(
            agd_core::ErrorCode::InvalidPath,
            format!("writes to symlinks are refused: {}", file.display()),
        );
        app.set_error(&refused.into());
        assert_eq!(
            app.status.as_ref().unwrap().text,
            "✗ writes to symlinks are refused: projects/alpha.org"
        );
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

#[test]
#[serial]
fn the_chord_hint_reads_the_same_before_and_after_a_wrong_key() {
    with_home(|root| {
        fs::write(root.join("work.org"), THREE_TODAY).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        press_control(&mut app, 'c');
        let waiting = app.status.as_ref().unwrap().text.clone();
        assert_eq!(waiting, "C-c C-w refiles · C-c C-c quits");
        press(&mut app, crossterm::event::KeyCode::Char('x'));
        assert_eq!(app.status.as_ref().unwrap().text, waiting);
        assert!(!app.quit);
        assert_eq!(
            fs::read_to_string(root.join("work.org")).unwrap(),
            THREE_TODAY
        );
    });
}

#[test]
#[serial]
fn the_footer_keeps_help_visible_at_80_columns_with_whole_hints() {
    with_home(|root| {
        write_tasks(root);
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
        let _refused = crate::tui::keys::dispatch(
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
fn today_and_triage_list_the_cores_overdue_items_once_each_in_its_order() {
    with_home(|root| {
        fs::write(
            root.join("work.org"),
            "* TODO Twice late\nSCHEDULED: <2026-09-01 Tue> DEADLINE: <2026-10-10 Sat>\n\
* TODO Old schedule\nSCHEDULED: <2026-09-20 Sun>\n\
* TODO Missed by one\nDEADLINE: <2026-10-12 Mon>\n",
        )
        .unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        // Missed deadlines first, most days late first; a task late twice
        // is one row.
        let overdue: Vec<String> = app
            .rows
            .iter()
            .filter(|row| row.section == "OVERDUE")
            .map(|row| row.item.title.clone())
            .collect();
        assert_eq!(overdue, ["Twice late", "Missed by one", "Old schedule"]);
        app.start_triage().unwrap();
        let queue: Vec<String> = app
            .triage
            .queue
            .iter()
            .map(|row| row.item.title.clone())
            .collect();
        assert_eq!(queue, overdue);
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
        assert!(screen.contains(">        schedule tomorrow"), "{screen}");
        // `m` only ever marks: in the menu it schedules nothing.
        press(&mut app, KeyCode::Char('m'));
        assert!(matches!(app.overlay, Overlay::Bulk));
        assert_eq!(fs::read_to_string(&path).unwrap(), THREE_TODAY);
        press(&mut app, KeyCode::Char('>'));
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
fn star_marks_every_row_shown_and_leaves_folded_rows_out() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        let path = root.join("work.org");
        fs::write(&path, FIVE_OVERDUE).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        // Three overdue rows show; "+2 more" stands for the rest.
        press(&mut app, KeyCode::Char('*'));
        assert_eq!(app.marks.len(), 3);
        let footer = render(&mut app, 100, 20).last().unwrap().clone();
        assert!(footer.contains("3 marked"), "{footer}");
        assert_eq!(fs::read_to_string(&path).unwrap(), FIVE_OVERDUE);
        press(&mut app, KeyCode::Char('U'));
        assert!(app.marks.is_empty());
        // One marked row is one, in the bulk menu too.
        type_keys(&mut app, "mB");
        let screen = render(&mut app, 100, 20).join("\n");
        assert!(screen.contains("Bulk · 1 marked row "), "{screen}");
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
        write_tasks(root);
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
fn org_agenda_block_titles_carry_no_count_but_groups_do() {
    with_home(|root| {
        write_tasks(root);
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
        write_tasks(root);
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
                &agd_core::parse_capture_line("From codex", today(), agd_core::Notation::Agd)
                    .unwrap(),
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
            "tasks.org:1  undid schedule · Pay rent"
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
            "home.org:1  undid state change · Water plants"
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
* DONE Delta\nSCHEDULED: <2026-10-13 Tue>\n",
        )
        .unwrap();
        fs::write(root.join("inbox.org"), "* Inbox\n** Gamma\n").unwrap();
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
fn peek_shows_the_headings_own_tags_then_the_inherited_ones_muted() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        fs::write(
            root.join("work.org"),
            "* Project :infra:\n** TODO [#B] Rotate certs :ops:\nSCHEDULED: <2026-10-13 Tue>\n",
        )
        .unwrap();
        let mut app = test_app(root);
        app.theme = theme_for(Some("truecolor"));
        app.reload_rows().unwrap();
        press(&mut app, KeyCode::Char(' '));
        let buffer = render_buffer(&mut app, 120, 30);
        let y = row_of(&buffer, "Tags");
        let line: String = (0..120).map(|x| buffer[(x, y)].symbol()).collect();
        assert!(line.contains("#ops #infra"), "{line}");
        assert!(!line.contains('↑'), "{line}");
        assert_eq!(
            buffer[(column_of(&buffer, y, "#ops"), y)].fg,
            app.theme.text
        );
        assert_eq!(
            buffer[(column_of(&buffer, y, "#infra"), y)].fg,
            app.theme.muted
        );
        // Priority is the pill, as on the row.
        let y = row_of(&buffer, "Priority");
        let cell = &buffer[(column_of(&buffer, y, " B ") + 1, y)];
        assert_eq!((cell.fg, cell.bg), (app.theme.due, app.theme.surface));
        // The row names the heading's own tags, as words::tags writes them.
        let y = row_of(&buffer, "TODO Rotate certs");
        let row: String = (0..120).map(|x| buffer[(x, y)].symbol()).collect();
        assert!(row.contains("#ops"), "{row}");
    });
}

#[test]
#[serial]
fn with_org_notation_every_tag_is_drawn_as_org_writes_it() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        fs::write(
            root.join("work.org"),
            "* Project :infra:\n** TODO Rotate certs :ops:web:\nSCHEDULED: <2026-10-13 Tue>\n",
        )
        .unwrap();
        let mut app = test_app(root);
        app.settings.notation = agd_core::Notation::Org;
        app.reload_rows().unwrap();
        let screen = render(&mut app, 120, 30).join("\n");
        assert!(screen.contains("Rotate certs  :ops:web:"), "{screen}");
        press(&mut app, KeyCode::Char(' '));
        let screen = render(&mut app, 120, 30).join("\n");
        assert!(screen.contains(":ops:web: :infra:"), "{screen}");
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Char('S'));
        let status = app.status.as_ref().unwrap().text.clone();
        assert!(status.contains("→ :someday:"), "{status}");
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

#[test]
#[serial]
fn todays_sections_count_what_is_left_to_do() {
    with_home(|root| {
        write_tasks(root);
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        let in_schedule = |app: &App, done: bool| {
            app.rows
                .iter()
                .filter(|row| row.section == "SCHEDULE" && row.item.done == done)
                .count()
        };
        assert!(in_schedule(&app, true) > 0);
        let open = in_schedule(&app, false);
        let screen = render(&mut app, 120, 40).join("\n");
        assert!(screen.contains(&format!("SCHEDULE  {open}")), "{screen}");
    });
}
