use std::io::{IsTerminal, Write};

pub mod agenda;
pub mod capture;
pub mod check;
pub mod import;
pub mod install;
pub mod setup;
pub mod undo;

pub use agenda::AgendaCommand;
pub use capture::CaptureCommand;
pub use check::CheckCommand;
pub use import::ImportCommand;
pub use install::InstallCommand;
pub use setup::SetupCommand;
pub use undo::UndoCommand;

/// `count` with its noun, singular for one: "1 file", "0 files".
pub fn counted(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("1 {singular}")
    } else {
        format!("{count} {plural}")
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
