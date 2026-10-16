use std::process::ExitCode;

use agd_core::{AgdError, ErrorResponse};
use anyhow::Result;
use clap::error::ErrorKind;
use clap::{CommandFactory, Parser, Subcommand};

mod commands;
mod importer;
mod mcp;
mod tui;
use commands::{AgendaCommand, CaptureCommand, SetupCommand};
use mcp::McpCommand;

/// Org agenda for terminals and coding agents. With no command, opens your
/// start_view (or Today when none is set).
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

impl Cli {
    /// True when the command prints JSON, so its errors are agd.error/v1.
    fn json(&self) -> bool {
        match &self.command {
            Some(Commands::Agenda(command)) => command.json(),
            Some(Commands::Capture(command)) => command.json(),
            Some(Commands::Setup(command)) => command.json(),
            Some(Commands::Mcp(_)) | None => false,
        }
    }
}

fn main() -> ExitCode {
    let cli = match parse_cli() {
        Ok(cli) => cli,
        Err(error) => return usage_error(error),
    };
    let json = cli.json();
    match run(cli) {
        Ok(true) => ExitCode::FAILURE,
        Ok(false) => ExitCode::SUCCESS,
        // stdout's reader went away (`agd agenda | head`): stop quietly,
        // as Unix tools do.
        Err(error) if broken_pipe(&error) => ExitCode::SUCCESS,
        Err(error) if json => {
            print_json_error(&error_response(&error));
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("Error: {error:#}");
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

/// A command line clap refused (exit 2). There are no parsed arguments to
/// ask, so here alone a literal `--json` argument asks for agd.error/v1.
fn usage_error(error: clap::Error) -> ExitCode {
    let json = std::env::args_os().any(|argument| argument == "--json");
    if !json || !error.use_stderr() {
        // Help and version go to stdout as usual.
        error.exit();
    }
    let rendered = error.render().to_string();
    let message = rendered.trim().trim_start_matches("error: ");
    print_json_error(&AgdError::new("usage", message).into_response());
    ExitCode::from(2)
}

/// agd.error/v1 for any error: an AgdError keeps its code and `current`, with
/// any context added on top leading its message; other errors are `error`.
fn error_response(error: &anyhow::Error) -> ErrorResponse {
    let Some(agd_error) = error.downcast_ref::<AgdError>() else {
        return AgdError::new("error", format!("{error:#}")).into_response();
    };
    let mut response = agd_error.response().clone();
    let context: Vec<String> = error
        .chain()
        .take_while(|cause| !cause.is::<AgdError>())
        .map(ToString::to_string)
        .collect();
    if !context.is_empty() {
        response.message = format!("{}: {}", context.join(": "), response.message);
    }
    response
}

fn broken_pipe(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::BrokenPipe)
    })
}

fn print_json_error(response: &ErrorResponse) {
    match serde_json::to_string(response) {
        Ok(json) => eprintln!("{json}"),
        Err(error) => eprintln!("Error: {}: {error}", response.message),
    }
}

/// Returns true when the command completed but found problems (exit 1).
fn run(cli: Cli) -> Result<bool> {
    let Cli {
        config,
        root_directory,
        pane,
        command,
    } = cli;
    match command {
        None => tui::run(config, root_directory, pane).map(|()| false),
        Some(Commands::Agenda(command)) => command.execute(config, root_directory).map(|()| false),
        Some(Commands::Capture(command)) => command.execute(config, root_directory).map(|()| false),
        Some(Commands::Mcp(command)) => command.execute(config, root_directory).map(|()| false),
        Some(Commands::Setup(command)) => command.execute(config, root_directory),
    }
}
