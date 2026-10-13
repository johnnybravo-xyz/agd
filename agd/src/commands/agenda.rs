use std::path::Path;

use anyhow::{Context, Result, anyhow};
use chrono::{Datelike, Duration, Local, NaiveDate};
use clap::{Args, ValueEnum};
use org_core::{
    AgendaEngine, AgendaEntry, AgendaQuery, AgendaSpan, JournalStatus, Settings, ViewSection,
    WorkspaceIndex, WriteEngine, agenda_csv, agenda_title, items_csv, load_settings,
    parse_date_input,
};

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
    #[arg(long)]
    from: Option<String>,
    /// Emacs tags/todo match, e.g. 'work-someday/NEXT|TODO'
    #[arg(long = "match")]
    match_expression: Option<String>,
    /// Run a saved view, like org-agenda's dispatcher keys (n, or [views.KEY])
    #[arg(long, conflicts_with_all = ["span", "match_expression"])]
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
    #[arg(long, conflicts_with_all = ["json", "csv", "brief", "view"])]
    status: bool,
    /// agd.agenda-entry/v1 objects
    #[arg(long, conflicts_with_all = ["csv", "brief"])]
    json: bool,
    /// The same columns as Emacs org-batch-agenda-csv
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
        let mut match_expression = self.match_expression;
        if let Some(name) = self.profile.as_deref() {
            let profile = settings
                .agents
                .profiles
                .get(name)
                .cloned()
                .ok_or_else(|| anyhow!("unknown agent agenda profile: {name}"))?;
            if !profile.files.is_empty() {
                settings.files.agenda = profile.files;
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
            let view = settings.views.get(key).ok_or_else(|| {
                let keys: Vec<&str> = settings.views.keys().map(String::as_str).collect();
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
                        agenda_csv(&section.entries),
                        items_csv(&section.items)
                    );
                }
            } else {
                print!("{}", render_view(&sections, &settings));
            }
            return Ok(());
        }
        let span = match self.span {
            Some(SpanArg::Day) => AgendaSpan::Day,
            Some(SpanArg::Week) => AgendaSpan::Week,
            Some(SpanArg::Month) => AgendaSpan::Month,
            None => span_from_settings(&settings),
        };
        let (from, to) = span.bounds(start, &settings.agenda.week_start);
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
            print!("{}", agenda_csv(&entries));
        } else if self.brief {
            for entry in &entries {
                println!("{} {}", entry.occurrence.date, agenda_title(&entry.item));
            }
        } else {
            print!("{}", render_agenda(&entries, from, to, span));
        }
        Ok(())
    }
}

/// Plain-text agenda in the layout of an Emacs org-agenda buffer: a header
/// per day, then `category:` + time + leader + TODO + title + tags.
fn render_agenda(
    entries: &[AgendaEntry],
    from: NaiveDate,
    to: NaiveDate,
    span: AgendaSpan,
) -> String {
    let mut output = String::new();
    if span == AgendaSpan::Week {
        output.push_str(&format!("Week-agenda (W{:02}):\n", from.iso_week().week()));
    }
    let mut day = from;
    while day <= to {
        let date = day.format("%Y-%m-%d").to_string();
        let rows: Vec<&AgendaEntry> = entries
            .iter()
            .filter(|entry| entry.occurrence.date == date)
            .collect();
        if span != AgendaSpan::Month || !rows.is_empty() {
            output.push_str(&format!(
                "{:<10} {:>2} {} {}{}\n",
                day.format("%A"),
                day.day(),
                day.format("%B"),
                day.year(),
                if span == AgendaSpan::Week && day.weekday().num_days_from_monday() == 0 {
                    format!(" W{:02}", day.iso_week().week())
                } else {
                    String::new()
                }
            ));
        }
        for entry in rows {
            output.push_str(&agenda_line(entry));
            output.push('\n');
        }
        day += Duration::days(1);
    }
    output
}

