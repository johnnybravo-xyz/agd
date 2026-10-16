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
        app.peek = true;
        let screen = render(&mut app, 120, 30).join("\n");
        assert!(screen.contains("Peek"), "{screen}");
        assert!(screen.contains("a accept"), "{screen}");
        // The peek panel names the task and where it lives.
        assert!(screen.matches("Agent task").count() >= 2, "{screen}");
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
        assert!(app.marked_changes.is_empty());

        // * marks every change; a accepts them all at once.
        press(&mut app, KeyCode::Char('*'));
        assert_eq!(app.marked_changes.len(), 3);
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
        let (file, line) = press(&mut app, KeyCode::Enter).unwrap();
        assert!(file.ends_with("work.org"), "{file}");
        assert_eq!(line, 20);
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
