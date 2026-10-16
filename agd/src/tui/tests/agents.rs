//! The Agents tab: agent change cards, accepting, undoing and opening them.

use super::*;

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
        // `d` is no key here: the diff lives in peek.
        let before = render(&mut app, 120, 30);
        crate::tui::keys::dispatch(
            &mut app,
            crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char('d')),
        )
        .unwrap();
        assert_eq!(render(&mut app, 120, 30), before);
        let footer = before.last().unwrap();
        assert!(footer.contains("SPC diff"), "{footer}");

        crate::tui::keys::dispatch(
            &mut app,
            crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char(' ')),
        )
        .unwrap();
        let lines = render(&mut app, 120, 30);
        let screen = lines.join("\n");
        assert!(screen.contains("Peek"), "{screen}");
        assert!(screen.contains("a accept"), "{screen}");
        // The change's diff comes first, then the task and where it lives.
        let added = lines
            .iter()
            .position(|line| line.contains("+** TODO [#A] Agent task"))
            .unwrap_or_else(|| panic!("{screen}"));
        let task = lines
            .iter()
            .rposition(|line| line.contains("Agent task") && !line.contains('+'))
            .unwrap_or_else(|| panic!("{screen}"));
        assert!(added < task, "{screen}");
        assert!(screen.contains("inbox.org"), "{screen}");
    });
}

