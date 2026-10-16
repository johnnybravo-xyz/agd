//! The visual system. Every screen, the pane, the Claude Code pane (which
//! reads these palettes from `agd agenda --views --json`) and the CLI's
//! terminal colours follow these rules; new code uses them instead of
//! choosing its own.
//!
//! Colour roles, each for one meaning:
//! - accent: focus, keys, scheduled rows and NEXT
//! - overdue (red): lateness only
//! - due (amber): deadlines, priority and TODO
//! - done (green): done and DOING
//! - yellow: WAIT; dim_red: CANCELLED
//! - muted: metadata (category, tags, ages, file:line, provenance)
//! - text: titles
//! - agent hues: only on the ● before an agent's name
//! - border, surface, selection: frames, pill fills and the cursor row
//!
//! Keywords take the user's `todo_keyword_colors` through `named`; the
//! defaults are TODO orange, NEXT blue, in-progress states green, WAIT
//! yellow and CANCELLED dim-red.
//!
//! Fills (pills) only for the Agenda badge, the footer mode pill and
//! active filters, overdue badges, priority, and parsed chips (capture's
//! tokens, a triage card's facts). In 16 colours a pill is its hue
//! reversed, so it keeps it on the reversed cursor row, and a pill without
//! a meaning colour is bold text with no fill. Under NO_COLOR only pills
//! that carry state (lateness, deadline, priority, done) are reversed.
//!
//! Layout: the 6-cell lead after the marker column always means "when"
//! (time, `in 3d`, `12d`, `-2d`); on sectioned screens the keyword column
//! is padded to the widest visible keyword (at most 9); tags show from 100
//! columns; the right meta column is right-aligned, at most 14 cells, with
//! at least one blank cell before it. One blank line above each section
//! header. Popups have two columns and one row of padding, a blank row
//! between sections and their key hints on the last row.
//!
//! Type: UPPERCASE only for section headers and pills; bold for keys, the
//! active tab, the next item and open non-TODO keywords; dim and struck
//! through for done; underline only for the active tab and today's date
//! (and, under NO_COLOR, the cursor row, which cannot be tinted); no
//! italics.
//!
//! Glyphs, each for one meaning, and no others: ▌ selection and focus, ▸
//! next, ✓ done (only where the keyword is hidden), ↻ repeat, ● agent, *
//! mark, → destination or new state, › breadcrumb and prompt, ↗ link, …
//! truncation, · separator, — none (an empty field, no keyword), ✗ error,
//! ↳ nudge, • a changed field, ▏ the text cursor, │ ┃ a scrollbar's track
//! and thumb (│ also divides the footer), ━ ─ the gauge (─ also rules off
//! peek's body), and ⏎ ↑ ↓ ← → only in key names. Nerd Font caps only
//! decorate pills; no width depends on them.
//!
//! The CLI's terminal colours (`agd agenda` on a terminal) are a subset in
//! the 16 colours, on Org's own text: day headers bold and today's also
//! underlined, categories and tags faint, lateness red, deadlines yellow,
//! done rows faint.

