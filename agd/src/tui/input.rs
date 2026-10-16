//! One line of typed text with a cursor and Emacs minibuffer keys, shared
//! by every text input: prompts, the filter, capture, the edit popup and
//! the refile picker.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Modifier;
use ratatui::text::Span;

use super::theme::Theme;

/// The text, the cursor as a character index, and whether the whole text
/// is selected (a field just focused), so typing replaces it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct TextInput {
    text: String,
    cursor: usize,
    selected: bool,
}

/// What a key did to an input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Edit {
    /// The text or cursor changed.
    Changed,
    /// C-g: leave the input without doing anything, like esc.
    Cancel,
    /// Not an editing key; the popup decides what it means.
    Other,
}

impl TextInput {
    /// `text` with the cursor at its end.
    pub(super) fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            cursor: text.chars().count(),
            text,
            selected: false,
        }
    }

    pub(super) fn text(&self) -> &str {
        &self.text
    }

    /// Selects everything, so the next character typed replaces it.
    pub(super) fn select_all(&mut self) {
        self.cursor = self.text.chars().count();
        self.selected = !self.text.is_empty();
    }

    /// Replaces the text, cursor at the end (a choice picked with ←/→).
    pub(super) fn set(&mut self, text: impl Into<String>) {
        *self = Self::new(text);
    }

    /// Applies one key: characters insert at the cursor; ←/→, Home/End and
    /// C-a/C-e move; Backspace, Delete and C-d delete; C-u clears; C-w and
    /// M-DEL delete the word before the cursor; C-g cancels.
    pub(super) fn handle(&mut self, key: KeyEvent) -> Edit {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let length = self.text.chars().count();
        match key.code {
            KeyCode::Char('g') if control => return Edit::Cancel,
            KeyCode::Char('u') if control => self.set(""),
            KeyCode::Char('w') if control => self.delete_word(),
            KeyCode::Backspace if alt => self.delete_word(),
            KeyCode::Char('a') if control => self.cursor = 0,
            KeyCode::Home => self.cursor = 0,
            KeyCode::Char('e') if control => self.cursor = length,
            KeyCode::End => self.cursor = length,
            KeyCode::Char('d') if control => self.delete_at_cursor(),
            KeyCode::Delete => self.delete_at_cursor(),
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(length),
            KeyCode::Backspace if self.selected => self.set(""),
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.delete_at_cursor();
                }
            }
            KeyCode::Char(character) if !control && !alt => {
                if self.selected {
                    self.set("");
                }
                let at = self.byte_index(self.cursor);
                self.text.insert(at, character);
                self.cursor += 1;
            }
            _ => return Edit::Other,
        }
        self.selected = false;
        Edit::Changed
    }

    fn byte_index(&self, cursor: usize) -> usize {
        self.text
            .char_indices()
            .nth(cursor)
            .map_or(self.text.len(), |(index, _)| index)
    }

    fn delete_at_cursor(&mut self) {
        if self.cursor < self.text.chars().count() {
            let at = self.byte_index(self.cursor);
            self.text.remove(at);
        }
    }

    /// Deletes back to the start of the word before the cursor, as
    /// readline's C-w does.
    fn delete_word(&mut self) {
        let before: Vec<char> = self.text.chars().take(self.cursor).collect();
        let mut start = before.len();
        while start > 0 && before[start - 1] == ' ' {
            start -= 1;
        }
        while start > 0 && before[start - 1] != ' ' {
            start -= 1;
        }
        let (from, to) = (self.byte_index(start), self.byte_index(self.cursor));
        self.text.replace_range(from..to, "");
        self.cursor = start;
    }

    /// The text with the cursor drawn at its place when `focused`; selected
    /// text is reversed, so it reads as about to be replaced.
    pub(super) fn spans(&self, theme: &Theme, focused: bool) -> Vec<Span<'static>> {
        if !focused {
            return vec![Span::raw(self.text.clone())];
        }
        let cursor = Span::styled("▏", theme.fg(theme.accent));
        if self.selected {
            return vec![
                Span::styled(
                    self.text.clone(),
                    theme.strong(theme.text).add_modifier(Modifier::REVERSED),
                ),
                cursor,
            ];
        }
        let at = self.byte_index(self.cursor);
        vec![
            Span::styled(self.text[..at].to_string(), theme.strong(theme.text)),
            cursor,
            Span::styled(self.text[at..].to_string(), theme.strong(theme.text)),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::from(code)
    }

    fn control(character: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL)
    }

    fn typed(input: &mut TextInput, text: &str) {
        for character in text.chars() {
            input.handle(key(KeyCode::Char(character)));
        }
    }

    #[test]
    fn minibuffer_keys_move_delete_and_cancel() {
        let mut input = TextInput::new("");
        typed(&mut input, "review the plan");
        input.handle(control('w'));
        assert_eq!(input.text(), "review the ");
        input.handle(control('a'));
        typed(&mut input, "re-");
        assert_eq!(input.text(), "re-review the ");
        input.handle(control('e'));
        typed(&mut input, "doc");
        assert_eq!(input.text(), "re-review the doc");
        input.handle(KeyEvent::new(KeyCode::Backspace, KeyModifiers::ALT));
        assert_eq!(input.text(), "re-review the ");
        input.handle(control('u'));
        assert_eq!(input.text(), "");
        assert_eq!(input.handle(control('g')), Edit::Cancel);
    }

    #[test]
    fn a_selected_field_is_replaced_by_typing_and_kept_by_arrows() {
        let mut input = TextInput::new("2026-10-16 10:00");
        input.select_all();
        typed(&mut input, "fri");
        assert_eq!(input.text(), "fri");

        let mut input = TextInput::new("2026-10-16");
        input.select_all();
        input.handle(key(KeyCode::End));
        typed(&mut input, " 14:00");
        assert_eq!(input.text(), "2026-10-16 14:00");
    }
}
