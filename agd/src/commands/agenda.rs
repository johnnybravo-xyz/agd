use std::path::Path;

use agd_core::{
    AgendaEngine, AgendaEntry, AgendaQuery, AgendaSpan, JournalStatus, Settings, ViewSection,
    WorkspaceIndex, WriteEngine, agenda_csv, agenda_row, agenda_text, agenda_title, item_row,
    items_csv, keyword_colors, load_settings, parse_date_input,
};
use anyhow::{Context, Result, anyhow, bail};
use chrono::{Local, NaiveDate};
use clap::{Args, ValueEnum};

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum SpanArg {
    Day,
    Week,
    Month,
}

#[derive(Args, Debug)]
pub struct AgendaCommand {
    /// Days to show; defaults to agenda.span from the configuration
    #[arg(long, value_enum)]
    span: Option<SpanArg>,
    /// First day: a date (2026-10-13) or org relative syntax (fri, +2d, -1w)
    #[arg(long, allow_hyphen_values = true)]
    from: Option<String>,
    /// Emacs tags/todo match, e.g. 'work-someday/NEXT|TODO'
    #[arg(long = "match")]
    match_expression: Option<String>,
    /// Run a saved view, like org-agenda's dispatcher keys (n, or a [[view]]
    /// key); without a key, the configured start_view
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "",
        value_name = "KEY",
        conflicts_with_all = ["span", "match_expression", "brief", "profile"]
    )]
    view: Option<String>,
    /// Use the files and match of a configured agents.profiles entry
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
    pub fn execute(self, config: Option<String>, root: Option<String>) -> Result<()> {
        let mut settings = load_settings(
            config.as_deref().map(Path::new),
            root.as_deref().map(Path::new),
        )?
        .settings;
        if self.views {
            return print_views(&settings, self.json);
        }
        let mut match_expression = self.match_expression;
        if let Some(name) = self.profile.as_deref() {
            let profile = settings
                .agent_profile(name)
                .cloned()
                .ok_or_else(|| anyhow!("unknown agent agenda profile: {name}"))?;
            if !profile.files.is_empty() {
                settings.agenda_files = profile.files;
            }
            match_expression = match_expression.or(profile.match_expression);
        }
        let index = load_index(settings.clone())?;
        let today = Local::now().date_naive();
        if self.status {
            println!("{}", status_line(&index, &settings, today)?);
            return Ok(());
        }
        let start = match self.from {
            Some(value) => parse_day(&value, today)?,
            None => default_start(&settings, today)?,
        };
        if let Some(key) = self.view.as_deref() {
            let key = if key.is_empty() {
                if settings.start_view.is_empty() {
                    bail!("--view without a key runs start_view, which is not set");
                }
                settings.start_view.as_str()
            } else {
                key
            };
            let view = settings.view(key).ok_or_else(|| {
                let keys: Vec<&str> = settings
                    .views
                    .iter()
                    .map(|view| view.key.as_str())
                    .collect();
                anyhow!(
                    "unknown view '{key}'; configured views: {}",
                    keys.join(", ")
                )
            })?;
            let sections = AgendaEngine::new(&index).view(view, start, today, self.log)?;
            if self.json {
                println!("{}", serde_json::to_string_pretty(&sections)?);
            } else if self.csv {
                for section in &sections {
                    print!(
                        "{}{}",
                        agenda_csv(&section.entries, today, &settings),
                        items_csv(&section.items, &section.kind, &settings)
                    );
                    for group in &section.groups {
                        print!(
                            "{}{}",
                            agenda_csv(&group.entries, today, &settings),
                            items_csv(&group.items, &section.kind, &settings)
                        );
                    }
                }
            } else {
                print!("{}", render_view(&sections));
            }
            return Ok(());
        }
        let span = match self.span {
            Some(SpanArg::Day) => AgendaSpan::Day,
            Some(SpanArg::Week) => AgendaSpan::Week,
            Some(SpanArg::Month) => AgendaSpan::Month,
            None => span_from_settings(&settings),
        };
        let (from, to) = span.bounds(start, &settings.week_start);
        let entries = AgendaEngine::new(&index).agenda(&AgendaQuery {
            from,
            to,
            today,
            match_expression,
            log_mode: self.log,
        })?;
        if self.json {
            println!("{}", serde_json::to_string_pretty(&entries)?);
        } else if self.csv {
            print!("{}", agenda_csv(&entries, today, &settings));
        } else if self.brief {
            for entry in &entries {
                println!("{} {}", entry.occurrence.date, agenda_title(&entry.item));
            }
        } else {
            print!("{}", agenda_text(&entries, from, to, None));
        }
        Ok(())
    }
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

/// The saved views in config order, with start_view moved first. The JSON
/// carries the TUI's display settings, so a client such as the Claude Code
/// pane draws them the same way.
fn print_views(settings: &Settings, json: bool) -> Result<()> {
    let mut views: Vec<_> = settings
        .views
        .iter()
        .map(|view| {
            (
                view.key.as_str(),
                view.desc.as_str(),
                view.key == settings.start_view,
            )
        })
        .collect();
    views.sort_by_key(|(_, _, start)| !start);
    if json {
        let rows: Vec<_> = views
            .iter()
            .map(|(key, desc, start)| serde_json::json!({ "key": key, "desc": desc, "start": start }))
            .collect();
        let index = load_index(settings.clone())?;
        let payload = serde_json::json!({
            "views": rows,
            "theme": settings.theme,
            "nerd_font": settings.nerd_font,
            "keyword_colors": keyword_colors(&index),
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        for (key, desc, start) in views {
            println!("{key:<4}{desc}{}", if start { "  (start)" } else { "" });
        }
    }
    Ok(())
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

fn parse_day(value: &str, today: NaiveDate) -> Result<NaiveDate> {
    Ok(parse_date_input(value, today)?.date.parse::<NaiveDate>()?)
}

fn default_start(settings: &Settings, today: NaiveDate) -> Result<NaiveDate> {
    let Some(value) = settings.agenda_start_day.as_deref() else {
        return Ok(today);
    };
    parse_day(value, today).with_context(|| format!("invalid agenda start_day: {value}"))
}

fn span_from_settings(settings: &Settings) -> AgendaSpan {
    match settings.agenda_span.as_str() {
        "day" => AgendaSpan::Day,
        "month" => AgendaSpan::Month,
        _ => AgendaSpan::Week,
    }
}
