//! The capture popup (`c`, and `agd capture -i`): pick a template as in
//! org-capture's menu, type one line with tokens (`due:fri +tag #A`) that
//! show as chips, answer the template's prompts, and see the exact Org text
//! a dry run would write before ⏎ writes it.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Stdout, Write};
use std::path::Path;
use std::process::Command;

use agd_core::{
    CaptureDraft, CapturePrompt, WriteEngine, capture_prompts, keyword_colors, parse_capture_line,
};
use anyhow::{Context, Result};
use chrono::NaiveDate;
use crossterm::event::{EnableMouseCapture, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{EnterAlternateScreen, enable_raw_mode};
use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, ListState, Paragraph, Wrap};

use super::input::TextInput;
use super::keys::{self, Action};
use super::theme::{Theme, centered};
use super::{App, Overlay, stop_terminal, write_context};

/// The capture popup's state. Field 0 is the one-line title with tokens;
/// later fields answer the template's `%^{prompt}`s.
#[derive(Debug)]
pub(super) struct Draft {
    template: Option<String>,
    input: TextInput,
    prompts: Vec<(CapturePrompt, TextInput)>,
    field: usize,
    body: String,
    preview: Result<String, String>,
    /// The preview is out of date; it is rebuilt once before the next draw.
    stale: bool,
    /// The highlighted row of the template list, before one is chosen.
    selected: usize,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            template: None,
            input: TextInput::default(),
            prompts: Vec::new(),
            field: 0,
            selected: 0,
            body: String::new(),
            preview: Ok(String::new()),
            stale: false,
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
            .map(|prompt| (prompt, TextInput::default()))
            .collect();
        self.field = 0;
    }

    fn current(&mut self) -> &mut TextInput {
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
        parse_capture_line(self.input.text(), app.today).map_err(|error| error.to_string())
    }

    /// The exact text that will be written, from a dry-run capture through
    /// the real template and write path.
    fn refresh(&mut self, app: &App) {
        let Some(template) = &self.template else {
            self.preview = Ok(String::new());
            return;
        };
        if self.input.text().trim().is_empty() {
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
                .map_err(|error| app.error_text(&error))
        });
    }
}

/// Rebuilds a stale preview with one dry run, so keys that arrive together
/// (a paste, fast typing) cost one dry run and not one each.
pub(super) fn refresh_stale(app: &mut App) {
    // Every other overlay goes back untouched.
    app.overlay = match std::mem::replace(&mut app.overlay, Overlay::None) {
        Overlay::Capture(mut draft) if draft.stale => {
            draft.refresh(app);
            draft.stale = false;
            Overlay::Capture(draft)
        }
        other => other,
    };
}

