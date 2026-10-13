use std::path::Path;

use anyhow::{Context, Result, anyhow};
use chrono::{Datelike, Duration, Local, NaiveDate};
use clap::{Args, ValueEnum};
use org_core::{
    AgendaEngine, AgendaEntry, AgendaQuery, AgendaSpan, Settings, WorkspaceIndex, agenda_csv,
    agenda_title, load_settings, parse_date_input,
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
    /// Use the files and match of a configured agents.profiles entry
    #[arg(long)]
    profile: Option<String>,
    /// Also list CLOSED entries, like org-agenda log mode (l)
    #[arg(long)]
    log: bool,
    /// One `date title` line per entry, for hooks and status bars
    #[arg(long)]
    brief: bool,
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
        let start = match self.from {
            Some(value) => parse_day(&value, today)?,
            None => default_start(&settings, today)?,
        };
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
