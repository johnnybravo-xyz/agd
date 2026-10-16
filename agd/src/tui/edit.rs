//! The edit popup (⏎ on a task): its title, state, priority, tags and
//! dates as fields, the exact diff of the one guarded write they make, and
//! the hand-over to $EDITOR.

use std::collections::BTreeSet;
use std::path::Path;

use agd_core::{Change, EditIntent, Item, PlanningTimestamp, WriteEngine, parse_date_input};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};

use super::input::TextInput;
use super::keys::{self, Action};
use super::theme::centered;
use super::{App, Overlay, Screen, write_context};

const TITLE: usize = 0;
const STATE: usize = 1;
const PRIORITY: usize = 2;
const TAGS: usize = 3;
const SCHEDULED: usize = 4;
const DEADLINE: usize = 5;
const LABELS: [&str; 6] = [
    "Title",
    "State",
    "Priority",
    "Tags",
    "Scheduled",
    "Deadline",
];

/// The edit popup (⏎ on an entry): its title, state, priority, tags and
/// dates as text fields. Only fields that differ from the entry are
/// written, in one guarded write, and the preview is that write's exact
/// diff. Dates take the same input as `s` and `d` and show the entry's own
/// repeater and warning, so an untouched date is never rewritten.
#[derive(Debug)]
pub(super) struct EditForm {
    item_ref: String,
    /// The heading's file and line, for C-o.
    file: String,
    line: usize,
    location: String,
    keywords: Vec<String>,
    priorities: Vec<String>,
    original: [String; 6],
    /// The fields as typed; the focused one starts selected, so typing
    /// replaces it and an arrow or End keeps it for editing.
    values: [TextInput; 6],
    field: usize,
    preview: Result<String, String>,
    /// The preview is out of date; it is rebuilt once before the next draw.
    stale: bool,
}

impl EditForm {
    /// The popup for `item`: the cursor row's task in the lists, the
    /// changed task on the Agents tab.
    pub(super) fn open(app: &App, item: &Item) -> Self {
        let date = |timestamp: &Option<PlanningTimestamp>| {
            timestamp.as_ref().map(date_text).unwrap_or_default()
        };
        let original = [
            item.title.clone(),
            item.todo.clone().unwrap_or_default(),
            item.priority.clone().unwrap_or_default(),
            item.tags.join(" "),
            date(&item.planning.scheduled),
            date(&item.planning.deadline),
        ];
        let mut keywords = vec![String::new()];
        keywords.extend(
            app.keywords_of(item)
                .into_iter()
                .map(|(keyword, _)| keyword),
        );
        let mut priorities = vec![String::new()];
        priorities.extend(app.priority_range(item).cookies());
        let mut form = Self {
            item_ref: item.item_ref.clone(),
            file: item.file.clone(),
            line: item.line,
            location: app.short_location(item),
            keywords,
            priorities,
            values: original.clone().map(TextInput::new),
            original,
            field: TITLE,
            preview: Ok(String::new()),
            stale: false,
        };
        form.focus(TITLE);
        form
    }

    /// Moves to `field` with its text selected.
    fn focus(&mut self, field: usize) {
        self.field = field;
        self.values[field].select_all();
    }

    fn changed(&self, field: usize) -> Option<&str> {
        let value = self.values[field].text().trim();
        (value != self.original[field].trim()).then_some(value)
    }

    /// The write for the fields that changed; an empty value clears.
    fn intent(&self, app: &App) -> Result<EditIntent, String> {
        let text = |field: usize| match self.changed(field) {
            None => Change::Keep,
            Some("") => Change::Clear,
            Some(value) => Change::Set(value.to_string()),
        };
        let date = |field: usize| match self.changed(field) {
            None => Ok(Change::Keep),
            Some("") => Ok(Change::Clear),
            Some(value) => parse_date_input(value, app.today)
                .map(Change::Set)
                .map_err(|error| format!("{}: {error}", LABELS[field])),
        };
        let words = |field: usize| -> BTreeSet<String> {
            self.values[field]
                .text()
                .split([' ', ':', ','])
                .filter(|tag| !tag.is_empty())
                .map(str::to_string)
                .collect()
        };
        let (old_tags, new_tags) = (
            self.original[TAGS]
                .split(' ')
                .filter(|tag| !tag.is_empty())
                .map(str::to_string)
                .collect::<BTreeSet<_>>(),
            words(TAGS),
        );
        if self.changed(TITLE) == Some("") {
            return Err("Title: a heading needs a title".to_string());
        }
        Ok(EditIntent {
            title: text(TITLE),
            state: text(STATE),
            priority: text(PRIORITY),
            tags_add: new_tags.difference(&old_tags).cloned().collect(),
            tags_remove: old_tags.difference(&new_tags).cloned().collect(),
            scheduled: date(SCHEDULED)?,
            deadline: date(DEADLINE)?,
            ..EditIntent::default()
        })
    }

