use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use org_core::{ImportStatus, Settings, default_config_path, parse_config_file};

use crate::importer::{ImportExecution, import_emacs, import_nvim};

#[derive(Args, Debug)]
pub struct ImportCommand {
    #[command(subcommand)]
    editor: ImportEditor,
}

#[derive(Subcommand, Debug)]
enum ImportEditor {
    /// Import org settings from a running Emacs daemon
    Emacs(EmacsImport),
    /// Import org settings from nvim-orgmode
    Nvim(NvimImport),
}

#[derive(Args, Debug)]
struct EmacsImport {
    /// Emacs server (socket) name to ask
    #[arg(long)]
    server: Option<String>,
    #[command(flatten)]
    output: ImportOutput,
}

#[derive(Args, Debug)]
struct NvimImport {
    #[command(flatten)]
    output: ImportOutput,
}

#[derive(Args, Debug)]
struct ImportOutput {
    /// Show the config diff without writing it
    #[arg(long)]
    dry_run: bool,
    /// Print the generated config instead of writing it
    #[arg(long)]
    stdout: bool,
    /// Replace an existing config file
    #[arg(long)]
    force: bool,
    /// Compare imported views with the editor's own agenda rows
    #[arg(long)]
    verify: bool,
}

impl ImportCommand {
    pub fn execute(self, config: Option<String>, _root_directory: Option<String>) -> Result<()> {
        let destination = match config {
            Some(path) => PathBuf::from(path),
            None => default_config_path()?,
        };
        let base = if destination.exists() {
            parse_config_file(&destination)?
        } else {
            Settings::default()
        };
        let output = match &self.editor {
            ImportEditor::Emacs(command) => &command.output,
            ImportEditor::Nvim(command) => &command.output,
        };
        let preview = output.dry_run || output.stdout;
        let write = !preview && (!destination.exists() || output.force);
        let result = match &self.editor {
            ImportEditor::Emacs(command) => import_emacs(
                &base,
                command.server.as_deref(),
                &destination,
                write,
                output.verify,
            )?,
            ImportEditor::Nvim(_) => import_nvim(&base, &destination, write, output.verify)?,
        };
        print_result(&result, output);
        if !preview && !write && !result.diff.is_empty() {
            bail!(
                "{} exists; review the diff above and rerun with --force to replace it",
                destination.display()
            );
        }
        if write {
            println!("wrote {}", destination.display());
        }
        Ok(())
    }
}

fn print_result(execution: &ImportExecution, output: &ImportOutput) {
    if output.stdout {
        print!("{}", execution.toml);
    } else if !execution.diff.is_empty() {
        print!("{}", execution.diff);
    }
    let settings = &execution.result.settings;
    println!(
        "imported  {} agenda files, {} views, {} capture templates",
        settings.agenda_files.len(),
        settings.views.len(),
        settings.captures.len()
    );
    for result in &execution.verification {
        println!(
            "  {} {}  {}  {}",
            mark(&result.status),
            result.key,
            result.description,
            result.detail
        );
    }
    for diagnostic in &execution.result.diagnostics {
        println!(
            "  {} {}  {}",
            mark(&diagnostic.status),
            diagnostic.key,
            diagnostic.message
        );
    }
}

fn mark(status: &ImportStatus) -> &'static str {
    match status {
        ImportStatus::Exact => "✓",
        ImportStatus::Difference => "~",
        ImportStatus::Unsupported => "✗",
    }
}
