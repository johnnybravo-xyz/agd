use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, BorderType, Borders};

/// The few colour roles the design allows: accent for focus and keys, red
/// for slipped, amber for due and priority, green for done, one hue per
/// agent. Built once at startup from the terminal and `[tui] theme`.
#[derive(Clone, Debug)]
pub(super) struct Theme {
    pub accent: Color,
    pub slipped: Color,
    pub due: Color,
    pub done: Color,
    pub text: Color,
    pub muted: Color,
    pub border: Color,
    pub surface: Color,
    pub selection: Color,
    agents: [Color; 5],
    /// NO_COLOR: no colours at all; state is carried by bold, dim,
    /// reversed and strike-through instead.
    pub plain: bool,
}

impl Theme {
    /// `setting` is `auto`, `dark` or `light`. Auto reads COLORFGBG, which
    /// most terminals set; without it, dark is assumed.
    pub(super) fn detect(setting: &str) -> Self {
        if std::env::var_os("NO_COLOR").is_some() {
            return Self::plain();
        }
        let truecolor = std::env::var("COLORTERM")
            .is_ok_and(|value| value.contains("truecolor") || value.contains("24bit"));
        let light = match setting {
            "light" => true,
            "dark" => false,
            _ => std::env::var("COLORFGBG")
                .ok()
                .and_then(|value| value.rsplit(';').next()?.parse::<u8>().ok())
                .is_some_and(|background| background == 7 || background >= 9),
        };
        match (truecolor, light) {
            (true, true) => Self::light(),
            (true, false) => Self::dark(),
            (false, _) => Self::basic(),
        }
    }

    fn dark() -> Self {
        Self {
            accent: Color::Rgb(110, 168, 254),
            slipped: Color::Rgb(240, 113, 120),
            due: Color::Rgb(229, 181, 103),
            done: Color::Rgb(126, 198, 143),
            text: Color::Rgb(222, 226, 232),
            muted: Color::Rgb(128, 136, 150),
            border: Color::Rgb(66, 74, 88),
            surface: Color::Rgb(32, 37, 46),
            selection: Color::Rgb(37, 52, 74),
            agents: [
                Color::Rgb(232, 121, 176),
                Color::Rgb(94, 196, 160),
                Color::Rgb(176, 148, 245),
                Color::Rgb(240, 160, 90),
                Color::Rgb(100, 190, 230),
            ],
            plain: false,
        }
    }

    fn light() -> Self {
        Self {
            accent: Color::Rgb(37, 99, 196),
            slipped: Color::Rgb(197, 48, 48),
            due: Color::Rgb(176, 111, 0),
            done: Color::Rgb(36, 135, 72),
            text: Color::Rgb(31, 35, 41),
            muted: Color::Rgb(108, 114, 126),
            border: Color::Rgb(196, 201, 208),
            surface: Color::Rgb(238, 240, 243),
            selection: Color::Rgb(214, 226, 242),
            agents: [
                Color::Rgb(200, 60, 130),
                Color::Rgb(20, 140, 100),
                Color::Rgb(110, 80, 200),
                Color::Rgb(200, 100, 20),
                Color::Rgb(20, 120, 170),
            ],
            plain: false,
        }
    }

    /// 16-colour palette for terminals without truecolor.
    fn basic() -> Self {
        Self {
            accent: Color::LightBlue,
            slipped: Color::Red,
            due: Color::Yellow,
            done: Color::Green,
            text: Color::Reset,
            muted: Color::DarkGray,
            border: Color::DarkGray,
            surface: Color::Reset,
            selection: Color::DarkGray,
            agents: [
                Color::Magenta,
                Color::Cyan,
                Color::LightMagenta,
                Color::LightYellow,
                Color::LightCyan,
            ],
            plain: false,
        }
    }

    fn plain() -> Self {
        Self {
            accent: Color::Reset,
            slipped: Color::Reset,
            due: Color::Reset,
            done: Color::Reset,
            text: Color::Reset,
            muted: Color::Reset,
            border: Color::Reset,
            surface: Color::Reset,
            selection: Color::Reset,
            agents: [Color::Reset; 5],
            plain: true,
        }
    }

    pub(super) fn fg(&self, color: Color) -> Style {
        if self.plain {
            Style::default()
        } else {
            Style::default().fg(color)
        }
    }

    pub(super) fn muted(&self) -> Style {
        if self.plain {
            Style::default().add_modifier(Modifier::DIM)
        } else {
            self.fg(self.muted)
        }
    }

    pub(super) fn strong(&self, color: Color) -> Style {
        self.fg(color).add_modifier(Modifier::BOLD)
    }

    /// Done rows stay visible but dimmed and struck through.
    pub(super) fn done_row(&self) -> Style {
        self.muted()
            .add_modifier(Modifier::CROSSED_OUT | Modifier::DIM)
    }

    /// Full-width highlight of the selected row; the accent bar comes from
    /// the list's highlight symbol.
    pub(super) fn selected(&self) -> Style {
        if self.plain {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default().bg(self.selection)
        }
    }

    /// A rounded badge such as `12x`, `A` or `agents 2`.
    pub(super) fn pill(&self, text: impl Into<String>, color: Color) -> Span<'static> {
        let text = format!(" {} ", text.into());
        if self.plain {
            return Span::styled(text, Style::default().add_modifier(Modifier::REVERSED));
        }
        let style = if self.surface == Color::Reset {
            Style::default().fg(Color::Black).bg(color)
        } else {
            Style::default().fg(color).bg(self.surface)
        };
        Span::styled(text, style.add_modifier(Modifier::BOLD))
    }

    /// A key hint: the key in accent bold, its action muted.
    pub(super) fn key_hint(&self, key: &str, action: &str) -> Vec<Span<'static>> {
        vec![
            Span::styled(key.to_string(), self.strong(self.accent)),
            Span::styled(format!(" {action}   "), self.muted()),
        ]
    }

    /// Rounded panel with its title set into the border.
    pub(super) fn panel(&self, title: impl Into<String>) -> Block<'static> {
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(self.fg(self.border))
            .title(Span::styled(
                format!(" {} ", title.into()),
                self.strong(self.text),
            ))
    }

    /// The same agent always gets the same hue.
    pub(super) fn agent(&self, name: &str) -> Color {
        let index = name
            .bytes()
            .fold(0usize, |value, byte| value.wrapping_add(byte as usize))
            % self.agents.len();
        self.agents[index]
    }
}

/// A centred rectangle of at most `width` by `height`, for popups.
pub(super) fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(2));
    let height = height.min(area.height.saturating_sub(2));
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}
