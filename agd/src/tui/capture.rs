use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Stdout, Write};
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use chrono::{Datelike, NaiveDate};
use crossterm::event::{EnableMouseCapture, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{EnterAlternateScreen, enable_raw_mode};
use org_core::{CaptureDraft, CapturePrompt, WriteEngine, capture_prompts, parse_capture_line};
use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};

use super::theme::{Theme, centered};
use super::{App, Overlay, stop_terminal, write_context};

/// The capture popup's state. Field 0 is the one-line title with tokens;
/// later fields answer the template's `%^{prompt}`s.
#[derive(Debug)]
pub(super) struct Draft {
    template: Option<String>,
    input: String,
    prompts: Vec<(CapturePrompt, String)>,
    field: usize,
    body: String,
    preview: Result<String, String>,
    /// The highlighted row of the template list, before one is chosen.
    selected: usize,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            template: None,
            input: String::new(),
            prompts: Vec::new(),
            field: 0,
            selected: 0,
            body: String::new(),
            preview: Ok(String::new()),
        }
    }
}

impl Draft {
    fn choose(&mut self, app: &App, key: &str) {
        let Some(template) = app.settings.capture_template(key) else {
            return;
        };
        self.template = Some(key.to_string());
        self.prompts = capture_prompts(template)
            .into_iter()
            .map(|prompt| (prompt, String::new()))
            .collect();
        self.field = 0;
    }

    fn current(&mut self) -> &mut String {
        match self.field {
            0 => &mut self.input,
            index => &mut self.prompts[index - 1].1,
        }
    }

    /// Field numbers (1-based) of the prompts still to ask: a date prompt
    /// the capture line already answers with `due:` or `on:` is skipped.
    fn open_fields(&self, app: &App) -> Vec<usize> {
        let parsed = self.parsed(app).ok();
        self.prompts
            .iter()
            .enumerate()
            .filter(|(_, (prompt, _))| {
                !parsed
                    .as_ref()
                    .is_some_and(|draft| prompt.answered_by(draft))
            })
            .map(|(index, _)| index + 1)
            .collect()
    }

    fn next_field(&self, app: &App) -> Option<usize> {
        self.open_fields(app)
            .into_iter()
            .find(|field| *field > self.field)
    }

    fn previous_field(&self, app: &App) -> usize {
        self.open_fields(app)
            .into_iter()
            .rfind(|field| *field < self.field)
            .unwrap_or(0)
    }

    fn parsed(&self, app: &App) -> Result<CaptureDraft, String> {
        parse_capture_line(&self.input, app.today).map_err(|error| error.to_string())
    }

    /// The exact text that will be written, from a dry-run capture through
    /// the real template and write path.
    fn refresh(&mut self, app: &App) {
        let Some(template) = &self.template else {
            self.preview = Ok(String::new());
            return;
        };
        if self.input.trim().is_empty() {
            self.preview = Ok(String::new());
            return;
        }
        let prompts = prompt_answers(&self.prompts);
        self.preview = self.parsed(app).and_then(|draft| {
            WriteEngine::with_default_journal(app.settings.clone())
                .and_then(|engine| {
                    engine.capture(
                        template,
                        &draft,
                        &self.body,
                        &prompts,
                        true,
                        &write_context(),
                    )
                })
                .map(|mutation| added_lines(&mutation.diff))
                .map_err(|error| error.to_string())
        });
    }
}

fn prompt_answers(prompts: &[(CapturePrompt, String)]) -> BTreeMap<String, String> {
    prompts
        .iter()
        .map(|(prompt, value)| (prompt.name.clone(), value.clone()))
        .collect()
}

