//! The capture popup: templates, the token line, prompts and the exact
//! preview.

use super::*;

#[test]
#[serial]
fn capture_preview_is_the_exact_text_that_will_be_written() {
    with_home(|root| {
        fs::write(root.join("inbox.org"), "* Inbox\n").unwrap();
        let mut app = test_app(root);
        app.settings.captures[0].file = Some(root.join("inbox.org").to_string_lossy().into_owned());
        app.overlay = Overlay::Capture(crate::tui::capture::Draft::default());
        for key in "tRenew passport due:fri on:2027-01-04 #personal !A".chars() {
            crate::tui::capture::type_key(&mut app, key);
        }
        // Wide enough: the heading with its tags aligned to tags_column, as
        // an edit writes them, on one line.
        let wide = render(&mut app, 120, 30);
        let heading = wide
            .iter()
            .find(|line| line.contains("TODO [#A] Renew passport"))
            .unwrap_or_else(|| panic!("{}", wide.join("\n")));
        assert!(heading.contains(":personal:"), "{heading}");
        // Narrower, the line is cut with "…"; the tags never wrap onto a
        // line of their own.
        let narrow = render(&mut app, 80, 30).join("\n");
        let heading = narrow
            .lines()
            .find(|line| line.contains("TODO [#A] Renew passport"))
            .unwrap_or_else(|| panic!("{narrow}"));
        assert!(heading.contains('…'), "{heading}");
        assert!(!narrow.contains(":personal:"), "{narrow}");
        let screen = render(&mut app, 100, 30).join("\n");
        // The tag's chip shows it as lists do.
        let lines = render(&mut app, 100, 30);
        let chips = lines
            .iter()
            .find(|line| line.contains("deadline Fri 16 Oct"))
            .unwrap_or_else(|| panic!("{screen}"));
        assert!(chips.contains(" #personal "), "{chips}");
        // `!A` is typed; its chip is the priority pill rows show.
        assert!(chips.contains("    A "), "{chips}");
        assert!(!chips.contains("!A"), "{chips}");
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
fn alt_or_shift_return_adds_checklist_lines_written_under_the_task() {
    use crate::tui::capture::{apply_key, refresh_stale, type_key};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    with_home(|root| {
        fs::write(root.join("inbox.org"), "* Inbox\n").unwrap();
        let mut app = test_app(root);
        app.settings.captures[0].file = Some(root.join("inbox.org").to_string_lossy().into_owned());
        app.overlay = Overlay::Capture(crate::tui::capture::Draft::default());
        let press = |app: &mut App, code, modifiers| {
            apply_key(app, KeyEvent::new(code, modifiers)).unwrap();
        };
        let type_text = |app: &mut App, text: &str| text.chars().for_each(|key| type_key(app, key));
        type_text(&mut app, "tPlan the offsite due:fri");
        press(&mut app, KeyCode::Enter, KeyModifiers::ALT);
        type_text(&mut app, "book venue");
        // S-⏎ where the terminal reports it apart from ⏎.
        press(&mut app, KeyCode::Enter, KeyModifiers::SHIFT);
        type_text(&mut app, "send invites");
        // ⌫ on an empty checklist line removes it.
        press(&mut app, KeyCode::Enter, KeyModifiers::ALT);
        press(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
        refresh_stale(&mut app);
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains("- [ ] book venue"), "{screen}");
        assert!(screen.contains("M-⏎ checklist"), "{screen}");
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        let text = fs::read_to_string(root.join("inbox.org")).unwrap();
        assert!(text.contains("** TODO Plan the offsite\n"), "{text}");
        assert!(
            text.ends_with("- [ ] book venue\n- [ ] send invites\n"),
            "{text}"
        );
    });
}

#[test]
#[serial]
fn capture_opens_on_a_list_of_templates_with_their_destinations() {
    with_home(|root| {
        let mut app = test_app(root);
        app.overlay = Overlay::Capture(crate::tui::capture::Draft::default());
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains("choose a template"), "{screen}");
        assert!(screen.contains("t  Todo"), "{screen}");
        assert!(screen.contains("→ inbox.org › Inbox"), "{screen}");
        assert!(screen.contains("esc cancel"), "{screen}");
        // The tokens a line takes, in agd's notation.
        assert!(screen.contains("#work tag  !A priority"), "{screen}");
    });
}

#[test]
#[serial]
fn with_org_notation_capture_takes_and_shows_orgs_tags_and_priority() {
    with_home(|root| {
        fs::write(root.join("inbox.org"), "* Inbox\n").unwrap();
        let mut app = test_app(root);
        app.settings.notation = agd_core::Notation::Org;
        app.settings.captures[0].file = Some(root.join("inbox.org").to_string_lossy().into_owned());
        app.overlay = Overlay::Capture(crate::tui::capture::Draft::default());
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains(":work: tag  #A priority"), "{screen}");
        crate::tui::capture::type_key(&mut app, 't');
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains(":work:  #A"), "{screen}");
        for key in "Fix #42 :ops:web: #B".chars() {
            crate::tui::capture::type_key(&mut app, key);
        }
        let lines = render(&mut app, 120, 30);
        let screen = lines.join("\n");
        assert!(screen.contains("TODO [#B] Fix #42"), "{screen}");
        assert!(screen.contains(":ops:web:"), "{screen}");
        let chips = lines
            .iter()
            .find(|line| line.contains(" :ops: "))
            .unwrap_or_else(|| panic!("{screen}"));
        assert!(chips.contains(" :web: "), "{chips}");
        assert!(chips.contains(" B "), "{chips}");
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
        app.overlay = Overlay::Capture(crate::tui::capture::Draft::default());
        for key in "tRenew passport".chars() {
            crate::tui::capture::type_key(&mut app, key);
        }
        let asking = render(&mut app, 100, 30).join("\n");
        assert!(asking.contains("or due: above"), "{asking}");

        for key in " due:fri".chars() {
            crate::tui::capture::type_key(&mut app, key);
        }
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(!screen.contains("or due: above"), "{screen}");
        assert!(screen.contains("DEADLINE: <2026-10-16 Fri>"), "{screen}");
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
        app.overlay = Overlay::Capture(crate::tui::capture::Draft::default());
        // The chooser is a list: the bar and the tint cross the whole row.
        let buffer = render_buffer(&mut app, 100, 30);
        let y = row_of(&buffer, "t  Todo");
        let bar = column_of(&buffer, y, "t  Todo") - 1;
        assert_eq!(buffer[(bar, y)].symbol(), "▌");
        assert_eq!(buffer[(bar + 40, y)].bg, app.theme.selection);

        for key in "tRenew passport due:fri on:+2d #home".chars() {
            crate::tui::capture::type_key(&mut app, key);
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