use std::io::{IsTerminal, Write};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{
    Block, BorderType, Borders, HighlightSpacing, List, ListItem, Padding, Scrollbar,
    ScrollbarOrientation,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// The colour roles above, built once at startup from the terminal and the
/// `theme` setting.
#[derive(Clone, Debug)]
pub(super) struct Theme {
    pub accent: Color,
    pub overdue: Color,
    pub due: Color,
    pub done: Color,
    pub text: Color,
    pub muted: Color,
    pub border: Color,
    pub surface: Color,
    pub selection: Color,
    /// Named in COLOR_NAMES for keywords and view sections only.
    yellow: Color,
    dim_red: Color,
    pink: Color,
    teal: Color,
    purple: Color,
    /// One per agent, by name; never teal or orange, which read as done
    /// and due.
    agents: [Color; 3],
    /// 16 colours: pills are reversed hues and muted text is dimmed.
    basic: bool,
    /// NO_COLOR: no colours at all; state is carried by bold, dim,
    /// reversed and strike-through instead. The roles keep the dark
    /// palette's values only so a pill knows whether it carries state.
    pub plain: bool,
    /// Pills get round caps from Nerd Font glyphs (`nerd_font = true`).
    pub rounded: bool,
}

impl Theme {
    /// `setting` is `auto`, `dark` or `light`. Auto takes the terminal's
    /// own answer about its background (`reported_light`, from
    /// `ask_background`), else COLORFGBG, which many terminals set, else
    /// dark.
    pub(super) fn detect(setting: &str, reported_light: Option<bool>) -> Self {
        if no_color() {
            return Self::plain();
        }
        let light = match setting {
            "light" => true,
            "dark" => false,
            _ => reported_light.unwrap_or_else(|| {
                std::env::var("COLORFGBG")
                    .ok()
                    .and_then(|value| value.rsplit(';').next()?.parse::<u8>().ok())
                    .is_some_and(|background| background == 7 || background >= 9)
            }),
        };
        match (truecolor(), light) {
            (true, true) => Self::light(),
            (true, false) => Self::dark(),
            (false, _) => Self::basic(),
        }
    }

    pub(super) fn dark() -> Self {
        Self {
            accent: Color::Rgb(110, 168, 254),
            overdue: Color::Rgb(240, 113, 120),
            due: Color::Rgb(229, 181, 103),
            done: Color::Rgb(126, 198, 143),
            text: Color::Rgb(222, 226, 232),
            muted: Color::Rgb(128, 136, 150),
            border: Color::Rgb(66, 74, 88),
            surface: Color::Rgb(32, 37, 46),
            selection: Color::Rgb(37, 52, 74),
            yellow: Color::Rgb(220, 214, 100),
            dim_red: Color::Rgb(176, 112, 118),
            pink: Color::Rgb(232, 121, 176),
            teal: Color::Rgb(94, 196, 160),
            purple: Color::Rgb(176, 148, 245),
            agents: [
                Color::Rgb(232, 121, 176),
                Color::Rgb(176, 148, 245),
                Color::Rgb(100, 190, 230),
            ],
            basic: false,
            plain: false,
            rounded: false,
        }
    }

    pub(super) fn light() -> Self {
        Self {
            accent: Color::Rgb(37, 99, 196),
            overdue: Color::Rgb(197, 48, 48),
            due: Color::Rgb(176, 111, 0),
            done: Color::Rgb(36, 135, 72),
            text: Color::Rgb(31, 35, 41),
            muted: Color::Rgb(108, 114, 126),
            border: Color::Rgb(196, 201, 208),
            surface: Color::Rgb(238, 240, 243),
            selection: Color::Rgb(214, 226, 242),
            yellow: Color::Rgb(120, 118, 0),
            dim_red: Color::Rgb(184, 120, 120),
            pink: Color::Rgb(200, 60, 130),
            teal: Color::Rgb(20, 140, 100),
            purple: Color::Rgb(110, 80, 200),
            agents: [
                Color::Rgb(200, 60, 130),
                Color::Rgb(110, 80, 200),
                Color::Rgb(20, 120, 170),
            ],
            basic: false,
            plain: false,
            rounded: false,
        }
    }

    /// 16-colour palette for terminals without truecolor. Text keeps the
    /// terminal's own foreground; muted and border are that foreground
    /// dimmed (DarkGray is only their name here, see `fg`).
    pub(super) fn basic() -> Self {
        Self {
            accent: Color::LightBlue,
            overdue: Color::Red,
            due: Color::Yellow,
            done: Color::Green,
            text: Color::Reset,
            muted: Color::DarkGray,
            border: Color::DarkGray,
            surface: Color::Reset,
            // Reset: the selected row is reversed (see `selected`).
            selection: Color::Reset,
            yellow: Color::LightYellow,
            dim_red: Color::Red,
            pink: Color::Magenta,
            teal: Color::Cyan,
            purple: Color::LightMagenta,
            agents: [Color::Magenta, Color::LightMagenta, Color::LightCyan],
            basic: true,
            plain: false,
            rounded: false,
        }
    }

    fn plain() -> Self {
        Self {
            plain: true,
            ..Self::dark()
        }
    }

    /// A role's foreground. NO_COLOR draws none; in 16 colours muted text
    /// is the terminal's foreground dimmed, which stays readable where dark
    /// grey would vanish or turn into a grey bar on the reversed row.
    pub(super) fn fg(&self, color: Color) -> Style {
        if self.plain {
            Style::default()
        } else if self.basic && color == self.muted {
            Style::default().add_modifier(Modifier::DIM)
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
    /// the list's highlight symbol. Truecolor tints the background. With 16
    /// colours no background is dark enough and distinct from the muted
    /// grey, so the row is reversed and every role keeps its hue. NO_COLOR
    /// state pills are already reverse video, so the row is underlined
    /// instead.
    pub(super) fn selected(&self) -> Style {
        if self.plain {
            Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else if self.basic {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default().bg(self.selection)
        }
    }

    /// A rounded badge such as `12d`, `A` or `ctx work`.
    /// Round pill caps need these Nerd Font glyphs; without one they would
    /// render as missing-glyph boxes, so they are opt-in.
    pub(super) fn rounded_pills(mut self, on: bool) -> Self {
        self.rounded = on && !self.plain;
        self
    }

    /// Whether a pill in `color` says something about the task: late, due,
    /// prioritised or done, rather than marking a key or a filter.
    fn carries_state(&self, color: Color) -> bool {
        [self.overdue, self.due, self.done].contains(&color)
    }

    /// A pill: the label on a fill, with round caps when `rounded`. The
    /// label and its padding or caps are always two cells wider than the
    /// text.
    pub(super) fn pill(&self, text: impl Into<String>, color: Color) -> Vec<Span<'static>> {
        let text = text.into();
        let padded = |style: Style| vec![Span::styled(format!(" {text} "), style)];
        let bold = Style::default().add_modifier(Modifier::BOLD);
        if self.plain {
            return if self.carries_state(color) {
                padded(bold.add_modifier(Modifier::REVERSED))
            } else {
                padded(bold)
            };
        }
        if self.basic && color == self.muted {
            return padded(bold);
        }
        // 16 colours reverse the hue, so the fill is the hue itself and
        // the label the terminal's background.
        let (label, fill) = if self.basic {
            (
                Style::default().fg(color).add_modifier(Modifier::REVERSED),
                color,
            )
        } else {
            (Style::default().fg(color).bg(self.surface), self.surface)
        };
        let label = label.add_modifier(Modifier::BOLD);
        if self.rounded {
            vec![
                Span::styled("\u{e0b6}", Style::default().fg(fill)),
                Span::styled(text, label),
                Span::styled("\u{e0b4}", Style::default().fg(fill)),
            ]
        } else {
            padded(label)
        }
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

    /// A popup's frame, as in the capture design: accent border, two
    /// columns of padding each side and a blank first row. Sections inside
    /// are separated by one blank row; key hints form the last row.
    pub(super) fn popup(&self, title: impl Into<String>) -> Block<'static> {
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(self.fg(self.accent))
            .title(Span::styled(
                format!(" {} ", title.into()),
                self.strong(self.accent),
            ))
            .padding(Padding::new(2, 2, 1, 0))
    }

    /// An inset inside a popup for Org text (previews, diffs): a rounded,
    /// subtle border with two columns of padding. Terminals cannot round a
    /// filled background, so the inset is outlined, not shaded.
    pub(super) fn inset(&self) -> Block<'static> {
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(self.fg(self.border))
            .padding(Padding::horizontal(2))
    }

    /// A list whose selected row carries the ▌ bar and the selection's
    /// tint, as every list and picker draws it.
    pub(super) fn list<'a>(&self, items: Vec<ListItem<'a>>) -> List<'a> {
        List::new(items)
            .highlight_symbol(Span::styled("▌", self.fg(self.accent)))
            .highlight_spacing(HighlightSpacing::Always)
            .highlight_style(self.selected())
    }

    /// A list's scroll position drawn on its right border: the track is
    /// the border itself and the thumb a heavier line in muted, so it
    /// reads as part of the frame.
    pub(super) fn scrollbar(&self) -> Scrollbar<'static> {
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some("│"))
            .track_style(self.fg(self.border))
            .thumb_symbol("┃")
            .thumb_style(self.muted())
    }

    /// A one-line input box: rounded, accent while focused.
    pub(super) fn input(&self, focused: bool) -> Block<'static> {
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(self.fg(if focused { self.accent } else { self.border }))
            .padding(Padding::horizontal(1))
    }

    /// A color name (agd_core::COLOR_NAMES) in this palette.
    pub(super) fn named(&self, name: &str) -> Color {
        match name {
            "orange" => self.due,
            "yellow" => self.yellow,
            "green" => self.done,
            "red" => self.overdue,
            "dim-red" => self.dim_red,
            "pink" => self.pink,
            "teal" => self.teal,
            "purple" => self.purple,
            "grey" => self.muted,
            _ => self.accent,
        }
    }

    /// The same agent always gets the same hue.
    pub(super) fn agent(&self, name: &str) -> Color {
        let index = name
            .bytes()
            .fold(0usize, |value, byte| value.wrapping_add(byte as usize))
            % self.agents.len();
        self.agents[index]
    }

    /// Every role by its name, as the Claude Code pane reads them. A role
    /// drawn in the terminal's own colour is left out: in 16 colours text
    /// is the terminal's foreground, and muted and border are it dimmed.
    fn roles_json(&self) -> serde_json::Value {
        let roles = [
            ("accent", self.accent),
            ("overdue", self.overdue),
            ("due", self.due),
            ("done", self.done),
            ("text", self.text),
            ("muted", self.muted),
            ("border", self.border),
            ("surface", self.surface),
            ("selection", self.selection),
            ("yellow", self.yellow),
            ("dim_red", self.dim_red),
            ("pink", self.pink),
            ("teal", self.teal),
            ("purple", self.purple),
        ];
        let mut json = serde_json::Map::new();
        for (role, color) in roles {
            let dimmed = self.basic && color == self.muted;
            if let Some(value) = color_name(color).filter(|_| !dimmed) {
                json.insert(role.to_string(), value.into());
            }
        }
        let agents: Vec<String> = self.agents.iter().filter_map(|c| color_name(*c)).collect();
        json.insert("agents".to_string(), agents.into());
        json.into()
    }
}

