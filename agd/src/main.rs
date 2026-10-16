use std::process::ExitCode;

use agd_core::AgdError;
use anyhow::Result;
use clap::error::ErrorKind;
use clap::{CommandFactory, Parser, Subcommand};

mod commands;
mod importer;
mod mcp;
mod tui;
use commands::{AgendaCommand, CaptureCommand, SetupCommand};
use mcp::McpCommand;

/// Org agenda for terminals and coding agents. With no command, opens Today.
#[derive(Parser)]
#[command(name = "agd", version)]
struct Cli {
    /// Configuration file (default: ~/.config/agd/config.toml, or $XDG_CONFIG_HOME/agd)
    #[arg(short, long, global = true)]
    config: Option<String>,

    /// Use this directory as the only files root for this run
    #[arg(short, long, global = true)]
    root_directory: Option<String>,

    /// Open the narrow glance pane for a tmux or herdr split
    #[arg(long)]
    pane: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Print the agenda like org-agenda, or as --json / --csv
    Agenda(AgendaCommand),
    /// Capture one line through a template, or open the popup with -i
    Capture(CaptureCommand),
    /// Run the stdio MCP server for coding agents
    Mcp(McpCommand),
    /// Import editor settings, install agent integrations, check files
    Setup(SetupCommand),
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::FAILURE,
        Ok(false) => ExitCode::SUCCESS,
        Err(error) => {
            let json_requested = std::env::args_os().any(|argument| argument == "--json");
            if json_requested && let Some(error) = error.downcast_ref::<AgdError>() {
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

/// Clap's parse, plus `--pane` with a command: clap cannot declare that
/// conflict without also refusing the global options before a command.
fn parse_cli() -> Result<Cli, clap::Error> {
    let cli = Cli::try_parse()?;
    if cli.pane && cli.command.is_some() {
        return Err(Cli::command().error(
            ErrorKind::ArgumentConflict,
            "--pane opens the glance pane, so it takes no command",
        ));
    }
    Ok(cli)
}

/// Returns true when the command completed but found problems (exit 1).
fn run() -> Result<bool> {
    let Cli {
        config,
        root_directory,
        pane,
        command,
    } = parse_cli().unwrap_or_else(|error| error.exit());
    match command {
        None => tui::run(config, root_directory, pane).map(|()| false),
        Some(Commands::Agenda(command)) => command.execute(config, root_directory).map(|()| false),
        Some(Commands::Capture(command)) => command.execute(config, root_directory).map(|()| false),
        Some(Commands::Mcp(command)) => command.execute(config, root_directory).map(|()| false),
        Some(Commands::Setup(command)) => command.execute(config, root_directory),
    }
}
