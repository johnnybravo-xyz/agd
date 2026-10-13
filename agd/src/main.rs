use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};
use org_core::{AgendumError, OrgMode};

mod commands;
mod config;
mod importer;
mod mcp;
mod tui;
use commands::{
    AgendaCommand, CaptureCommand, CheckCommand, ConfigCommand, DeadlineCommand, EditCommand,
    ElementByIdCommand, HeadingCommand, ImportCommand, InstallCommand, ListCommand, OpenCommand,
    OutlineCommand, QueryCommand, ReadCommand, RefileCommand, ScheduleCommand, SearchCommand,
    StateCommand, TodoCommand, UndoCommand,
};
use config::CliAppConfig;
use mcp::McpCommand;

#[derive(Parser)]
#[command(name = "agd")]
#[command(about = "Local-first org-mode tools for terminals and coding agents")]
#[command(version = env!("CARGO_PKG_VERSION"))]
struct Cli {
    /// Path to configuration file
    #[arg(short, long)]
    config: Option<String>,

    /// Root directory containing org-mode files
    #[arg(short, long)]
    root_directory: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// List agenda occurrences
    Agenda(AgendaCommand),
    /// List unfinished TODO items
    Todo(TodoCommand),
    /// Query items with the supported org-ql subset
    Q(QueryCommand),
    /// Capture a new heading from a configured template
    Capture(CaptureCommand),
    /// Change an item's TODO state
    State(StateCommand),
    /// Set an item's scheduled timestamp
    Schedule(ScheduleCommand),
    /// Set an item's deadline
    Deadline(DeadlineCommand),
    /// Edit item title, tags, priority, properties, or note
    Edit(EditCommand),
    /// Open an item in the configured editor
    Open(OpenCommand),
    /// Refile an item below another item
    Refile(RefileCommand),
    /// Undo a guarded journal entry
    Undo(UndoCommand),
    /// Check org files for parsing and identity problems
    Check(CheckCommand),
    /// Configuration management
    Config(ConfigCommand),
    /// Import resolved editor configuration into a cold native snapshot
    Import(ImportCommand),
    /// Install a privacy-safe coding-agent integration
    Install(InstallCommand),
    /// List all .org files in a directory
    List(ListCommand),
    /// Run the stdio Model Context Protocol server
    Mcp(McpCommand),
    /// Open the full-screen terminal workflow
    Tui,
    /// Open the narrow glance surface
    Pane,
    /// Read the contents of an org file
    Read(ReadCommand),
    /// Get the outline (headings) of an org file
    Outline(OutlineCommand),
    /// Extract content from a specific heading in an org file
    Heading(HeadingCommand),
    /// Extract content from an element by ID across all org files
    ElementById(ElementByIdCommand),
    /// Search for text content across all org files using fuzzy matching
    Search(SearchCommand),
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::FAILURE,
        Ok(false) => ExitCode::SUCCESS,
        Err(error) => {
            let json_requested = std::env::args_os().any(|argument| argument == "--json");
            if json_requested && let Some(error) = error.downcast_ref::<AgendumError>() {
                eprintln!(
                    "{}",
                    serde_json::to_string(error.response())
                        .unwrap_or_else(|_| format!(r#"{{"message":"{error}"}}"#))
                );
            } else {
                eprintln!("Error: {error:#}");
            }
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<bool> {
    let Cli {
        config,
        root_directory,
        command,
    } = Cli::parse();
    let Some(command) = command else {
        tui::run(config, root_directory, false)?;
        return Ok(false);
    };

    match command {
        Commands::Agenda(command) => command.execute(config, root_directory),
        Commands::Todo(command) => command.execute(config, root_directory),
        Commands::Q(command) => command.execute(config, root_directory),
        Commands::Capture(command) => command.execute(config, root_directory),
        Commands::State(command) => command.execute(config, root_directory),
        Commands::Schedule(command) => command.execute(config, root_directory),
        Commands::Deadline(command) => command.execute(config, root_directory),
        Commands::Edit(command) => command.execute(config, root_directory),
        Commands::Open(command) => command.execute(config, root_directory),
        Commands::Refile(command) => command.execute(config, root_directory),
        Commands::Undo(command) => command.execute(config, root_directory),
        Commands::Config(command) => command.execute(config),
        Commands::Import(command) => command.execute(config, root_directory),
        Commands::Install(command) => command.execute(config, root_directory),
        Commands::Check(command) => return command.execute(config, root_directory),
        Commands::Mcp(command) => command.execute(config, root_directory),
        Commands::Tui => tui::run(config, root_directory, false),
        Commands::Pane => tui::run(config, root_directory, true),
        command => {
            let config = CliAppConfig::load(config, root_directory)?;
            let org_mode = OrgMode::new(config.org)?;
            match command {
                Commands::List(command) => command.execute(org_mode, config.cli),
                Commands::Read(command) => command.execute(org_mode, config.cli),
                Commands::Outline(command) => command.execute(org_mode, config.cli),
                Commands::Heading(command) => command.execute(org_mode, config.cli),
                Commands::ElementById(command) => command.execute(org_mode, config.cli),
                Commands::Search(command) => command.execute(org_mode, config.cli),
                Commands::Agenda(_)
                | Commands::Todo(_)
                | Commands::Q(_)
                | Commands::Capture(_)
                | Commands::State(_)
                | Commands::Schedule(_)
                | Commands::Deadline(_)
                | Commands::Edit(_)
                | Commands::Open(_)
                | Commands::Refile(_)
                | Commands::Undo(_)
                | Commands::Check(_)
                | Commands::Config(_)
                | Commands::Import(_)
                | Commands::Install(_)
                | Commands::Mcp(_)
                | Commands::Tui
                | Commands::Pane => unreachable!(),
            }
        }
    }?;
    Ok(false)
}