fn prompt_answers(prompts: &[(CapturePrompt, TextInput)]) -> BTreeMap<String, String> {
    prompts
        .iter()
        .map(|(prompt, value)| (prompt.name.clone(), value.text().to_string()))
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
        draft
            .current()
            .handle(KeyEvent::from(KeyCode::Char(character)));
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
    // In the text fields C-e is end of line, so the body editor is C-o.
    let action = keys::popup_action(key);
    match key.code {
        _ if action == Some(Action::Back) => return close(app),
        _ if action == Some(Action::Editor) => edit_body(terminal, app, &mut draft)?,
        _ if action == Some(Action::NextField) => {
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
        // Before a template is chosen, the picker keys and ⏎ pick one from
        // the list; after, the picker keys move between fields.
        _ if keys::picker_step(key).is_some() && draft.template.is_none() => {
            let count = app.settings.captures.len();
            draft.selected = keys::stepped(draft.selected, keys::picker_step(key), count);
        }
        _ if keys::picker_step(key) == Some(1) => {
            if let Some(field) = draft.next_field(app) {
                draft.field = field;
            }
        }
        _ if keys::picker_step(key) == Some(-1) => draft.field = draft.previous_field(app),
        _ if action == Some(Action::Save) && draft.template.is_none() => {
            if let Some(template) = app.settings.captures.get(draft.selected) {
                let key = template.key.clone();
                draft.choose(app, &key);
            }
        }
        // Backspace on an empty line goes back to the template list.
        KeyCode::Backspace if draft.field == 0 && draft.input.text().is_empty() => {
            draft.template = None;
            draft.prompts.clear();
        }
        _ if action == Some(Action::Save) => {
            if !draft.input.text().trim().is_empty() {
                match draft.next_field(app) {
                    Some(field) => draft.field = field,
                    None => return commit(app, draft),
                }
            }
        }
        // Before a template is chosen, a letter picks one, like C-c c t in
        // Emacs.
        KeyCode::Char(character) if draft.template.is_none() && !control => {
            draft.choose(app, &character.to_string());
            if draft.template.is_none() {
                app.set_status(format!("no capture template '{character}'"));
            }
        }
        _ if draft.template.is_none() => {}
        _ => {
            draft.current().handle(key);
        }
    }
    draft.stale = true;
    app.overlay = Overlay::Capture(draft);
    Ok(())
}

/// Leaves the popup without writing; `agd capture -i` then quits.
fn close(app: &mut App) -> Result<()> {
    if app.capture_only {
        app.quit = true;
    }
    Ok(())
}

fn commit(app: &mut App, draft: Draft) -> Result<()> {
    let template = draft.template.clone().unwrap_or_default();
    let parsed = parse_capture_line(draft.input.text(), app.today)?;
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
    let location = app.short_location(&mutation.item);
    app.remember_undo(
        &mutation.journal_id,
        &location,
        format!("captured · {}", mutation.item.title),
    );
    app.set_done(format!(
        "{} · u undo",
        crate::words::captured(&mutation.item, &location, app.today)
    ));
    if app.capture_only {
        app.quit = true;
        return Ok(());
    }
    app.reload_rows()
}

/// C-o: the body goes through $EDITOR as a temporary file.
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
    let preview = preview_lines(theme, &keyword_colors(&app.index), &draft.preview);
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
    let mut input_spans = vec![Span::styled("› ", theme.strong(theme.accent))];
    if draft.input.text().is_empty() {
        input_spans.push(cursor(draft.field == 0));
        input_spans.push(Span::styled(placeholder(), theme.muted()));
    } else {
        input_spans.extend(draft.input.spans(theme, draft.field == 0));
    }
    let input_line = Line::from(input_spans);
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
            Span::styled(if focused { "▌ " } else { "  " }, theme.fg(theme.accent)),
            Span::styled(
                format!("{:<label_width$}", prompt.name),
                if focused {
                    theme.strong(theme.text)
                } else {
                    theme.muted()
                },
            ),
        ];
        spans.extend(value.spans(theme, focused));
        if value.text().is_empty() && prompt.date {
            let examples = format!("fri · +2d · {}", super::actions::example_date(app.today));
            let token = match prompt.planning.as_deref() {
                Some("DEADLINE") => format!("{examples}, or due: above"),
                Some("SCHEDULED") => format!("{examples}, or on: above"),
                _ => examples,
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
    let key = |action| keys::key_label(keys::POPUP_KEYS, action);
    frame.render_widget(
        Paragraph::new(Line::from(theme.key_hint(key(Action::NextField), "change")))
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
                theme.key_hint(key(Action::Save), "save"),
                theme.key_hint(key(Action::Editor), "body in $EDITOR"),
                theme.key_hint(key(Action::Back), "cancel"),
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
    // As the Views popup: the key, what it is, where it files, and the
    // selection bar across the whole row.
    let rows: Vec<ListItem<'_>> = templates
        .iter()
        .map(|template| {
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<3}", template.key), theme.strong(theme.accent)),
                Span::styled(format!("{:<width$}", template.desc), theme.fg(theme.text)),
                Span::styled(destination(template), theme.muted()),
            ]))
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(draft.selected));
    frame.render_stateful_widget(
        List::new(rows)
            .highlight_symbol(Span::styled("▌", theme.fg(theme.accent)))
            .highlight_spacing(ratatui::widgets::HighlightSpacing::Always)
            .highlight_style(theme.selected()),
        parts[0],
        &mut state,
    );
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
                theme.key_hint(keys::key_label(keys::POPUP_KEYS, Action::Back), "cancel"),
            ]
            .concat(),
        )),
        parts[4],
    );
}