    fn unchanged(&self) -> bool {
        (0..LABELS.len()).all(|field| self.changed(field).is_none())
    }

    /// The exact lines the write would change, from a dry run.
    fn refresh(&mut self, app: &App) {
        if self.unchanged() {
            self.preview = Ok(String::new());
            return;
        }
        self.preview = self.intent(app).and_then(|intent| {
            WriteEngine::with_default_journal(app.settings.clone())
                .and_then(|engine| engine.edit(&self.item_ref, &intent, true, &write_context()))
                .map(|mutation| changed_lines(&mutation.diff))
                .map_err(|error| app.error_text(&error))
        });
    }

    /// ←/→ on State and Priority step through the file's keywords and the
    /// priority range; empty means none.
    fn cycle(&mut self, step: isize) {
        let choices = match self.field {
            STATE => &self.keywords,
            PRIORITY => &self.priorities,
            _ => return,
        };
        let current = choices
            .iter()
            .position(|choice| choice == self.values[self.field].text())
            .unwrap_or(0) as isize;
        let next = (current + step).rem_euclid(choices.len() as isize) as usize;
        let choice = choices[next].clone();
        self.values[self.field].set(choice);
    }

    /// State and Priority are choices: ←/→ step, a letter picks the first
    /// choice starting with it, Backspace picks none.
    fn choose(&mut self, key: KeyEvent) {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Left => self.cycle(-1),
            KeyCode::Right => self.cycle(1),
            KeyCode::Backspace => self.values[self.field].set(""),
            KeyCode::Char(character) if !control => {
                let choices = if self.field == STATE {
                    &self.keywords
                } else {
                    &self.priorities
                };
                let typed = character.to_lowercase().to_string();
                if let Some(choice) = choices
                    .iter()
                    .find(|choice| choice.to_lowercase().starts_with(&typed))
                    .cloned()
                {
                    self.values[self.field].set(choice);
                }
            }
            _ => {}
        }
    }
}