fn added_lines(diff: &str) -> String {
    diff.lines()
        .filter(|line| !line.starts_with("+++"))
        .filter_map(|line| line.strip_prefix('+'))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Types one character into the popup as a key press would.
#[cfg(test)]
pub(super) fn type_key(app: &mut App, character: char) {
    let Overlay::Capture(mut draft) = std::mem::replace(&mut app.overlay, Overlay::None) else {
        return;
    };
    if draft.template.is_none() {
        draft.choose(app, &character.to_string());
    } else {
        draft.current().push(character);
    }
    draft.refresh(app);
    app.overlay = Overlay::Capture(draft);
}

pub(super) fn handle_key(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    key: KeyEvent,
) -> Result<()> {
    let Overlay::Capture(mut draft) = std::mem::replace(&mut app.overlay, Overlay::None) else {
        return Ok(());
    };
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Esc => {
            if app.capture_only {
                app.quit = true;
            }
            return Ok(());
        }
        KeyCode::Char('e') if control => edit_body(terminal, app, &mut draft)?,
        KeyCode::Tab => {
            let keys: Vec<String> = app
                .settings
                .captures
                .iter()
                .map(|template| template.key.clone())
                .collect();
            let index = draft
                .template
                .as_ref()
                .and_then(|current| keys.iter().position(|key| key == current))
                .map_or(0, |index| (index + 1) % keys.len());
            if let Some(key) = keys.get(index) {
                draft.choose(app, key);
            }
        }
        // Before a template is chosen, ↑/↓ and ⏎ pick one from the list.
        KeyCode::Down if draft.template.is_none() => {
            draft.selected =
                (draft.selected + 1).min(app.settings.captures.len().saturating_sub(1));
        }
        KeyCode::Up if draft.template.is_none() => {
            draft.selected = draft.selected.saturating_sub(1);
        }
        KeyCode::Enter if draft.template.is_none() => {
            if let Some(template) = app.settings.captures.get(draft.selected) {
                let key = template.key.clone();
                draft.choose(app, &key);
            }
        }
        KeyCode::Down => {
            if let Some(field) = draft.next_field(app) {
                draft.field = field;
            }
        }
        KeyCode::Up => draft.field = draft.previous_field(app),
        KeyCode::Backspace => {
            if draft.field == 0 && draft.input.is_empty() {
                draft.template = None;
                draft.prompts.clear();
            } else {
                draft.current().pop();
            }
        }
        KeyCode::Enter => {
            if draft.template.is_some() && !draft.input.trim().is_empty() {
                match draft.next_field(app) {
                    Some(field) => draft.field = field,
                    None => return commit(app, draft),
                }
            }
        }
        KeyCode::Char(character) => {
            // On an empty line the first letter picks a template, like
            // C-c c t in Emacs.
            if draft.template.is_none() {
                draft.choose(app, &character.to_string());
                if draft.template.is_none() {
                    app.set_status(format!("no capture template '{character}'"));
                }
            } else {
                draft.current().push(character);
            }
        }
        _ => {}
    }
    draft.refresh(app);
    app.overlay = Overlay::Capture(draft);
    Ok(())
}

fn commit(app: &mut App, draft: Draft) -> Result<()> {
    let template = draft.template.clone().unwrap_or_default();
    let parsed = parse_capture_line(&draft.input, app.today)?;
    let prompts = prompt_answers(&draft.prompts);
    let mutation = WriteEngine::with_default_journal(app.settings.clone())?.capture(
        &template,
        &parsed,
        &draft.body,
        &prompts,
        false,
        &write_context(),
    )?;
    app.index.refresh_path(Path::new(&mutation.item.file))?;
    app.set_status(format!(
        "captured to {} · u undo",
        app.short_location(&mutation.item)
    ));
    if app.capture_only {
        app.quit = true;
        return Ok(());
    }
    app.reload_rows()
}