/// `→ inbox.org › Tasks`: the file name and heading a template files under.
fn destination(template: &agd_core::CaptureTemplate) -> String {
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

fn preview_lines(
    theme: &Theme,
    colors: &BTreeMap<String, String>,
    preview: &Result<String, String>,
) -> Vec<Line<'static>> {
    match preview {
        Ok(text) if text.is_empty() => vec![Line::from(Span::styled(
            "the exact org text appears here",
            theme.muted(),
        ))],
        Ok(text) => text
            .lines()
            .map(|line| org_line(theme, colors, line))
            .collect(),
        Err(error) => vec![Line::from(Span::styled(
            error.clone(),
            theme.fg(theme.overdue),
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
    let date = |value: &str| crate::words::when(value, None, today);
    if let Some(deadline) = &draft.deadline {
        spans.extend(theme.pill(format!("deadline {}", date(&deadline.date)), theme.due));
        spans.push(Span::raw("  "));
    }
    if let Some(scheduled) = &draft.scheduled {
        spans.extend(theme.pill(format!("scheduled {}", date(&scheduled.date)), theme.accent));
        spans.push(Span::raw("  "));
    }
    // `+work` is typed; the chip shows the tag as every list does.
    for tag in &draft.tags {
        spans.extend(theme.pill(crate::words::tag(tag), theme.muted));
        spans.push(Span::raw("  "));
    }
    // `#A` is typed; the chip is the priority pill every row shows.
    if let Some(priority) = &draft.priority {
        spans.extend(theme.pill(priority.clone(), theme.due));
    }
    spans
}

/// Org syntax coloured as the chips and rows are: stars and tags muted,
/// the keyword in its configured colour, priority amber, and each planning
/// keyword in its role (DEADLINE due, SCHEDULED accent, CLOSED muted).
fn org_line(theme: &Theme, colors: &BTreeMap<String, String>, line: &str) -> Line<'static> {
    if let Some(rest) = line.strip_prefix('*') {
        let stars = 1 + rest
            .chars()
            .take_while(|character| *character == '*')
            .count();
        let mut spans = vec![Span::styled("*".repeat(stars), theme.muted())];
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
                // A keyword no agenda file names yet reads as a TODO.
                let name = colors.get(trimmed).map_or("orange", String::as_str);
                if name == "orange" {
                    theme.fg(theme.named(name))
                } else {
                    theme.strong(theme.named(name))
                }
            } else if trimmed.starts_with("[#") {
                theme.strong(theme.due)
            } else if trimmed.starts_with(':') && trimmed.ends_with(':') && trimmed.len() > 1 {
                theme.muted()
            } else {
                theme.strong(theme.text)
            };
            spans.push(Span::styled(word.to_string(), style));
        }
        return Line::from(spans);
    }
    let trimmed = line.trim_start();
    if PLANNING
        .iter()
        .any(|(keyword, _)| trimmed.starts_with(keyword))
    {
        return planning_line(theme, line);
    }
    if trimmed.starts_with(':') {
        return Line::from(Span::styled(line.to_string(), theme.muted()));
    }
    Line::from(line.to_string())
}

/// Org's planning keywords and the role each is drawn in.
const PLANNING: [(&str, Role); 3] = [
    ("DEADLINE:", Role::Due),
    ("SCHEDULED:", Role::Accent),
    ("CLOSED:", Role::Muted),
];

#[derive(Clone, Copy)]
enum Role {
    Due,
    Accent,
    Muted,
}

/// `SCHEDULED: <…> DEADLINE: <…>`: each keyword and its timestamp in its
/// own role.
fn planning_line(theme: &Theme, line: &str) -> Line<'static> {
    let style = |role: Role| match role {
        Role::Due => theme.fg(theme.due),
        Role::Accent => theme.fg(theme.accent),
        Role::Muted => theme.muted(),
    };
    let mut spans = Vec::new();
    let mut rest = line;
    let mut role = Role::Muted;
    while !rest.is_empty() {
        // The next planning keyword after the start splits off a part.
        let next = PLANNING
            .iter()
            .filter_map(|(keyword, keyword_role)| {
                rest.get(1..)?
                    .find(keyword)
                    .map(|at| (at + 1, *keyword_role))
            })
            .min_by_key(|(at, _)| *at);
        if let Some((_, keyword_role)) = PLANNING
            .iter()
            .find(|(keyword, _)| rest.trim_start().starts_with(keyword))
        {
            role = *keyword_role;
        }
        let end = next.map_or(rest.len(), |(at, _)| at);
        spans.push(Span::styled(rest[..end].to_string(), style(role)));
        rest = &rest[end..];
    }
    Line::from(spans)
}