/// The TUI's palettes for other clients of agd: `dark` and `light` as
/// `#rrggbb`, `ansi` (16 colours) as Ink's colour names.
pub(super) fn palettes_json() -> serde_json::Value {
    serde_json::json!({
        "dark": Theme::dark().roles_json(),
        "light": Theme::light().roles_json(),
        "ansi": Theme::basic().roles_json(),
    })
}

/// `#rrggbb` for a truecolor value, Ink's name for one of the 16 colours,
/// None for the terminal's own.
fn color_name(color: Color) -> Option<String> {
    let name = match color {
        Color::Rgb(red, green, blue) => return Some(format!("#{red:02x}{green:02x}{blue:02x}")),
        Color::Reset => return None,
        Color::Black => "black",
        Color::Red => "red",
        Color::Green => "green",
        Color::Yellow => "yellow",
        Color::Blue => "blue",
        Color::Magenta => "magenta",
        Color::Cyan => "cyan",
        Color::Gray => "white",
        Color::DarkGray => "gray",
        Color::LightRed => "redBright",
        Color::LightGreen => "greenBright",
        Color::LightYellow => "yellowBright",
        Color::LightBlue => "blueBright",
        Color::LightMagenta => "magentaBright",
        Color::LightCyan => "cyanBright",
        Color::White => "whiteBright",
        Color::Indexed(_) => return None,
    };
    Some(name.to_string())
}

