//! The edit popup: its fields, the dry-run preview and the one guarded
//! write.

use super::*;

fn key(app: &mut App, code: crossterm::event::KeyCode) {
    let editor = crate::tui::edit::handle_key(app, crossterm::event::KeyEvent::from(code)).unwrap();
    assert_eq!(editor, None);
}

/// The edit popup on the cursor row's task, as ⏎ opens it.
fn open_edit(app: &mut App) {
    let item = app.selected_row().unwrap().item.clone();
    app.overlay = Overlay::Edit(Box::new(crate::tui::edit::EditForm::open(app, &item)));
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
        open_edit(&mut app);
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
        open_edit(&mut app);
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
        open_edit(&mut app);
        key(&mut app, KeyCode::Char('x'));
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains("C-o open in $EDITOR"), "{screen}");
        // In a field C-e is end of line, as in the minibuffer.
        let control = |character| KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL);
        assert_eq!(
            crate::tui::edit::handle_key(&mut app, control('e')).unwrap(),
            None
        );
        assert!(matches!(app.overlay, Overlay::Edit(_)));
        let (file, line) = crate::tui::edit::handle_key(&mut app, control('o'))
            .unwrap()
            .expect("C-o opens the heading");
        assert!(file.ends_with("work.org"), "{file}");
        assert_eq!(line, 1);
        assert!(matches!(app.overlay, Overlay::None));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
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
        open_edit(&mut app);
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
