//! The visual system: NO_COLOR, 16 colours, truecolor, pills and the
//! scrollbar.

use super::*;

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
                crate::tui::theme::Theme::detect("dark", None)
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
fn sixteen_colours_keep_every_role_readable_on_the_cursor_row() {
    use ratatui::style::{Color, Modifier};
    with_home(|root| {
        fs::write(root.join("tasks.org"), TASKS).unwrap();
        let mut app = test_app(root);
        app.theme = theme_for(None);
        app.reload_rows().unwrap();
        assert_eq!(app.selected_row().unwrap().item.title, "Migrate cron jobs");
        let buffer = render_buffer(&mut app, 120, 30);
        let y = row_of(&buffer, "Migrate cron jobs");
        let mut red = false;
        for x in 0..120 {
            let cell = &buffer[(x, y)];
            let (fg, bg) = if cell.modifier.contains(Modifier::REVERSED) {
                (cell.bg, cell.fg)
            } else {
                (cell.fg, cell.bg)
            };
            red |= fg == Color::Red || bg == Color::Red;
            if cell.symbol() != " " && fg != Color::Reset {
                assert_ne!(fg, bg, "unreadable cell {:?} at {x}", cell.symbol());
            }
        }
        assert!(red, "the overdue badge keeps its red on the cursor row");
    });
}

#[test]
#[serial]
fn no_color_badges_stand_out_on_the_cursor_row() {
    use ratatui::style::Modifier;
    with_home(|root| {
        fs::write(root.join("tasks.org"), TASKS).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        let buffer = render_buffer(&mut app, 120, 30);
        let y = row_of(&buffer, "Migrate cron jobs");
        let badge = column_of(&buffer, y, "12d");
        let title = column_of(&buffer, y, "Migrate");
        let reversed = |x: u16| buffer[(x, y)].modifier.contains(Modifier::REVERSED);
        assert!(reversed(badge));
        assert!(!reversed(title));
    });
}

#[test]
#[serial]
fn an_empty_no_color_keeps_colour() {
    let plain = |value: Option<&str>| {
        temp_env::with_var("NO_COLOR", value, || {
            crate::tui::theme::Theme::detect("dark", None).plain
        })
    };
    assert!(plain(Some("1")));
    assert!(!plain(Some("")));
    assert!(!plain(None));
}

#[test]
#[serial]
fn the_scrollbar_reads_as_the_border() {
    with_home(|root| {
        fs::write(root.join("tasks.org"), TASKS).unwrap();
        let mut app = test_app(root);
        app.theme = theme_for(Some("truecolor"));
        app.reload_rows().unwrap();
        let buffer = render_buffer(&mut app, 100, 12);
        let border: Vec<&ratatui::buffer::Cell> = (1..10).map(|y| &buffer[(99, y)]).collect();
        let symbols: String = border.iter().map(|cell| cell.symbol()).collect();
        assert!(
            !symbols.contains('▲') && !symbols.contains('▼'),
            "{symbols}"
        );
        let thumb: Vec<_> = border.iter().filter(|cell| cell.symbol() == "┃").collect();
        assert!(!thumb.is_empty(), "{symbols}");
        assert!(thumb.iter().all(|cell| cell.fg == app.theme.muted));
        let track = border
            .iter()
            .find(|cell| cell.symbol() == "│")
            .expect("track");
        assert_eq!(track.fg, app.theme.border);
        // The corners stay rounded: the track runs between them.
        assert_eq!(buffer[(99, 1)].symbol(), "╮");
        assert_eq!(buffer[(99, 10)].symbol(), "╯");
    });
}