/// ctrl-e: the body goes through $EDITOR as a temporary file.
fn edit_body(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    draft: &mut Draft,
) -> Result<()> {
    let mut file = tempfile::Builder::new()
        .suffix(".org")
        .tempfile()
        .context("cannot create capture draft")?;
    file.write_all(draft.body.as_bytes())?;
    file.flush()?;
    let (program, args) = app
        .settings
        .editor_command(&file.path().to_string_lossy(), 1, 1);
    stop_terminal(terminal)?;
    let status = Command::new(&program)
        .args(args)
        .status()
        .with_context(|| format!("cannot start editor {program}"));
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    terminal.clear()?;
    if status?.success() {
        draft.body = fs::read_to_string(file.path()).context("cannot read capture draft")?;
        draft.body = draft.body.trim_end().to_string();
    }
    Ok(())
}

pub(super) fn draw(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Overlay::Capture(draft) = &app.overlay else {
        return;
    };
    let theme = &app.theme;
    // Dim whatever is behind the popup.
    frame
        .buffer_mut()
        .set_style(area, Style::default().add_modifier(Modifier::DIM));
    let Some(template) = draft
        .template
        .as_ref()
        .and_then(|key| app.settings.capture_template(key))
    else {
        return draw_chooser(frame, app, draft, area);
    };
    let template = Some(template);
    let preview = preview_lines(theme, &draft.preview);
    let open = draft.open_fields(app);
    let prompts = open.len() as u16;
    let prompt_rows = if prompts > 0 { prompts + 1 } else { 0 };
    // border + top padding, input, gap, prompts, chips, gap, destination,
    // preview inset, gap, footer, border
    let height = 2 + 3 + 1 + prompt_rows + 1 + 1 + 1 + preview.len() as u16 + 2 + 1 + 1 + 1;
    let popup = centered(area, area.width.saturating_sub(4).clamp(60, 84), height);
    frame.render_widget(Clear, popup);
    let title = template.map_or("Capture".to_string(), |template| {
        format!("Capture · {}", template.desc)
    });
    let block = theme.popup(title);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let mut constraints = vec![Constraint::Length(3), Constraint::Length(1)];
    if prompts > 0 {
        constraints.extend(open.iter().map(|_| Constraint::Length(1)));
        constraints.push(Constraint::Length(1));
    }
    constraints.extend([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(1),
    ]);
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    let cursor = |active: bool| {
        if active {
            Span::styled("▏", theme.fg(theme.accent))
        } else {
            Span::raw("")
        }
    };
    let input_line = if draft.input.is_empty() {
        Line::from(vec![
            Span::styled("› ", theme.strong(theme.accent)),
            cursor(draft.field == 0),
            Span::styled(placeholder(), theme.muted()),
        ])
    } else {
        Line::from(vec![
            Span::styled("› ", theme.strong(theme.accent)),
            Span::raw(draft.input.clone()),
            cursor(draft.field == 0),
        ])
    };
    frame.render_widget(
        Paragraph::new(input_line).block(theme.input(draft.field == 0)),
        parts[0],
    );
    let label_width = draft
        .prompts
        .iter()
        .map(|(prompt, _)| prompt.name.chars().count())
        .max()
        .unwrap_or(0)
        + 2;
    for (row, field) in open.iter().enumerate() {
        let (prompt, value) = &draft.prompts[field - 1];
        let focused = draft.field == *field;
        let mut spans = vec![
            Span::styled(if focused { "› " } else { "  " }, theme.fg(theme.accent)),
            Span::styled(
                format!("{:<label_width$}", prompt.name),
                if focused {
                    theme.strong(theme.text)
                } else {
                    theme.muted()
                },
            ),
            Span::raw(value.clone()),
            cursor(focused),
        ];
        if value.is_empty() && prompt.date {
            let token = match prompt.planning.as_deref() {
                Some("DEADLINE") => "fri · +2d · 2026-10-30, or due: above",
                Some("SCHEDULED") => "fri · +2d · 2026-10-30, or on: above",
                _ => "fri · +2d · 2026-10-30",
            };
            spans.push(Span::styled(token, theme.muted()));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), parts[2 + row]);
    }
    let next = 2 + if prompts > 0 { prompts as usize + 1 } else { 0 };
    frame.render_widget(
        Paragraph::new(Line::from(chips(theme, draft.parsed(app).ok(), app.today))),
        parts[next],
    );
    let destination = template.map_or(String::new(), destination);
    frame.render_widget(
        Paragraph::new(Span::styled(destination, theme.fg(theme.text))),
        parts[next + 2],
    );
    frame.render_widget(
        Paragraph::new(Line::from(theme.key_hint("tab", "change")))
            .alignment(ratatui::layout::Alignment::Right),
        parts[next + 2],
    );
    frame.render_widget(
        Paragraph::new(preview)
            .wrap(Wrap { trim: false })
            .block(theme.inset()),
        parts[next + 3],
    );
    frame.render_widget(
        Paragraph::new(Line::from(
            [
                theme.key_hint("⏎", "save"),
                theme.key_hint("ctrl-e", "editor"),
                theme.key_hint("esc", "cancel"),
            ]
            .concat(),
        )),
        parts[next + 5],
    );
}