/// no-color.org: only a non-empty NO_COLOR turns colour off.
fn no_color() -> bool {
    std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty())
}

fn truecolor() -> bool {
    std::env::var("COLORTERM")
        .is_ok_and(|value| value.contains("truecolor") || value.contains("24bit"))
}

/// How long the terminal has to say what its background is.
const BACKGROUND_WAIT: Duration = Duration::from_millis(100);

/// The terminal's answer to the OSC 11 query `ESC ] 11 ; ? BEL`, as
/// crossterm reads it: Alt+`]` (ESC ]), the characters of
/// `11;rgb:RRRR/GGGG/BBBB`, then ctrl-g (BEL) or Alt+`\` (ST). While an
/// answer is awaited, its keys are taken out of the input, so one that
/// arrives late or in pieces never reaches a key binding.
#[derive(Debug, Default)]
pub(super) struct BackgroundReply {
    awaiting: bool,
    /// The answer's text so far, once Alt+`]` started it.
    text: Option<String>,
}

/// What one key was to a `BackgroundReply`.
#[derive(Debug, Eq, PartialEq)]
pub(super) enum ReplyKey {
    /// Part of the answer: drop it.
    Part,
    /// The answer is complete: whether the background is light, when its
    /// colour could be read.
    End(Option<bool>),
    /// An ordinary key.
    Other,
}

