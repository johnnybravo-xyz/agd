pub mod agenda;
pub mod capture;
pub mod check;
pub mod import;
pub mod install;
pub mod setup;

pub use agenda::AgendaCommand;
pub use capture::CaptureCommand;
pub use check::CheckCommand;
pub use import::ImportCommand;
pub use install::InstallCommand;
pub use setup::SetupCommand;

use std::io::Write;

/// Writes a command's output to stdout and flushes it. When the reader has
/// gone (`agd agenda | head`) this returns io::ErrorKind::BrokenPipe, which
/// main ends quietly; `println!` would panic instead.
pub fn write_stdout(text: &str) -> std::io::Result<()> {
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(text.as_bytes())?;
    stdout.flush()
}
