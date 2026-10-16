use std::io::{IsTerminal, Write};

use agd_core::WriteContext;

pub mod agenda;
pub mod capture;
pub mod check;
pub mod completions;
pub mod import;
pub mod install;
pub mod setup;
pub mod undo;

pub use agenda::AgendaCommand;
pub use capture::CaptureCommand;
pub use check::CheckCommand;
pub use completions::CompletionsCommand;
pub use import::ImportCommand;
pub use install::InstallCommand;
pub use setup::SetupCommand;
pub use undo::UndoCommand;

/// The actor of every change the command line makes, `agd capture` and
/// the Claude Code /capture command among them.
pub const CLI_ACTOR: &str = "cli";

/// The write context of a command: the CLI actor, run from the current
/// directory.
pub fn cli_context() -> WriteContext {
    WriteContext {
        actor: CLI_ACTOR.to_string(),
        project: std::env::current_dir()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "unknown".to_string()),
        now: None,
    }
}

/// Whether stdout gets terminal colours: it is a terminal and NO_COLOR is
/// unset or empty (no-color.org).
pub fn color_stdout() -> bool {
    std::io::stdout().is_terminal() && !no_color()
}

/// Whether stderr gets terminal colours, as `color_stdout`.
pub fn color_stderr() -> bool {
    std::io::stderr().is_terminal() && !no_color()
}

fn no_color() -> bool {
    std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty())
}

/// Writes a command's output to stdout and flushes it. When the reader has
/// gone (`agd agenda | head`) this returns io::ErrorKind::BrokenPipe, which
/// main ends quietly; `println!` would panic instead.
pub fn write_stdout(text: &str) -> std::io::Result<()> {
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(text.as_bytes())?;
    stdout.flush()
}