/// The tokens a capture line takes after the task, with what each does.
const TOKENS: [(&str, &str); 4] = [
    ("due:fri", "deadline"),
    ("on:+2d", "scheduled"),
    ("+work", "tag"),
    ("#A", "priority"),
];

/// The empty field's placeholder: the task, then the tokens.
fn placeholder() -> String {
    let tokens: Vec<&str> = TOKENS.iter().map(|(token, _)| *token).collect();
    format!("type a task  {}", tokens.join("  "))
}

/// Before a template is chosen: every template with its key and where it
/// files, like org-capture's menu, and how a capture line works.
fn draw_chooser(frame: &mut Frame<'_>, app: &App, draft: &Draft, area: Rect) {
    let theme = &app.theme;
    let templates = &app.settings.captures;
    // border + top padding, templates, gap, two help lines, gap, footer, border
    let height = 2 + templates.len() as u16 + 1 + 2 + 1 + 1 + 1;
    let popup = centered(area, area.width.saturating_sub(4).clamp(60, 84), height);
    frame.render_widget(Clear, popup);
    let block = theme.popup("Capture · choose a template");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(templates.len() as u16),
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);
    let width = templates
        .iter()
        .map(|template| template.desc.chars().count())
        .max()
        .unwrap_or(0)
        + 2;
    let rows: Vec<Line<'_>> = templates
        .iter()
        .enumerate()
        .map(|(index, template)| {
            let focused = index == draft.selected;
            let line = Line::from(vec![
                Span::styled(if focused { "› " } else { "  " }, theme.fg(theme.accent)),
                Span::styled(format!("{:<3}", template.key), theme.strong(theme.accent)),
                Span::styled(
                    format!("{:<width$}", template.desc),
                    if focused {
                        theme.strong(theme.text)
                    } else {
                        theme.fg(theme.text)
                    },
                ),
                Span::styled(destination(template), theme.muted()),
            ]);
            if focused {
                line.style(theme.selected())
            } else {
                line
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(rows), parts[0]);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "Then type a task, with any of:",
                theme.muted(),
            )),
            Line::from(
                TOKENS
                    .iter()
                    .flat_map(|(token, meaning)| {
                        [
                            Span::styled(format!("  {token} "), theme.fg(theme.text)),
                            Span::styled(meaning.to_string(), theme.muted()),
                        ]
                    })
                    .collect::<Vec<_>>(),
            ),
        ]),
        parts[2],
    );
    frame.render_widget(
        Paragraph::new(Line::from(
            [
                theme.key_hint("key", "choose"),
                theme.key_hint("↑↓ ⏎", "pick"),
                theme.key_hint("esc", "cancel"),
            ]
            .concat(),
        )),
        parts[4],
    );
}

