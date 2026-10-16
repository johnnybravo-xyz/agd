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

/// `count` with its noun, singular for one: "1 file", "0 files".
pub fn counted(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("1 {singular}")
    } else {
        format!("{count} {plural}")
    }
}