#[test]
#[serial]
fn a_second_shift_a_never_clears_the_queue_and_marks_choose_what_a_accepts() {
    use agd_core::JournalStatus::{Committed, Reviewed};
    use crossterm::event::KeyCode;
    with_home(|root| {
        fs::write(root.join("inbox.org"), "* Inbox\n").unwrap();
        let mut app = test_app(root);
        agent_capture(&app, root, "First agent task");
        agent_capture(&app, root, "Second agent task");
        agent_capture(&app, root, "Third agent task");
        app.index.refresh_path(&root.join("inbox.org")).unwrap();
        let settings = app.settings.clone();
        let journal = || {
            agd_core::WriteEngine::with_default_journal(settings.clone())
                .unwrap()
                .journal()
                .unwrap()
        };
        press(&mut app, KeyCode::Char('A'));
        assert_eq!(app.screen, Screen::Review);
        assert_eq!(app.journals.len(), 3);
        // `A` again goes nowhere new and accepts nothing.
        press(&mut app, KeyCode::Char('A'));
        assert_eq!(app.screen, Screen::Review);
        assert!(journal().iter().all(|record| record.status == Committed));

        // m marks a card and moves on; a accepts the marked cards only.
        press(&mut app, KeyCode::Char('m'));
        assert_eq!(app.selected, 1);
        let screen = render(&mut app, 120, 20);
        let text = screen.join("\n");
        assert!(text.contains("│▌* ●") || text.contains("│ * ●"), "{text}");
        assert!(screen.last().unwrap().contains("1 marked"), "{text}");
        let marked = app.journals[0].id.clone();
        press(&mut app, KeyCode::Char('a'));
        let status = app.status.as_ref().unwrap().text.clone();
        assert!(status.contains("accepted · Third agent task"), "{status}");
        assert!(status.ends_with("· nothing written"), "{status}");
        let records = journal();
        for record in &records {
            let expected = if record.id == marked {
                Reviewed
            } else {
                Committed
            };
            assert_eq!(record.status, expected, "{}", record.id);
        }
        assert!(app.marks.is_empty());

        // * marks every change; a accepts them all at once.
        press(&mut app, KeyCode::Char('*'));
        assert_eq!(app.marks.len(), 3);
        press(&mut app, KeyCode::Char('a'));
        let status = app.status.as_ref().unwrap().text.clone();
        assert!(status.starts_with("2 changes  accepted"), "{status}");
        assert!(journal().iter().all(|record| record.status == Reviewed));
        assert_eq!(app.counts.agents, 0);
        assert_eq!(
            fs::read_to_string(root.join("inbox.org"))
                .unwrap()
                .matches("agent task")
                .count(),
            3,
            "accepting writes nothing"
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
        // C-e opens the changed heading in $EDITOR, as on a row.
        let control_e = crossterm::event::KeyEvent::new(
            KeyCode::Char('e'),
            crossterm::event::KeyModifiers::CONTROL,
        );
        let (file, line) = crate::tui::keys::dispatch(&mut app, control_e)
            .unwrap()
            .unwrap();
        assert!(file.ends_with("work.org"), "{file}");
        assert_eq!(line, 20);

        // ⏎ opens the edit popup on the changed task, as on a row.
        assert_eq!(press(&mut app, KeyCode::Enter), None);
        let screen = render(&mut app, 120, 30).join("\n");
        assert!(screen.contains("Edit · work.org:20"), "{screen}");
        type_keys(&mut app, "Pay the rent");
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.overlay, Overlay::None));
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains("* DONE Pay the rent")
        );
        // Here `u` undoes the agent's change, so the line offers no undo.
        let status = app.status.as_ref().unwrap().text.clone();
        assert!(status.contains("edited · Pay the rent"), "{status}");
        assert!(!status.contains("u undo"), "{status}");
        assert_eq!(app.screen, Screen::Review);
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
        // No raw planning lines; peek has the diff.
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
fn less_than_narrows_the_agents_tab_by_agent_as_it_narrows_lists_by_category() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        fs::write(root.join("inbox.org"), "* Inbox\n").unwrap();
        let mut app = test_app(root);
        agent_capture(&app, root, "From the test agent");
        let mut settings = app.settings.clone();
        settings.captures[0].file = Some(root.join("inbox.org").to_string_lossy().into_owned());
        let codex = agd_core::WriteContext {
            actor: "mcp:codex".to_string(),
            project: root.to_string_lossy().into_owned(),
            now: None,
        };
        agd_core::WriteEngine::with_default_journal(settings)
            .unwrap()
            .capture(
                "t",
                &agd_core::parse_capture_line("From codex", today()).unwrap(),
                "",
                &Default::default(),
                false,
                &codex,
            )
            .unwrap();
        app.index = WorkspaceIndex::load(app.settings.clone()).unwrap();
        press(&mut app, KeyCode::Char('A'));
        assert_eq!(app.journals.len(), 2);
        // `f` is no key here; `<` cycles through the agents, then all.
        press(&mut app, KeyCode::Char('f'));
        assert_eq!(app.journals.len(), 2);
        press(&mut app, KeyCode::Char('<'));
        assert_eq!(app.agent_filter.as_deref(), Some("codex"));
        assert!(
            app.journals
                .iter()
                .all(|record| record.actor == "mcp:codex")
        );
        let header = render(&mut app, 120, 20)[0].clone();
        assert!(header.contains("agent codex"), "{header}");
        press(&mut app, KeyCode::Char('<'));
        assert_eq!(app.agent_filter.as_deref(), Some("test-agent"));
        press(&mut app, KeyCode::Char('<'));
        assert_eq!(app.agent_filter, None);
        assert_eq!(app.journals.len(), 2);
        // esc clears the agent filter before it leaves, as it clears a
        // list's filter.
        press(&mut app, KeyCode::Char('<'));
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.agent_filter, None);
        assert_eq!(app.screen, Screen::Review);
    });
}

#[test]
#[serial]
fn on_the_agents_tab_esc_steps_back_and_q_quits() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        fs::write(root.join("inbox.org"), "* Inbox\n").unwrap();
        let mut app = test_app(root);
        agent_capture(&app, root, "Agent task");
        app.index.refresh_path(&root.join("inbox.org")).unwrap();
        app.reload_rows().unwrap();
        press(&mut app, KeyCode::Char('A'));
        // esc closes peek, then clears the marks, then goes back.
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Char('*'));
        press(&mut app, KeyCode::Esc);
        assert!(!app.peek);
        assert_eq!(app.marks.len(), 1);
        press(&mut app, KeyCode::Esc);
        assert!(app.marks.is_empty());
        assert_eq!(app.screen, Screen::Review);
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.screen, Screen::Today);
        assert!(!app.quit);
        // T goes to Triage from the Agents tab too; q quits.
        press(&mut app, KeyCode::Char('A'));
        press(&mut app, KeyCode::Char('T'));
        assert_eq!(app.screen, Screen::Triage);
        press(&mut app, KeyCode::Char('A'));
        press(&mut app, KeyCode::Char('q'));
        assert!(app.quit);
    });
}
