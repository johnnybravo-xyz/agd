//! `?`: the key palette, its layout at each size, and running what it
//! finds.

use super::*;

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
        for (_, groups) in crate::tui::keys::SCREENS {
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
        // esc's help ends on a row of its own, under its column.
        assert!(
            lines
                .iter()
                .any(|line| line.trim_matches(['│', '┃', ' ']) == "back"),
            "{text}"
        );
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
        assert!(
            screen
                .lines()
                .any(|line| line.contains("▌s ") && line.contains(" schedule ")),
            "{screen}"
        );
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
            matches!(&app.overlay, Overlay::Prompt(prompt) if prompt.kind == crate::tui::PromptKind::Schedule),
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
fn popups_take_c_o_for_editor_because_c_e_ends_the_line_there() {
    use crate::tui::keys::{Action, POPUP_KEYS, key_label};
    with_home(|root| {
        fs::write(root.join("tasks.org"), TASKS).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        assert_eq!(key_label(POPUP_KEYS, Action::Editor), "C-o");
        assert_eq!(key_label(POPUP_KEYS, Action::LineEnd), "C-e");
        // Help lists the popups' keys with the rest.
        press(&mut app, crossterm::event::KeyCode::Char('?'));
        type_keys(&mut app, "end of the line");
        let screen = render(&mut app, 120, 30).join("\n");
        assert!(screen.contains("C-e End"), "{screen}");
        assert!(screen.contains("· Popups"), "{screen}");
        // Each popup's hint row names C-o.
        press(&mut app, crossterm::event::KeyCode::Esc);
        press(&mut app, crossterm::event::KeyCode::Esc);
        press(&mut app, crossterm::event::KeyCode::Char('c'));
        crate::tui::capture::type_key(&mut app, 't');
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains("C-o body in $EDITOR"), "{screen}");
    });
}

#[test]
fn every_key_label_reads_in_one_notation() {
    // Emacs's notation for chords (C-e, S-tab), words or glyphs for named
    // keys; never `ctrl-`, `Ctrl+` or `shift-`.
    for (title, groups) in crate::tui::keys::SCREENS {
        for binding in groups.iter().flat_map(|group| group.bindings.iter()) {
            for key in binding.label.split(' ') {
                let lowered = key.to_lowercase();
                assert!(
                    !lowered.contains("ctrl") && !lowered.contains("shift") && !key.contains('+')
                        || key == "+",
                    "{title}: {} is not in Emacs notation",
                    binding.label
                );
                assert!(
                    key != "RET" && key != "Enter" && key != "ESC" && key != "TAB",
                    "{title}: {} names a key two ways",
                    binding.label
                );
            }
        }
    }
    assert!(!crate::tui::keys::MOVING.contains("ctrl"));
}
