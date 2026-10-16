use std::path::Path;

use agd_core::{
    AgdError, AgendaEngine, AgendaEntry, AgendaQuery, AgendaSpan, JournalStatus, Settings,
    VIEWS_SCHEMA, ViewSection, ViewSettings, WorkspaceIndex, WriteEngine, agenda_csv, agenda_row,
    agenda_text, agenda_title, item_row, items_csv, keyword_colors, load_settings,
    parse_agenda_day,
};
use anyhow::Result;

use super::write_stdout;
use chrono::{Local, NaiveDate};
use clap::Args;

#[derive(Args, Debug)]
pub struct AgendaCommand {
    /// Days to show: day, week, month or a number of days; defaults to
    /// agenda_span from config.toml
    #[arg(long)]
    span: Option<AgendaSpan>,
    /// First day: a date (2026-10-13) or Org date-prompt syntax (fri, +2d,
    /// -1w, oct 30)
    #[arg(long, allow_hyphen_values = true)]
    from: Option<String>,
    /// Filter the dated agenda with an Emacs tags/todo match, e.g.
    /// 'work-someday/NEXT|TODO'. For a tags search like C-c a m, run a
    /// `tags` or `tags-todo` view
    #[arg(long = "match")]
    match_expression: Option<String>,
    /// Run a saved view by its dispatcher key, as org-agenda does: a [[view]]
    /// key, or Org's built-in n, which is always available. Without a key,
    /// the configured start_view
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "",
        value_name = "KEY",
        conflicts_with_all = ["span", "match_expression", "brief", "profile"]
    )]
    view: Option<String>,
    /// Use the files and match of a configured [[agent_profile]]
    #[arg(long)]
    profile: Option<String>,
    /// Also list CLOSED entries, like org-agenda log mode (l)
    #[arg(long)]
    log: bool,
    /// One `date title` line per entry, for hooks
    #[arg(long)]
    brief: bool,
    /// One line of counts for a tmux or herdr status bar
    #[arg(
        long,
        conflicts_with_all = [
            "json", "csv", "brief", "view", "span", "from", "match_expression", "log", "profile",
        ]
    )]
    status: bool,
    /// The saved views, start_view first; with --json, also what a client
    /// draws them with: theme, nerd_font and each TODO keyword's color
    #[arg(
        long,
        conflicts_with_all = [
            "csv", "brief", "view", "status", "span", "from", "match_expression", "log", "profile",
        ]
    )]
    views: bool,
    /// agd.agenda-entry/v1 objects
    #[arg(long, conflicts_with_all = ["csv", "brief"])]
    json: bool,
    /// Emacs org-batch-agenda-csv rows: the same eleven columns and values
    #[arg(long, conflicts_with = "brief")]
    csv: bool,
}

impl AgendaCommand {
    /// True when the output, and so any error, is JSON.
    pub fn json(&self) -> bool {
        self.json
    }

    pub fn execute(self, config: Option<String>, root: Option<String>) -> Result<()> {
        let mut settings = load_settings(
            config.as_deref().map(Path::new),
            root.as_deref().map(Path::new),
        )?
        .settings;
        if self.views {
            return Ok(write_stdout(&views_output(&settings, self.json)?)?);
        }
        let match_expression = match self.profile.as_deref() {
            Some(name) => apply_profile(&mut settings, name, self.match_expression)?,
            None => self.match_expression,
        };
        let index = load_index(settings.clone())?;
        let today = Local::now().date_naive();
        if self.status {
            let line = status_line(&index, &settings, today)?;
            return Ok(write_stdout(&format!("{line}\n"))?);
        }
        let start = match self.from {
            Some(value) => parse_agenda_day(&value, today)?,
            None => settings.agenda_start(today)?,
        };
        if let Some(key) = self.view.as_deref() {
            let view = find_view(&settings, key)?;
            let sections = AgendaEngine::new(&index).view(view, start, today, self.log)?;
            let output = view_output(&sections, self.json, self.csv, today, &settings)?;
            return Ok(write_stdout(&output)?);
        }
        let span = self.span.unwrap_or(settings.agenda_span);
        let (from, to) = span.bounds(start, settings.week_start);
        let entries = AgendaEngine::new(&index).agenda(&AgendaQuery {
            from,
            to,
            today,
            match_expression,
            log_mode: self.log,
        })?;
        let output = if self.json {
            format!("{}\n", serde_json::to_string_pretty(&entries)?)
        } else if self.csv {
            agenda_csv(&entries, today, &settings)
        } else if self.brief {
            entries
                .iter()
                .map(|entry| format!("{} {}\n", entry.occurrence.date, agenda_title(&entry.item)))
                .collect()
        } else {
            agenda_text(&entries, from, to, None)
        };
        Ok(write_stdout(&output)?)
    }
}