/// `→ inbox.org › Tasks`: the file name and heading a template files under.
fn destination(template: &org_core::CaptureTemplate) -> String {
    let file = template
        .file
        .as_deref()
        .and_then(|file| Path::new(file).file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let heading = template
        .heading
        .clone()
        .or_else(|| template.outline_path.as_ref().map(|path| path.join(" › ")))
        .unwrap_or_default();
    if heading.is_empty() {
        format!("→ {file}")
    } else {
        format!("→ {file} › {heading}")
    }
}

fn preview_lines(theme: &Theme, preview: &Result<String, String>) -> Vec<Line<'static>> {
    match preview {
        Ok(text) if text.is_empty() => vec![Line::from(Span::styled(
            "the exact org text appears here",
            theme.muted(),
        ))],
        Ok(text) => text.lines().map(|line| org_line(theme, line)).collect(),
        Err(error) => vec![Line::from(Span::styled(
            error.clone(),
            theme.fg(theme.slipped),
        ))],
    }
}

/// Parsed tokens shown as chips before anything is written. Dates carry
/// their month, and their year when it is not this one.
fn chips(theme: &Theme, draft: Option<CaptureDraft>, today: NaiveDate) -> Vec<Span<'static>> {
    let Some(draft) = draft else {
        return Vec::new();
    };
    let mut spans = Vec::new();
    let date = |value: &str| {
        value
            .parse::<NaiveDate>()
            .map(|date| {
                let format = if date.year() == today.year() {
                    "%a %-d %b"
                } else {
                    "%a %-d %b %Y"
                };
                date.format(format).to_string()
            })
            .unwrap_or_else(|_| value.to_string())
    };
    if let Some(deadline) = &draft.deadline {
        spans.extend(theme.pill(format!("deadline {}", date(&deadline.date)), theme.due));
        spans.push(Span::raw("  "));
    }
    if let Some(scheduled) = &draft.scheduled {
        spans.extend(theme.pill(format!("scheduled {}", date(&scheduled.date)), theme.accent));
        spans.push(Span::raw("  "));
    }
    for tag in &draft.tags {
        spans.extend(theme.pill(format!("+{tag}"), theme.muted));
        spans.push(Span::raw("  "));
    }
    if let Some(priority) = &draft.priority {
        spans.extend(theme.pill(format!("#{priority}"), theme.due));
    }
    spans
}

/// Light org syntax colouring for the preview: stars, keyword, priority,
/// tags, planning keywords and drawers.
fn org_line(theme: &Theme, line: &str) -> Line<'static> {
    if let Some(rest) = line.strip_prefix('*') {
        let stars = 1 + rest
            .chars()
            .take_while(|character| *character == '*')
            .count();
        let mut spans = vec![Span::styled("*".repeat(stars), theme.strong(theme.accent))];
        // Only the first word can be the TODO keyword; capitals later in
        // the title are ordinary text.
        let mut first = true;
        for word in line[stars..].split_inclusive(' ') {
            let trimmed = word.trim();
            let keyword = first
                && !trimmed.is_empty()
                && trimmed.chars().all(|c| c.is_ascii_uppercase())
                && trimmed.len() > 1;
            if !trimmed.is_empty() {
                first = false;
            }
            let style = if keyword {
                theme.strong(theme.due)
            } else if trimmed.starts_with("[#") {
                theme.fg(theme.due)
            } else if trimmed.starts_with(':') && trimmed.ends_with(':') && trimmed.len() > 1 {
                theme.fg(theme.accent)
            } else {
                theme.strong(theme.text)
            };
            spans.push(Span::styled(word.to_string(), style));
        }
        return Line::from(spans);
    }
    let trimmed = line.trim_start();
    if ["SCHEDULED:", "DEADLINE:", "CLOSED:"]
        .iter()
        .any(|keyword| trimmed.contains(keyword))
    {
        return Line::from(Span::styled(line.to_string(), theme.fg(theme.accent)));
    }
    if trimmed.starts_with(':') {
        return Line::from(Span::styled(line.to_string(), theme.muted()));
    }
    Line::from(line.to_string())
}
