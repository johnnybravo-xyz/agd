use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{Duration, Local, NaiveDate};
use clap::{Args, ValueEnum};
use org_core::{
    AgendaEngine, AgendaQuery, AgendaSpan, CaptureDraft, Change, EditIntent, Item, Mutation,
    PlanningTimestamp, Settings, WorkspaceIndex, WriteContext, WriteEngine, agenda_csv,
    load_settings, parse_date_input,
};

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum SpanArg {
    Day,
    Week,
    Month,
}

impl SpanArg {
    fn into_core(self) -> AgendaSpan {
        match self {
            Self::Day => AgendaSpan::Day,
            Self::Week => AgendaSpan::Week,
            Self::Month => AgendaSpan::Month,
        }
    }
}

#[derive(Args, Debug)]
pub struct AgendaCommand {
    #[arg(long, value_enum)]
    span: Option<SpanArg>,
    #[arg(long)]
    from: Option<String>,
    #[arg(long)]
    profile: Option<String>,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    csv: bool,
    #[arg(long)]
    brief: bool,
}

#[derive(Args, Debug)]
pub struct TodoCommand {
    #[arg(long = "match")]
    match_expression: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct QueryCommand {
    expression: String,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct CaptureCommand {
    template: Option<String>,
    title: Option<String>,
    /// Open the focused terminal capture popup.
    #[arg(short = 'i', long)]
    interactive: bool,
    #[arg(long)]
    deadline: Option<String>,
    #[arg(long)]
    scheduled: Option<String>,
    #[arg(long)]
    tag: Vec<String>,
    #[arg(long)]
    priority: Option<String>,
    #[arg(long, default_value = "")]
    body: String,
    #[arg(long = "prompt", value_name = "KEY=VALUE")]
    prompts: Vec<String>,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct StateCommand {
    item_ref: String,
    state: String,
    #[arg(long)]
    note: Option<String>,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct ScheduleCommand {
    item_ref: String,
    date: String,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct DeadlineCommand {
    item_ref: String,
    date: String,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct EditCommand {
    item_ref: String,
    #[arg(long)]
    title: Option<String>,
    #[arg(long, value_name = "+TAG,-TAG")]
    tags: Option<String>,
    #[arg(long)]
    priority: Option<String>,
    #[arg(long = "prop", value_name = "KEY=VALUE")]
    properties: Vec<String>,
    #[arg(long)]
    note: Option<String>,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct OpenCommand {
    item_ref: String,
}

#[derive(Args, Debug)]
pub struct UndoCommand {
    #[arg(long)]
    journal: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct RefileCommand {
    item_ref: String,
    target_ref: String,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    json: bool,
}

impl AgendaCommand {
    pub fn execute(self, config: Option<String>, root: Option<String>) -> Result<()> {
        let mut settings = settings(config, root)?;
        let match_expression = if let Some(name) = self.profile.as_deref() {
            let profile = settings
                .agents
                .profiles
                .get(name)
                .cloned()
                .ok_or_else(|| anyhow!("unknown agent agenda profile: {name}"))?;
            if !profile.files.is_empty() {
                settings.files.agenda = profile.files;
            }
            profile.match_expression
        } else {
            None
        };
        let index = WorkspaceIndex::load(settings.clone())?;
        let today = Local::now().date_naive();
        let start = match self.from {
            Some(value) => parse_day(&value, today)?,
            None => default_start(&settings, today)?,
        };
        let span = self.span.map(SpanArg::into_core).unwrap_or_else(|| span_from_settings(&settings));
        let (from, to) = span.bounds(start, &settings.agenda.week_start);
        let entries = AgendaEngine::new(&index).agenda(&AgendaQuery {
            from,
            to,
            today,
            match_expression,
        })?;
        if self.json {
            println!("{}", serde_json::to_string_pretty(&entries)?);
        } else if self.csv {
            print!("{}", agenda_csv(&entries));
        } else {
            for entry in entries {
                if self.brief {
                    println!("{} {}", entry.occurrence.date, entry.item.title);
                } else {
                    let time = entry.occurrence.time.as_deref().unwrap_or("--:--");
                    let todo = entry.item.todo.as_deref().unwrap_or("");
                    println!(
                        "{} {} {:<10} {}  {}",
                        entry.occurrence.date,
                        time,
                        todo,
                        entry.item.title,
                        entry.item.item_ref
                    );
                }
            }
        }
        Ok(())
    }
}

impl TodoCommand {
    pub fn execute(self, config: Option<String>, root: Option<String>) -> Result<()> {
        let settings = settings(config, root)?;
        let index = WorkspaceIndex::load(settings)?;
        let items = AgendaEngine::new(&index).todo(self.match_expression.as_deref())?;
        print_items(&items, self.json)
    }
}

impl QueryCommand {
    pub fn execute(self, config: Option<String>, root: Option<String>) -> Result<()> {
        let settings = settings(config, root)?;
        let index = WorkspaceIndex::load(settings)?;
        let items = AgendaEngine::new(&index).query(&self.expression, Local::now().date_naive())?;
        print_items(&items, self.json)
    }
}

impl CaptureCommand {
    pub fn execute(self, config: Option<String>, root: Option<String>) -> Result<()> {
        if self.interactive {
            if self.template.is_some() || self.title.is_some() {
                bail!("interactive capture does not accept template or title arguments");
            }
            return crate::tui::run_capture(config, root);
        }
        let template = self
            .template
            .as_deref()
            .ok_or_else(|| anyhow!("capture template is required unless --interactive is used"))?;
        let title = self
            .title
            .ok_or_else(|| anyhow!("capture title is required unless --interactive is used"))?;
        let settings = settings(config, root)?;
        let today = Local::now().date_naive();
        let draft = CaptureDraft {
            title,
            scheduled: parse_optional_date(self.scheduled.as_deref(), today)?,
            deadline: parse_optional_date(self.deadline.as_deref(), today)?,
            tags: self.tag,
            priority: self.priority,
        };
        let prompts = key_values(self.prompts, "prompt")?.into_iter().collect();
        let mutation = WriteEngine::with_default_journal(settings)?.capture(
            template,
            &draft,
            &self.body,
            &prompts,
            self.dry_run,
            &write_context(),
        )?;
        print_mutation(&mutation, self.json, self.dry_run)
    }
}

impl StateCommand {
    pub fn execute(self, config: Option<String>, root: Option<String>) -> Result<()> {
        let intent = EditIntent {
            state: Change::Set(self.state),
            note: self.note,
            ..EditIntent::default()
        };
        edit_item(config, root, &self.item_ref, intent, self.dry_run, self.json)
    }
}

impl ScheduleCommand {
    pub fn execute(self, config: Option<String>, root: Option<String>) -> Result<()> {
        let intent = EditIntent {
            scheduled: Change::Set(parse_date_input(&self.date, Local::now().date_naive())?),
            ..EditIntent::default()
        };
        edit_item(config, root, &self.item_ref, intent, self.dry_run, self.json)
    }
}

impl DeadlineCommand {
    pub fn execute(self, config: Option<String>, root: Option<String>) -> Result<()> {
        let intent = EditIntent {
            deadline: Change::Set(parse_date_input(&self.date, Local::now().date_naive())?),
            ..EditIntent::default()
        };
        edit_item(config, root, &self.item_ref, intent, self.dry_run, self.json)
    }
}

impl EditCommand {
    pub fn execute(self, config: Option<String>, root: Option<String>) -> Result<()> {
        let (tags_add, tags_remove) = parse_tag_edits(self.tags.as_deref())?;
        let properties = key_values(self.properties, "property")?.into_iter().map(|(key, value)| (key, Some(value))).collect();
        let intent = EditIntent {
            title: self.title.map(Change::Set).unwrap_or_default(),
            priority: self.priority.map(Change::Set).unwrap_or_default(),
            tags_add,
            tags_remove,
            properties,
            note: self.note,
            ..EditIntent::default()
        };
        edit_item(config, root, &self.item_ref, intent, self.dry_run, self.json)
    }
}

impl OpenCommand {
    pub fn execute(self, config: Option<String>, root: Option<String>) -> Result<()> {
        let settings = settings(config, root)?;
        let index = WorkspaceIndex::load(settings.clone())?;
        let (document, headline) = index.resolve(&self.item_ref)?;
        let file = document.path().to_string_lossy();
        let line = headline.item.line.to_string();
        let args: Vec<String> = settings.editor.args.iter().map(|argument| {
            argument.replace("{file}", &file).replace("{line}", &line)
        }).collect();
        let status = Command::new(&settings.editor.command)
            .args(&args)
            .status()
            .with_context(|| format!("cannot start editor {}", settings.editor.command))?;
        if !status.success() {
            bail!("editor exited with {status}");
        }
        Ok(())
    }
}

impl UndoCommand {
    pub fn execute(self, config: Option<String>, root: Option<String>) -> Result<()> {
        let result = WriteEngine::with_default_journal(settings(config, root)?)?
            .undo(self.journal.as_deref(), &write_context())?;
        if self.json {
            println!("{}", serde_json::to_string_pretty(&result)?);
        } else {
            print!("{}", result.diff);
            println!("undid {}", result.journal_id);
        }
        Ok(())
    }
}

impl RefileCommand {
    pub fn execute(self, config: Option<String>, root: Option<String>) -> Result<()> {
        let result = WriteEngine::with_default_journal(settings(config, root)?)?.refile(
            &self.item_ref,
            &self.target_ref,
            self.dry_run,
            &write_context(),
        )?;
        if self.json {
            println!("{}", serde_json::to_string_pretty(&result)?);
        } else if self.dry_run {
            print!("{}", result.diffs.join("\n"));
        } else {
            println!("{}", result.item.item_ref);
        }
        Ok(())
    }
}

fn settings(config: Option<String>, root: Option<String>) -> Result<Settings> {
    Ok(load_settings(
        config.as_deref().map(Path::new),
        root.as_deref().map(Path::new),
    )?.settings)
}

fn edit_item(
    config: Option<String>,
    root: Option<String>,
    item_ref: &str,
    intent: EditIntent,
    dry_run: bool,
    json: bool,
) -> Result<()> {
    let mutation = WriteEngine::with_default_journal(settings(config, root)?)?
        .edit(item_ref, &intent, dry_run, &write_context())?;
    print_mutation(&mutation, json, dry_run)
}

fn print_mutation(mutation: &Mutation, json: bool, dry_run: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(mutation)?);
    } else if dry_run {
        print!("{}", mutation.diff);
    } else {
        println!("{}", mutation.item.item_ref);
    }
    Ok(())
}

fn print_items(items: &[Item], json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(items)?);
    } else {
        for item in items {
            let todo = item.todo.as_deref().unwrap_or("");
            println!("{:<10} {}  {}", todo, item.title, item.item_ref);
        }
    }
    Ok(())
}

fn parse_optional_date(value: Option<&str>, today: NaiveDate) -> Result<Option<PlanningTimestamp>> {
    value.map(|value| parse_date_input(value, today).map_err(Into::into)).transpose()
}

fn parse_day(value: &str, today: NaiveDate) -> Result<NaiveDate> {
    Ok(parse_date_input(value, today)?.date.parse::<NaiveDate>()?)
}

fn default_start(settings: &Settings, today: NaiveDate) -> Result<NaiveDate> {
    let Some(value) = settings.agenda.start_day.as_deref() else {
        return Ok(today);
    };
    if let Some(days) = value.strip_prefix('-').and_then(|value| value.strip_suffix('d')) {
        let days = days.parse::<i64>().with_context(|| format!("invalid agenda start_day: {value}"))?;
        return Ok(today - Duration::days(days));
    }
    parse_day(value, today)
}

fn span_from_settings(settings: &Settings) -> AgendaSpan {
    match settings.agenda.span.as_str() {
        "day" => AgendaSpan::Day,
        "month" => AgendaSpan::Month,
        _ => AgendaSpan::Week,
    }
}

fn parse_tag_edits(value: Option<&str>) -> Result<(BTreeSet<String>, BTreeSet<String>)> {
    let mut add = BTreeSet::new();
    let mut remove = BTreeSet::new();
    for edit in value.into_iter().flat_map(|value| value.split(',')).filter(|value| !value.is_empty()) {
        if let Some(tag) = edit.strip_prefix('+') {
            add.insert(tag.to_string());
        } else if let Some(tag) = edit.strip_prefix('-') {
            remove.insert(tag.to_string());
        } else {
            bail!("tag edit must start with + or -: {edit}");
        }
    }
    Ok((add, remove))
}

fn key_values(values: Vec<String>, label: &str) -> Result<BTreeMap<String, String>> {
    values.into_iter().map(|value| {
        let (key, value) = value.split_once('=').ok_or_else(|| anyhow!("{label} must be KEY=VALUE: {value}"))?;
        if key.is_empty() {
            bail!("{label} key cannot be empty");
        }
        Ok((key.to_string(), value.to_string()))
    }).collect()
}

fn write_context() -> WriteContext {
    WriteContext {
        actor: "cli".to_string(),
        project: std::env::current_dir().map(|path| path.to_string_lossy().into_owned()).unwrap_or_else(|_| "unknown".to_string()),
    }
}