/// Applies a `[[agent_profile]]`: its files replace agenda_files, and its
/// match is the match unless --match gives one.
fn apply_profile(
    settings: &mut Settings,
    name: &str,
    match_expression: Option<String>,
) -> Result<Option<String>, AgdError> {
    let profile = settings.agent_profile(name).cloned().ok_or_else(|| {
        let names: Vec<&str> = settings
            .agent_profiles
            .iter()
            .map(|profile| profile.name.as_str())
            .collect();
        AgdError::new(
            "unknown_profile",
            format!(
                "unknown agent profile '{name}'; profiles: {}",
                names.join(", ")
            ),
        )
    })?;
    if !profile.files.is_empty() {
        settings.agenda_files = profile.files;
    }
    Ok(match_expression.or(profile.match_expression))
}

/// The view `--view KEY` names; an empty key is the configured start_view.
fn find_view<'a>(settings: &'a Settings, key: &str) -> Result<&'a ViewSettings, AgdError> {
    let key = if key.is_empty() {
        if settings.start_view.is_empty() {
            return Err(AgdError::new(
                "no_start_view",
                "--view without a key runs start_view, which is not set",
            ));
        }
        settings.start_view.as_str()
    } else {
        key
    };
    settings.view(key).ok_or_else(|| {
        let keys: Vec<&str> = settings
            .all_views()
            .iter()
            .map(|view| view.key.as_str())
            .collect();
        AgdError::new(
            "unknown_view",
            format!("unknown view '{key}'; views: {}", keys.join(", ")),
        )
    })
}

/// A view's output: its sections as JSON, their CSV rows, or the text.
fn view_output(
    sections: &[ViewSection],
    json: bool,
    csv: bool,
    today: NaiveDate,
    settings: &Settings,
) -> Result<String> {
    if json {
        return Ok(format!("{}\n", serde_json::to_string_pretty(sections)?));
    }
    if !csv {
        return Ok(render_view(sections));
    }
    let mut rows = String::new();
    for section in sections {
        rows.push_str(&agenda_csv(&section.entries, today, settings));
        rows.push_str(&items_csv(&section.items, &section.kind, settings));
        for group in &section.groups {
            rows.push_str(&agenda_csv(&group.entries, today, settings));
            rows.push_str(&items_csv(&group.items, &section.kind, settings));
        }
    }
    Ok(rows)
}

/// Blocks separated by a rule of `=`, as in an Emacs block agenda. An agenda
/// block's header replaces its `Week-agenda (W42):` line, as
/// org-agenda-overriding-header does.
fn render_view(sections: &[ViewSection]) -> String {
    let mut blocks = Vec::new();
    for section in sections {
        let mut block = String::new();
        if !section.groups.is_empty() {
            // org-super-agenda: the block's groups replace its days or list,
            // each under its own indented header.
            if !section.header.is_empty() {
                block.push_str(&format!("{}\n", section.header));
            }
            for group in &section.groups {
                block.push_str(&format!(" {}\n", group.name));
                for entry in &group.entries {
                    block.push_str(&agenda_row(entry));
                    block.push('\n');
                }
                for item in &group.items {
                    block.push_str(&item_row(item));
                    block.push('\n');
                }
            }
        } else if let (Some(from), Some(to)) = (section.from, section.to) {
            let header = (!section.header.is_empty()).then_some(section.header.as_str());
            block.push_str(&agenda_text(&section.entries, from, to, header));
        } else {
            block.push_str(&format!("{}\n", section.header));
            for item in &section.items {
                block.push_str(&item_row(item));
                block.push('\n');
            }
        }
        blocks.push(block);
    }
    blocks.join(&format!("{}\n", "=".repeat(78)))
}

