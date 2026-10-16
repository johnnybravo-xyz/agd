//! Triage: the frozen queue, its cards and chips, and what each decision
//! writes.

use super::*;

#[test]
#[serial]
fn every_chip_fits_under_the_card_at_120_columns() {
    with_home(|root| {
        fs::write(
            root.join("work.org"),
            "* TODO Book conference travel\nDEADLINE: <2026-10-10 Sat>\n",
        )
        .unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        press(&mut app, crossterm::event::KeyCode::Char('T'));
        let screen = render(&mut app, 120, 30).join("\n");
        // A missed deadline's chips: the longest card, five chips a row.
        for chip in [
            "d  move deadline",
            "x  done",
            "s  pick date",
            "N  make NEXT",
            "R  refile",
        ] {
            assert!(screen.contains(chip), "{chip}:\n{screen}");
        }
        // The two rows' columns line up.
        let lines: Vec<&str> = screen.lines().collect();
        let first = lines
            .iter()
            .position(|line| line.contains("move deadline"))
            .unwrap();
        assert_eq!(
            lines[first].find("today"),
            lines[first + 1].find("make NEXT"),
            "{screen}"
        );
    });
}

#[test]
#[serial]
fn an_inbox_card_says_who_captured_it_and_when_as_words_say_it() {
    with_home(|root| {
        fs::write(root.join("inbox.org"), "* Inbox\n").unwrap();
        let mut app = test_app(root);
        agent_capture(&app, root, "Agent task");
        app.index = WorkspaceIndex::load(app.settings.clone()).unwrap();
        app.reload_rows().unwrap();
        press(&mut app, crossterm::event::KeyCode::Char('T'));
        let journal = agd_core::WriteEngine::with_default_journal(app.settings.clone())
            .unwrap()
            .journal()
            .unwrap();
        let when = crate::words::moment(&journal[0].timestamp, today());
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(
            screen.contains(&format!("Inbox · from test-agent {when}")),
            "{screen}"
        );
    });
}

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

#[test]
#[serial]
fn triage_advances_after_every_decision_with_a_fixed_counter() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        let path = root.join("work.org");
        fs::write(
            &path,
            "* TODO Book conference travel\nDEADLINE: <2026-10-10 Sat>\n\
* TODO Old chore\nSCHEDULED: <2026-10-01 Thu>\n",
        )
        .unwrap();
        fs::write(root.join("inbox.org"), INBOX).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        press(&mut app, KeyCode::Char('T'));
        assert_eq!(app.triage.queue.len(), 3);
        while app.selected_row().unwrap().item.title != "Book conference travel" {
            press(&mut app, KeyCode::Char('j'));
        }
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains("d  move deadline"), "{screen}");
        assert!(screen.contains(">  tomorrow"), "{screen}");
        let before = app.triage.position;

        // `m` only ever marks, and Triage has no marks: it decides nothing.
        press(&mut app, KeyCode::Char('m'));
        assert_eq!(app.triage.position, before);
        assert!(!fs::read_to_string(&path).unwrap().contains("2026-10-14"));

        // Scheduling leaves the deadline missed; the card still moves on.
        press(&mut app, KeyCode::Char('>'));
        assert_eq!(app.triage.position, before + 1);
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains("SCHEDULED: <2026-10-14 Wed>"), "{written}");
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains(&format!("{} / 3", before + 2)), "{screen}");
        assert!(!screen.contains("invalid_edit"), "{screen}");

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
fn a_triage_card_shows_only_the_facts_it_has_and_earns_its_nudge() {
    with_home(|root| {
        // reschedule_nudge is 9 by default.
        let logged = "- Rescheduled from \"[2026-09-20 Sun]\" on [2026-09-21 Mon]\n".repeat(9);
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
        assert!(text.contains("rescheduled 9x"), "{text}");
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
            matches!(&app.overlay, Overlay::Prompt(prompt) if prompt.kind == crate::tui::PromptKind::Schedule)
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
fn in_triage_q_quits_esc_goes_back_and_tab_switches_tabs() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        let path = root.join("work.org");
        fs::write(&path, THREE_OVERDUE).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        press(&mut app, KeyCode::Char('T'));
        // Triage is the last tab while it runs, so tab wraps to the first
        // and S-tab goes to the one before it, as on every screen.
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.screen, Screen::Today);
        press(&mut app, KeyCode::Char('T'));
        press(&mut app, KeyCode::BackTab);
        assert_eq!(app.screen, Screen::Review);
        // T on Triage stays on its card; A goes to the Agents tab.
        press(&mut app, KeyCode::Char('T'));
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('T'));
        assert_eq!(app.screen, Screen::Triage);
        assert_eq!(app.triage.position, 1);
        press(&mut app, KeyCode::Char('A'));
        assert_eq!(app.screen, Screen::Review);
        // esc goes back to where Triage began.
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char('T'));
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.screen, Screen::Today);
        assert!(!app.quit);
        assert_eq!(fs::read_to_string(&path).unwrap(), THREE_OVERDUE);

        // C-e opens the card's heading in $EDITOR, as in the lists.
        press(&mut app, KeyCode::Char('T'));
        let card = app.selected_row().unwrap().item.clone();
        let control_e = crossterm::event::KeyEvent::new(
            KeyCode::Char('e'),
            crossterm::event::KeyModifiers::CONTROL,
        );
        let opened = crate::tui::keys::dispatch(&mut app, control_e).unwrap();
        assert_eq!(opened, Some((card.file, card.line)));

        // On a card and on the finished pass, q quits.
        press(&mut app, KeyCode::Char('q'));
        assert!(app.quit);
        app.quit = false;
        press(&mut app, KeyCode::Char('T'));
        while !app.triage.finished() {
            press(&mut app, KeyCode::Char('n'));
        }
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains("p last card   esc goes back"), "{screen}");
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.screen, Screen::Today);
        press(&mut app, KeyCode::Char('T'));
        while !app.triage.finished() {
            press(&mut app, KeyCode::Char('n'));
        }
        press(&mut app, KeyCode::Char('q'));
        assert!(app.quit);
    });
}