/// `TIMESTAMP` as the date input accepts it: `2026-10-30 14:00 +1w -2d`.
fn date_text(timestamp: &PlanningTimestamp) -> String {
    [
        Some(timestamp.date.clone()),
        timestamp.time.clone(),
        timestamp.repeater.clone(),
        timestamp.warning.clone(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
}

/// The `-` and `+` lines of a unified diff, without its file headers.
pub(super) fn changed_lines(diff: &str) -> String {
    diff.lines()
        .filter(|line| !line.starts_with("+++") && !line.starts_with("---"))
        .filter(|line| line.starts_with('+') || line.starts_with('-'))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Returns the heading to open in $EDITOR on C-o (in a field C-e is end of
/// line), which closes the popup without writing.
pub(super) fn handle_key(app: &mut App, key: KeyEvent) -> Result<keys::EditorTarget> {
    let Overlay::Edit(mut form) = std::mem::replace(&mut app.overlay, Overlay::None) else {
        return Ok(None);
    };
    let next = (form.field + 1) % LABELS.len();
    let previous = (form.field + LABELS.len() - 1) % LABELS.len();
    match keys::popup_action(key) {
        Some(Action::Editor) => return Ok(Some((form.file, form.line))),
        Some(Action::Back) => return Ok(None),
        Some(Action::Save) => {
            if !form.unchanged() {
                commit(app, form)?;
            }
            return Ok(None);
        }
        Some(Action::NextField) => form.focus(next),
        Some(Action::PreviousField) => form.focus(previous),
        _ if keys::picker_step(key) == Some(1) => form.focus(next),
        _ if keys::picker_step(key) == Some(-1) => form.focus(previous),
        _ if form.field == STATE || form.field == PRIORITY => form.choose(key),
        _ => {
            form.values[form.field].handle(key);
        }
    }
    form.stale = true;
    app.overlay = Overlay::Edit(form);
    Ok(None)
}

/// Rebuilds a stale preview with one dry run, as for capture.
pub(super) fn refresh_stale(app: &mut App) {
    // Every other overlay goes back untouched.
    app.overlay = match std::mem::replace(&mut app.overlay, Overlay::None) {
        Overlay::Edit(mut form) if form.stale => {
            form.refresh(app);
            form.stale = false;
            Overlay::Edit(form)
        }
        other => other,
    };
}

fn commit(app: &mut App, form: Box<EditForm>) -> Result<()> {
    let intent = match form.intent(app) {
        Ok(intent) => intent,
        Err(error) => {
            app.set_error(&anyhow::Error::msg(error));
            app.overlay = Overlay::Edit(form);
            return Ok(());
        }
    };
    let engine = WriteEngine::with_default_journal(app.settings.clone())?;
    let mutation = engine.edit(&form.item_ref, &intent, false, &write_context())?;
    app.index.refresh_path(Path::new(&mutation.item.file))?;
    let location = app.short_location(&mutation.item);
    app.remember_undo(
        &mutation.journal_id,
        &location,
        format!("edited · {}", mutation.item.title),
    );
    let line = crate::words::change(&location, "edited", &mutation.item.title);
    // On the Agents tab `u` undoes the agent's change, not this one.
    app.set_done(if app.screen == Screen::Review {
        line
    } else {
        format!("{line} · u undo")
    });
    app.reload_rows()
}

pub(super) fn draw(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Overlay::Edit(form) = &app.overlay else {
        return;
    };
    let theme = &app.theme;
    frame
        .buffer_mut()
        .set_style(area, Style::default().add_modifier(Modifier::DIM));
    let preview: Vec<Line<'static>> = match &form.preview {
        Ok(text) if text.is_empty() => {
            vec![Line::from(Span::styled("no changes yet", theme.muted()))]
        }
        Ok(text) => text
            .lines()
            .map(|line| {
                let color = if line.starts_with('+') {
                    theme.done
                } else {
                    theme.overdue
                };
                Line::from(Span::styled(line.to_string(), theme.fg(color)))
            })
            .collect(),
        Err(error) => vec![Line::from(Span::styled(
            error.clone(),
            theme.fg(theme.overdue),
        ))],
    };
    // border + top padding, fields, gap, preview inset, gap, footer, border
    let height = 2 + LABELS.len() as u16 + 1 + preview.len() as u16 + 2 + 1 + 1 + 1;
    // Wide enough for a headline with tags aligned at org-tags-column plus
    // the diff marker, when the terminal allows.
    let popup = centered(area, area.width.saturating_sub(4).clamp(60, 104), height);
    frame.render_widget(Clear, popup);
    let block = theme.popup(format!("Edit · {}", form.location));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(LABELS.len() as u16),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let mut fields = Vec::new();
    for (field, label) in LABELS.iter().enumerate() {
        let focused = field == form.field;
        let value = &form.values[field];
        let choice = field == STATE || field == PRIORITY;
        let mut spans = vec![
            Span::styled(if focused { "▌ " } else { "  " }, theme.fg(theme.accent)),
            Span::styled(
                format!("{label:<11}"),
                if focused {
                    theme.strong(theme.text)
                } else {
                    theme.muted()
                },
            ),
        ];
        if value.text().is_empty() && !focused {
            spans.push(Span::styled("—", theme.muted()));
        } else if focused && choice {
            spans.push(Span::styled(
                value.text().to_string(),
                theme.strong(theme.text),
            ));
            spans.push(Span::styled("  ←/→", theme.muted()));
        } else {
            spans.extend(value.spans(theme, focused));
        }
        // A date field is typed as Org writes it; beside it, how agd says it.
        if (field == SCHEDULED || field == DEADLINE)
            && let Ok(date) = parse_date_input(value.text().trim(), app.today)
        {
            spans.push(Span::styled(
                format!("  {}", super::actions::when(&date, app.today)),
                theme.muted(),
            ));
        }
        if form.changed(field).is_some() {
            spans.push(Span::styled("  •", theme.fg(theme.due)));
        }
        fields.push(Line::from(spans));
    }
    frame.render_widget(Paragraph::new(fields), parts[0]);
    frame.render_widget(
        Paragraph::new(preview)
            .wrap(Wrap { trim: false })
            .block(theme.inset()),
        parts[2],
    );
    let key = |action| keys::key_label(keys::POPUP_KEYS, action);
    frame.render_widget(
        Paragraph::new(Line::from(
            [
                theme.key_hint(key(Action::Save), "save"),
                theme.key_hint(key(Action::NextField), "next field"),
                theme.key_hint(key(Action::Back), "cancel"),
                theme.key_hint(key(Action::Editor), "open in $EDITOR"),
            ]
            .concat(),
        )),
        parts[4],
    );
}