impl BackgroundReply {
    pub(super) fn feed(&mut self, key: &KeyEvent) -> ReplyKey {
        if !self.awaiting {
            return ReplyKey::Other;
        }
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(text) = &mut self.text else {
            if alt && key.code == KeyCode::Char(']') {
                self.text = Some(String::new());
                return ReplyKey::Part;
            }
            return ReplyKey::Other;
        };
        match key.code {
            KeyCode::Char('g') if control => {}
            KeyCode::Char('\\') if alt => {}
            KeyCode::Char(character)
                if !control
                    && !alt
                    && text.len() < 64
                    && (character.is_ascii_hexdigit() || ";:/rgb".contains(character)) =>
            {
                text.push(character);
                return ReplyKey::Part;
            }
            _ => {
                self.text = None;
                return ReplyKey::Other;
            }
        }
        let light = light_background(text);
        *self = Self::default();
        ReplyKey::End(light)
    }
}

/// With `theme = "auto"`, asks the terminal for its background before the
/// alternate screen opens, in raw mode so nothing is echoed, and waits up
/// to BACKGROUND_WAIT. Returns whether the background is light (None when
/// not asked or not answered), the reply state for an answer still on its
/// way, and any events that arrived meanwhile, for the event loop to handle
/// first. Not asked under NO_COLOR, without truecolor (the 16-colour
/// palette is the same on light and dark), or when stdin or stdout is not a
/// terminal.
pub(super) fn ask_background(setting: &str) -> (Option<bool>, BackgroundReply, Vec<Event>) {
    let mut reply = BackgroundReply::default();
    let terminal = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    if setting != "auto" || no_color() || !truecolor() || !terminal {
        return (None, reply, Vec::new());
    }
    let mut typed = Vec::new();
    // A terminal that cannot be asked leaves COLORFGBG to decide.
    let light = read_background(&mut reply, &mut typed).unwrap_or_default();
    let _ = disable_raw_mode();
    (light, reply, typed)
}

fn read_background(
    reply: &mut BackgroundReply,
    typed: &mut Vec<Event>,
) -> std::io::Result<Option<bool>> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    stdout.write_all(b"\x1b]11;?\x07")?;
    stdout.flush()?;
    reply.awaiting = true;
    let deadline = Instant::now() + BACKGROUND_WAIT;
    while reply.awaiting {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || !event::poll(left)? {
            return Ok(None);
        }
        let event = event::read()?;
        if let Event::Key(key) = &event {
            match reply.feed(key) {
                ReplyKey::Part => continue,
                ReplyKey::End(light) => return Ok(light),
                ReplyKey::Other => {}
            }
        }
        typed.push(event);
    }
    Ok(None)
}

