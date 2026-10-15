use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use org_core::{Change, EditIntent, PlanningTimestamp, WriteEngine, parse_date_input};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};

use super::theme::centered;
use super::{App, Overlay, write_context};

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
    location: String,
    keywords: Vec<String>,
    priorities: Vec<String>,
    original: [String; 6],
    values: [String; 6],
    field: usize,
    preview: Result<String, String>,
    /// The preview is out of date; it is rebuilt once before the next draw.
    stale: bool,
}

impl EditForm {
    pub(super) fn open(app: &App) -> Option<Self> {
        let item = &app.selected_row()?.item;
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
            app.selected_keywords()
                .into_iter()
                .map(|(keyword, _)| keyword),
        );
        let letter = |value: &str| value.chars().next().unwrap_or('A');
        let (highest, lowest) = (
            letter(&app.settings.highest_priority),
            letter(&app.settings.lowest_priority),
        );
        let mut priorities = vec![String::new()];
        priorities.extend((highest..=lowest).map(|letter| letter.to_string()));
        Some(Self {
            item_ref: item.item_ref.clone(),
            location: app.short_location(item),
            keywords,
            priorities,
            values: original.clone(),
            original,
            field: TITLE,
            preview: Ok(String::new()),
            stale: false,
        })
    }

    fn changed(&self, field: usize) -> Option<&str> {
        let value = self.values[field].trim();
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
                .map_err(|error| error.to_string())
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
            .position(|choice| *choice == self.values[self.field])
            .unwrap_or(0) as isize;
        let next = (current + step).rem_euclid(choices.len() as isize) as usize;
        self.values[self.field] = choices[next].clone();
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

fn changed_lines(diff: &str) -> String {
    diff.lines()
        .filter(|line| !line.starts_with("+++") && !line.starts_with("---"))
        .filter(|line| line.starts_with('+') || line.starts_with('-'))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Returns true when the caller should open the entry in $EDITOR (ctrl-e),
/// which closes the popup without writing.
pub(super) fn handle_key(app: &mut App, key: KeyEvent) -> Result<bool> {
    let Overlay::Edit(mut form) = std::mem::replace(&mut app.overlay, Overlay::None) else {
        return Ok(false);
    };
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('e') {
        return Ok(true);
    }
    match key.code {
        KeyCode::Esc => return Ok(false),
        KeyCode::Enter => {
            if !form.unchanged() {
                commit(app, form)?;
            }
            return Ok(false);
        }
        KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % LABELS.len(),
        KeyCode::BackTab | KeyCode::Up => {
            form.field = (form.field + LABELS.len() - 1) % LABELS.len()
        }
        KeyCode::Left => form.cycle(-1),
        KeyCode::Right => form.cycle(1),
        KeyCode::Backspace => {
            form.values[form.field].pop();
        }
        KeyCode::Char(character) => {
            let field = form.field;
            if field == STATE || field == PRIORITY {
                // Typing picks the first choice starting with that letter.
                let choices = if field == STATE {
                    &form.keywords
                } else {
                    &form.priorities
                };
                if let Some(choice) = choices.iter().find(|choice| {
                    choice
                        .to_lowercase()
                        .starts_with(&character.to_lowercase().to_string())
                }) {
                    form.values[field] = choice.clone();
                }
            } else {
                form.values[field].push(character);
            }
        }
        _ => {}
    }
    form.stale = true;
    app.overlay = Overlay::Edit(form);
    Ok(false)
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
            app.set_status(error);
            app.overlay = Overlay::Edit(form);
            return Ok(());
        }
    };
    let engine = WriteEngine::with_default_journal(app.settings.clone())?;
    let mutation = engine.edit(&form.item_ref, &intent, false, &write_context())?;
    app.index.refresh_path(Path::new(&mutation.item.file))?;
    app.set_status(format!("{}  edited · u undo", form.location));
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
                    theme.slipped
                };
                Line::from(Span::styled(line.to_string(), theme.fg(color)))
            })
            .collect(),
        Err(error) => vec![Line::from(Span::styled(
            error.clone(),
            theme.fg(theme.slipped),
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
        let shown = if value.is_empty() && !focused {
            Span::styled("—", theme.muted())
        } else if focused {
            Span::styled(value.clone(), theme.strong(theme.text))
        } else {
            Span::raw(value.clone())
        };
        let mut spans = vec![
            Span::styled(if focused { "› " } else { "  " }, theme.fg(theme.accent)),
            Span::styled(
                format!("{label:<11}"),
                if focused {
                    theme.strong(theme.text)
                } else {
                    theme.muted()
                },
            ),
            shown,
        ];
        if focused && (field == STATE || field == PRIORITY) {
            spans.push(Span::styled("  ←/→", theme.muted()));
        } else if focused {
            spans.push(Span::styled("▏", theme.fg(theme.accent)));
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
    frame.render_widget(
        Paragraph::new(Line::from(
            [
                theme.key_hint("⏎", "save"),
                theme.key_hint("tab", "next field"),
                theme.key_hint("esc", "cancel"),
                theme.key_hint("ctrl-e", "open in $EDITOR"),
            ]
            .concat(),
        )),
        parts[4],
    );
}
