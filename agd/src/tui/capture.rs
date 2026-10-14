use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Stdout, Write};
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
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
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

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
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            template: None,
            input: String::new(),
            prompts: Vec::new(),
            field: 0,
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
        KeyCode::Down if draft.field < draft.prompts.len() => draft.field += 1,
        KeyCode::Up => draft.field = draft.field.saturating_sub(1),
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
                if draft.field < draft.prompts.len() {
                    draft.field += 1;
                } else {
                    return commit(app, draft);
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
    let height = 14 + draft.prompts.len() as u16;
    let popup = centered(area, 78, height);
    frame.render_widget(Clear, popup);
    let template = draft
        .template
        .as_ref()
        .and_then(|key| app.settings.capture_template(key));
    let title = template.map_or("Capture".to_string(), |template| {
        format!("Capture · {}", template.desc)
    });
    let block = theme.panel(title).border_style(theme.fg(theme.accent));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let mut constraints = vec![Constraint::Length(3)];
    constraints.extend(draft.prompts.iter().map(|_| Constraint::Length(1)));
    constraints.extend([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(3),
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
    let input_line = if draft.template.is_none() {
        Line::from(Span::styled("type a template key", theme.muted()))
    } else {
        Line::from(vec![
            Span::styled("› ", theme.strong(theme.accent)),
            Span::raw(draft.input.clone()),
            cursor(draft.field == 0),
        ])
    };
    frame.render_widget(
        Paragraph::new(input_line).block(
            Block::bordered()
                .border_type(ratatui::widgets::BorderType::Rounded)
                .border_style(theme.fg(if draft.field == 0 {
                    theme.accent
                } else {
                    theme.border
                })),
        ),
        parts[0],
    );
    for (index, (prompt, value)) in draft.prompts.iter().enumerate() {
        let name = &prompt.name;
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!("  {name}: "), theme.muted()),
                Span::raw(value.clone()),
                cursor(draft.field == index + 1),
            ])),
            parts[1 + index],
        );
    }
    let next = 1 + draft.prompts.len();
    frame.render_widget(
        Paragraph::new(Line::from(chips(theme, draft.parsed(app).ok()))),
        parts[next],
    );
    let destination = template.map_or(String::new(), |template| {
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
        format!("→ {file} › {heading}")
    });
    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            destination,
            theme.fg(theme.text),
        )])),
        parts[next + 1],
    );
    frame.render_widget(
        Paragraph::new(Line::from([theme.key_hint("tab", "change")].concat()))
            .alignment(ratatui::layout::Alignment::Right),
        parts[next + 1],
    );
    let preview = match &draft.preview {
        Ok(text) if text.is_empty() => vec![Line::from(Span::styled(
            "the exact org text appears here",
            theme.muted(),
        ))],
        Ok(text) => text.lines().map(|line| org_line(theme, line)).collect(),
        Err(error) => vec![Line::from(Span::styled(
            error.clone(),
            theme.fg(theme.slipped),
        ))],
    };
    let preview_style = if theme.plain {
        Style::default()
    } else {
        Style::default().bg(theme.surface)
    };
    frame.render_widget(
        Paragraph::new(preview)
            .style(preview_style)
            .wrap(Wrap { trim: false })
            .block(Block::default().padding(ratatui::widgets::Padding::horizontal(1))),
        parts[next + 2],
    );
    let mut footer = [
        theme.key_hint("⏎", "save"),
        theme.key_hint("ctrl-e", "editor"),
        theme.key_hint("esc", "cancel"),
    ]
    .concat();
    let templates: Vec<String> = app
        .settings
        .captures
        .iter()
        .map(|template| format!("{} {}", template.key, template.desc.to_lowercase()))
        .collect();
    footer.push(Span::styled(templates.join(" · "), theme.muted()));
    frame.render_widget(Paragraph::new(Line::from(footer)), parts[next + 3]);
}

/// Parsed tokens shown as chips before anything is written.
fn chips(theme: &Theme, draft: Option<CaptureDraft>) -> Vec<Span<'static>> {
    let Some(draft) = draft else {
        return Vec::new();
    };
    let mut spans = Vec::new();
    let date = |value: &str| {
        value
            .parse::<chrono::NaiveDate>()
            .map(|date| date.format("%a %-d").to_string())
            .unwrap_or_else(|_| value.to_string())
    };
    if let Some(deadline) = &draft.deadline {
        spans.push(theme.pill(format!("deadline {}", date(&deadline.date)), theme.due));
        spans.push(Span::raw(" "));
    }
    if let Some(scheduled) = &draft.scheduled {
        spans.push(theme.pill(format!("scheduled {}", date(&scheduled.date)), theme.accent));
        spans.push(Span::raw(" "));
    }
    for tag in &draft.tags {
        spans.push(theme.pill(format!("+{tag}"), theme.muted));
        spans.push(Span::raw(" "));
    }
    if let Some(priority) = &draft.priority {
        spans.push(theme.pill(format!("#{priority}"), theme.due));
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
        for word in line[stars..].split_inclusive(' ') {
            let trimmed = word.trim();
            let style = if trimmed.chars().all(|c| c.is_ascii_uppercase()) && trimmed.len() > 1 {
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