/// Whether `11;rgb:RRRR/GGGG/BBBB` (or `rgba:`, with 1 to 4 hex digits a
/// component) is a light colour: relative luminance of at least 0.5.
fn light_background(reply: &str) -> Option<bool> {
    let color = reply.strip_prefix("11;")?;
    let components = color
        .strip_prefix("rgb:")
        .or_else(|| color.strip_prefix("rgba:"))?;
    let channels: Vec<f64> = components
        .split('/')
        .take(3)
        .map(|hex| {
            let value = u32::from_str_radix(hex, 16).ok()?;
            let digits = u32::try_from(hex.len())
                .ok()
                .filter(|n| (1..=4).contains(n))?;
            Some(f64::from(value) / f64::from(16u32.pow(digits) - 1))
        })
        .collect::<Option<_>>()?;
    let [red, green, blue] = channels[..] else {
        return None;
    };
    // sRGB to linear light, then WCAG's relative luminance.
    let linear = |channel: f64| {
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    };
    let luminance = 0.2126 * linear(red) + 0.7152 * linear(green) + 0.0722 * linear(blue);
    Some(luminance >= 0.5)
}

/// `text` cut to `width` terminal columns, ending in "…" when cut. Widths
/// are display widths, so CJK and emoji count two columns.
pub(super) fn truncate(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut cut = String::new();
    let mut used = 0;
    for character in text.chars() {
        let columns = character.width().unwrap_or(0);
        if used + columns + 1 > width {
            break;
        }
        cut.push(character);
        used += columns;
    }
    cut.push('…');
    cut
}

/// Where a framed list's scrollbar runs: its right border between the
/// corners, which stay rounded.
pub(super) fn scrollbar_track(frame: Rect) -> Rect {
    Rect {
        y: frame.y + 1,
        height: frame.height.saturating_sub(2),
        ..frame
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

#[cfg(test)]
mod tests {
    use super::*;

    fn char_key(character: char) -> KeyEvent {
        KeyEvent::from(KeyCode::Char(character))
    }

    fn alt(character: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(character), KeyModifiers::ALT)
    }

    fn awaiting() -> BackgroundReply {
        BackgroundReply {
            awaiting: true,
            text: None,
        }
    }

    #[test]
    fn a_background_reads_light_from_its_luminance() {
        assert_eq!(light_background("11;rgb:ffff/ffff/ffff"), Some(true));
        assert_eq!(light_background("11;rgb:1414/1717/1c1c"), Some(false));
        assert_eq!(light_background("11;rgb:ff/fa/f0"), Some(true));
        assert_eq!(light_background("11;rgba:0000/0000/0000/ffff"), Some(false));
        // Mid grey is dark: its relative luminance is about 0.22.
        assert_eq!(light_background("11;rgb:8080/8080/8080"), Some(false));
        assert_eq!(light_background("11;rgb:zz/00/00"), None);
        assert_eq!(light_background("10;rgb:ffff/ffff/ffff"), None);
    }

    #[test]
    fn a_late_reply_never_reaches_the_key_bindings() {
        let mut reply = awaiting();
        assert_eq!(reply.feed(&alt(']')), ReplyKey::Part);
        for character in "11;rgb:ffff/ffff/ffff".chars() {
            assert_eq!(reply.feed(&char_key(character)), ReplyKey::Part);
        }
        let bel = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL);
        assert_eq!(reply.feed(&bel), ReplyKey::End(Some(true)));
        // Answered: from now on every key is the person's.
        assert_eq!(reply.feed(&alt(']')), ReplyKey::Other);
        assert_eq!(reply.feed(&char_key('r')), ReplyKey::Other);

        // ST ends it too, and keys outside the answer's alphabet pass.
        let mut reply = awaiting();
        assert_eq!(reply.feed(&char_key('j')), ReplyKey::Other);
        reply.feed(&alt(']'));
        for character in "11;rgb:0000/0000/0000".chars() {
            reply.feed(&char_key(character));
        }
        assert_eq!(reply.feed(&alt('\\')), ReplyKey::End(Some(false)));
    }
}
