//! Prompts, the filter, the refile picker and the Views popup: their keys
//! and previews.

use super::*;

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
        assert!(!screen.contains("invalid_edit"), "{screen}");
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
fn the_views_popup_takes_j_and_k_as_view_keys_and_q_closes_it() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        write_tasks(root);
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
fn prompts_and_the_refile_picker_take_minibuffer_keys() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    with_home(|root| {
        fs::write(root.join("work.org"), THREE_TODAY).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        let control = |app: &mut App, character| {
            let key = KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL);
            crate::tui::keys::dispatch(app, key).unwrap();
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
        assert!(screen.last().unwrap().contains(" 1 row "), "{screen:?}");
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