/// The saved views in config order, then the built-in ones, with
/// start_view moved first. The JSON carries the TUI's display settings, so a
/// client such as the Claude Code pane draws them the same way.
fn views_output(settings: &Settings, json: bool) -> Result<String> {
    let mut views = settings.all_views();
    views.sort_by_key(|view| view.key != settings.start_view);
    if json {
        let rows: Vec<_> = views
            .iter()
            .map(|view| {
                serde_json::json!({
                    "key": view.key,
                    "desc": view.desc,
                    "start": view.key == settings.start_view,
                    "builtin": settings.is_builtin_view(&view.key),
                })
            })
            .collect();
        let index = load_index(settings.clone())?;
        let payload = serde_json::json!({
            "schema": VIEWS_SCHEMA,
            "views": rows,
            "theme": settings.theme,
            "nerd_font": settings.nerd_font,
            "keyword_colors": keyword_colors(&index),
        });
        return Ok(format!("{}\n", serde_json::to_string_pretty(&payload)?));
    }
    Ok(views
        .iter()
        .map(|view| {
            let start = if view.key == settings.start_view {
                "  (start)"
            } else {
                ""
            };
            let builtin = if settings.is_builtin_view(&view.key) {
                "  (built-in)"
            } else {
                ""
            };
            format!("{:<4}{}{start}{builtin}\n", view.key, view.desc)
        })
        .collect())
}

/// `3 overdue · 7 today · 4 inbox · 2 agents`, zero counts left out. Today
/// counts what is scheduled, due or timestamped today; a deadline warning
/// (`In 4 d.:`) is neither today nor overdue.
fn status_line(index: &WorkspaceIndex, settings: &Settings, today: NaiveDate) -> Result<String> {
    let engine = AgendaEngine::new(index);
    let entries = engine.agenda(&AgendaQuery {
        from: today,
        to: today,
        today,
        match_expression: None,
        log_mode: false,
    })?;
    let overdue_kind = |entry: &AgendaEntry| {
        entry.occurrence.kind == agd_core::OccurrenceType::PastScheduled
            || entry.occurrence.extra.ends_with("ago:")
    };
    let mut overdue = std::collections::BTreeSet::new();
    let mut open = std::collections::BTreeSet::new();
    let counted = entries.iter().filter(|entry| {
        !entry.item.done && entry.occurrence.kind != agd_core::OccurrenceType::UpcomingDeadline
    });
    for entry in counted {
        if overdue_kind(entry) {
            overdue.insert(&entry.item.item_ref);
        } else {
            open.insert(&entry.item.item_ref);
        }
    }
    let agents = WriteEngine::with_default_journal(settings.clone())?
        .journal()?
        .iter()
        .filter(|record| {
            record.actor.starts_with("mcp:") && record.status == JournalStatus::Committed
        })
        .count();
    let counts = [
        (overdue.len(), "overdue"),
        (open.len(), "today"),
        (engine.inbox().len(), "inbox"),
        (agents, "agents"),
    ];
    Ok(counts
        .iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, label)| format!("{count} {label}"))
        .collect::<Vec<_>>()
        .join(" · "))
}

// Reads keep working when one agenda file cannot be indexed; say which file
// was left out on stderr so stdout stays machine-readable.
pub fn load_index(settings: Settings) -> Result<WorkspaceIndex> {
    let index = WorkspaceIndex::load(settings)?;
    for (_, reason) in index.skipped_files() {
        eprintln!("warning: skipped {reason}");
    }
    Ok(index)
}
