//! The narrow pane: its rows, footer and the way back to the full view.

use super::*;

#[test]
#[serial]
fn pane_sorts_by_clock_time_and_keeps_done_rows_last() {
    with_home(|root| {
        write_tasks(root);
        let mut app = test_app(root);
        app.pane = true;
        app.reload_rows().unwrap();
        // Quarterly report's deadline is in three days: Today's DUE SOON,
        // not today's.
        let titles: Vec<&str> = app.rows.iter().map(|row| row.item.title.as_str()).collect();
        assert_eq!(titles, ["Review ingestion PR", "Call with bank"]);
        let screen = render(&mut app, 35, 12).join("\n");
        assert!(screen.contains("1 overdue"), "{screen}");
        assert!(screen.contains("Inbox 1"), "{screen}");
    });
}

#[test]
#[serial]
fn a_narrow_pane_says_to_widen_instead_of_opening_the_full_view() {
    use crossterm::event::KeyCode;
    with_home(|root| {
        write_tasks(root);
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
fn a_full_pane_title_keeps_a_space_before_its_mark() {
    with_home(|root| {
        fs::write(
            root.join("work.org"),
            "* TODO [#A] Submit expense claims for the offsite\nDEADLINE: <2026-10-13 Tue>\n",
        )
        .unwrap();
        let mut app = test_app(root);
        app.pane = true;
        app.reload_rows().unwrap();
        let screen = render(&mut app, 34, 10).join("\n");
        // The lead says when, as on Today; the priority is the pill rows
        // show, a space after the title however long it is.
        assert!(screen.contains("due    Submit"), "{screen}");
        assert!(screen.contains("…  A │"), "{screen}");
    });
}

#[test]
#[serial]
fn the_pane_counts_each_item_once_as_the_status_band_does() {
    with_home(|root| {
        fs::write(
            root.join("work.org"),
            "* TODO Late twice\nSCHEDULED: <2026-09-01 Tue> DEADLINE: <2026-10-10 Sat>\n\
* TODO Both today\nSCHEDULED: <2026-10-13 Tue> DEADLINE: <2026-10-13 Tue>\n\
* TODO Due soon\nDEADLINE: <2026-10-16 Fri>\n",
        )
        .unwrap();
        let mut app = test_app(root);
        app.pane = true;
        app.reload_rows().unwrap();
        // A deadline's warning is not today's, so the pane leaves it to
        // Today, and two rows of one task count once.
        assert_eq!(crate::words::summary(&app.counts), "1 overdue · 1 today");
        let screen = render(&mut app, 34, 12).join("\n");
        assert!(screen.contains(" 1 overdue "), "{screen}");
        assert!(screen.contains("TODAY  1"), "{screen}");
        assert!(!screen.contains("Due soon"), "{screen}");
    });
}