#[test]
fn wait_yellow_and_agent_hues_stay_clear_of_due_and_done() {
    use crate::tui::theme::Theme;
    for theme in [Theme::dark(), Theme::light(), Theme::basic()] {
        assert_ne!(theme.named("yellow"), theme.due);
        for agent in ["claude-code", "codex", "opencode", "gemini", "cursor"] {
            let hue = theme.agent(agent);
            for meaning in [theme.due, theme.done, theme.overdue, theme.named("teal")] {
                assert_ne!(hue, meaning, "{agent} looks like a meaning colour");
            }
        }
    }
    // Hue angles: WAIT yellow sits well apart from due amber.
    let hue = |color: ratatui::style::Color| {
        let ratatui::style::Color::Rgb(r, g, b) = color else {
            panic!("truecolor")
        };
        let (r, g, b) = (f64::from(r), f64::from(g), f64::from(b));
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let degrees = if max == r {
            60.0 * ((g - b) / (max - min))
        } else if max == g {
            60.0 * ((b - r) / (max - min) + 2.0)
        } else {
            60.0 * ((r - g) / (max - min) + 4.0)
        };
        degrees.rem_euclid(360.0)
    };
    let dark = Theme::dark();
    assert!(hue(dark.named("yellow")) - hue(dark.due) > 18.0);
}

#[test]
#[serial]
fn auto_follows_the_background_the_terminal_reports() {
    use crate::tui::theme::Theme;
    let theme = |setting: &str, reported: Option<bool>, colorfgbg: Option<&str>| {
        with_vars(
            [
                ("NO_COLOR", None),
                ("COLORTERM", Some("truecolor")),
                ("COLORFGBG", colorfgbg),
            ],
            || Theme::detect(setting, reported).text,
        )
    };
    let (light, dark) = (Theme::light().text, Theme::dark().text);
    assert_eq!(theme("auto", Some(true), None), light);
    // The terminal's own answer wins over COLORFGBG, which wins over dark.
    assert_eq!(theme("auto", Some(false), Some("0;15")), dark);
    assert_eq!(theme("auto", None, Some("0;15")), light);
    assert_eq!(theme("auto", None, None), dark);
    // An explicit setting is never second-guessed.
    assert_eq!(theme("dark", Some(true), None), dark);
    assert_eq!(theme("light", Some(false), None), light);
}

#[test]
#[serial]
fn sixteen_colours_draw_no_grey_bars_and_muted_text_dims() {
    use ratatui::style::{Color, Modifier};
    with_home(|root| {
        fs::write(root.join("tasks.org"), TASKS).unwrap();
        let mut app = test_app(root);
        app.theme = theme_for(None);
        app.context = Some("tasks".to_string());
        app.reload_rows().unwrap();
        let buffer = render_buffer(&mut app, 120, 30);
        for cell in buffer.content() {
            assert_ne!(cell.fg, Color::DarkGray, "{:?}", cell.symbol());
            assert_ne!(cell.bg, Color::DarkGray, "{:?}", cell.symbol());
        }
        // A pill with a meaning keeps its hue reversed, so the cursor row's
        // reverse video leaves it filled.
        let y = row_of(&buffer, "Migrate cron jobs");
        let badge = &buffer[(column_of(&buffer, y, "12d"), y)];
        assert_eq!(badge.fg, Color::Red);
        assert!(badge.modifier.contains(Modifier::REVERSED));
        // The count after a section header is muted: dimmed, not grey.
        let y = row_of(&buffer, "SCHEDULE");
        let count = &buffer[(column_of(&buffer, y, "SCHEDULE") + 10, y)];
        assert!(count.modifier.contains(Modifier::DIM), "{count:?}");
    });
}

#[test]
#[serial]
fn no_color_reverses_only_pills_that_carry_state() {
    use ratatui::style::Modifier;
    with_home(|root| {
        fs::write(root.join("tasks.org"), TASKS).unwrap();
        let mut app = test_app(root);
        app.reload_rows().unwrap();
        let buffer = render_buffer(&mut app, 120, 30);
        let reversed = |needle: &str| {
            let y = row_of(&buffer, needle);
            buffer[(column_of(&buffer, y, needle), y)]
                .modifier
                .contains(Modifier::REVERSED)
        };
        assert!(reversed("12d"), "an overdue badge carries state");
        assert!(!reversed("Agenda"), "the badge is chrome");
        assert!(!reversed("TODAY"), "the mode pill is chrome");
    });
}
