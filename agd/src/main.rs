use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};
use org_core::OrgMode;

mod commands;
mod config;
mod mcp;
use commands::{
    AgendaCommand, CaptureCommand, CheckCommand, ConfigCommand, ElementByIdCommand, HeadingCommand,
    ListCommand, OutlineCommand, ReadCommand, SearchCommand, UpdateTodoCommand,
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
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Agenda views and task management
    Agenda(AgendaCommand),
    /// Capture a new heading into an org file
    Capture(Box<CaptureCommand>),
    /// Check org files for parsing and identity problems
    Check(CheckCommand),
    /// Configuration management
    Config(ConfigCommand),
    /// List all .org files in a directory
    List(ListCommand),
    /// Run the stdio Model Context Protocol server
    Mcp(McpCommand),
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
    /// Update TODO state and planning metadata of an existing heading
    UpdateTodo(Box<UpdateTodoCommand>),
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::FAILURE,
        Ok(false) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error:#}");
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

    match command {
        Commands::Config(command) => {
            command.execute(config)?;
            Ok(false)
        }
        Commands::Check(command) => {
            let default_root = if command.has_explicit_paths() {
                None
            } else {
                let config = CliAppConfig::load(config, root_directory, None)?;
                Some(config.org.org_directory.into())
            };
            command.execute(default_root)
        }
        Commands::Mcp(command) => {
            let config = CliAppConfig::load(config, root_directory, command.log_level.clone())?;
            command.execute(config.org, config.logging)?;
            Ok(false)
        }
        command => {
            let config = CliAppConfig::load(
                config,
                root_directory,
                None, // log_level not needed for CLI
            )?;
            let org_mode = OrgMode::new(config.org)?;

            match command {
                Commands::Agenda(command) => command.execute(org_mode, config.cli),
                Commands::Capture(command) => command.execute(org_mode, config.cli),
                Commands::List(command) => command.execute(org_mode, config.cli),
                Commands::Read(command) => command.execute(org_mode, config.cli),
                Commands::Outline(command) => command.execute(org_mode, config.cli),
                Commands::Heading(command) => command.execute(org_mode, config.cli),
                Commands::ElementById(command) => command.execute(org_mode, config.cli),
                Commands::Search(command) => command.execute(org_mode, config.cli),
                Commands::UpdateTodo(command) => command.execute(org_mode, config.cli),
                Commands::Check(_) | Commands::Config(_) | Commands::Mcp(_) => unreachable!(),
            }?;
            Ok(false)
        }
    }
}