fn agenda_line(entry: &AgendaEntry) -> String {
    let item = &entry.item;
    let mut line = format!("  {:<12}", format!("{}:", item.category));
    if let Some(time) = &entry.occurrence.time {
        line.push_str(&format!("{time}...... "));
    }
    if !entry.occurrence.extra.is_empty() {
        line.push_str(&format!("{:<11} ", entry.occurrence.extra));
    }
    if let Some(todo) = &item.todo {
        line.push_str(todo);
        line.push(' ');
    }
    if let Some(priority) = &item.priority {
        line.push_str(&format!("[#{priority}] "));
    }
    line.push_str(&agenda_title(item));
    if !item.tags.is_empty() {
        line.push_str(&format!("  :{}:", item.tags.join(":")));
    }
    line
}

/// Blocks separated by a rule of `=`, as in an Emacs block agenda.
fn render_view(sections: &[ViewSection], settings: &Settings) -> String {
    let mut blocks = Vec::new();
    for section in sections {
        let mut block = String::new();
        if let (Some(from), Some(to)) = (section.from, section.to) {
            if !section.header.is_empty() {
                block.push_str(&format!("{}\n", section.header));
            }
            let span = span_for_bounds(from, to, settings);
            block.push_str(&render_agenda(&section.entries, from, to, span));
        } else {
            block.push_str(&format!("{}\n", section.header));
            for item in &section.items {
                let mut line = format!("  {:<12}", format!("{}:", item.category));
                if let Some(todo) = &item.todo {
                    line.push_str(&format!("{todo} "));
                }
                if let Some(priority) = &item.priority {
                    line.push_str(&format!("[#{priority}] "));
                }
                line.push_str(&agenda_title(item));
                if !item.tags.is_empty() {
                    line.push_str(&format!("  :{}:", item.tags.join(":")));
                }
                block.push_str(&line);
                block.push('\n');
            }
        }
        blocks.push(block);
    }
    blocks.join(&format!("{}\n", "=".repeat(78)))
}

fn span_for_bounds(from: NaiveDate, to: NaiveDate, settings: &Settings) -> AgendaSpan {
    if from == to {
        AgendaSpan::Day
    } else if AgendaSpan::Week.bounds(from, &settings.agenda.week_start) == (from, to) {
        AgendaSpan::Week
    } else {
        AgendaSpan::Month
    }
}

/// `3 slipped · 7 today · 4 inbox · 2 agents`, zero counts left out.
fn status_line(index: &WorkspaceIndex, settings: &Settings, today: NaiveDate) -> Result<String> {
    let engine = AgendaEngine::new(index);
    let entries = engine.agenda(&AgendaQuery {
        from: today,
        to: today,
        today,
        match_expression: None,
        log_mode: false,
    })?;
    let slipped_kind = |entry: &AgendaEntry| {
        entry.occurrence.kind == org_core::OccurrenceType::PastScheduled
            || entry.occurrence.extra.ends_with("ago:")
    };
    let mut slipped = std::collections::BTreeSet::new();
    let mut open = std::collections::BTreeSet::new();
    for entry in entries.iter().filter(|entry| !entry.item.done) {
        if slipped_kind(entry) {
            slipped.insert(&entry.item.item_ref);
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
        (slipped.len(), "slipped"),
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
    if let Some(days) = value.strip_prefix('-') {
        let back = parse_date_input(&format!("+{days}"), today)?
            .date
            .parse::<NaiveDate>()?;
        return Ok(today - (back - today));
    }
    Ok(parse_date_input(value, today)?.date.parse::<NaiveDate>()?)
}

fn default_start(settings: &Settings, today: NaiveDate) -> Result<NaiveDate> {
    let Some(value) = settings.agenda.start_day.as_deref() else {
        return Ok(today);
    };
    parse_day(value, today).with_context(|| format!("invalid agenda start_day: {value}"))
}

fn span_from_settings(settings: &Settings) -> AgendaSpan {
    match settings.agenda.span.as_str() {
        "day" => AgendaSpan::Day,
        "month" => AgendaSpan::Month,
        _ => AgendaSpan::Week,
    }
}
