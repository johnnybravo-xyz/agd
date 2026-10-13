use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::{Args, Subcommand};
use org_core::{
    ImportSnapshot, ImportStatus, load_settings, resolve_config_path, snapshot_or_default,
};

use crate::importer::{import_emacs, import_nvim};

#[derive(Args, Debug)]
pub struct ImportCommand {
    #[command(subcommand)]
    editor: ImportEditor,
}

#[derive(Subcommand, Debug)]
enum ImportEditor {
    /// Import resolved configuration from a selected Emacs daemon or batch profile
    Emacs(EmacsImport),
    /// Import resolved configuration from nvim-orgmode
    Nvim(NvimImport),
}

#[derive(Args, Debug)]
struct EmacsImport {
    /// Emacs server (socket) name to ask; overrides imports.emacs.server
    #[arg(long, conflicts_with = "batch_profile")]
    server: Option<String>,
    /// Explicit configured batch profile; never selected implicitly
    #[arg(long)]
    batch_profile: Option<String>,
    #[command(flatten)]
    output: ImportOutput,
}

#[derive(Args, Debug)]
struct NvimImport {
    /// Configured Neovim executable-and-argv profile
    #[arg(long)]
    profile: Option<String>,
    #[command(flatten)]
    output: ImportOutput,
}

#[derive(Args, Debug)]
struct ImportOutput {
    /// Extract and show the snapshot diff without writing it
    #[arg(long)]
    dry_run: bool,
    /// Print generated TOML instead of writing it
    #[arg(long)]
    stdout: bool,
    /// Print exact, documented-difference, and unsupported entries
    #[arg(long)]
    verify: bool,
}

impl ImportCommand {
    pub fn execute(self, config: Option<String>, root_directory: Option<String>) -> Result<()> {
        let loaded = load_settings(
            config.as_deref().map(Path::new),
            root_directory.as_deref().map(Path::new),
        )?;
        match self.editor {
            ImportEditor::Emacs(command) => {
                let destination = snapshot_path(
                    &loaded.config_dir,
                    &loaded.settings.imports.emacs.snapshot,
                    "emacs.toml",
                );
                let write = !command.output.dry_run && !command.output.stdout;
                if command.output.verify {
                    eprintln!(
                        "warning: verification executes supported agenda views in the selected Emacs endpoint"
                    );
                }
                let result = import_emacs(
                    &loaded.settings,
                    command.server.as_deref(),
                    command.batch_profile.as_deref(),
                    &destination,
                    write,
                    command.output.verify,
                )?;
                print_result(
                    &result.snapshot,
                    &result.toml,
                    &result.diff,
                    &result.verification,
                    &command.output,
                );
            }
            ImportEditor::Nvim(command) => {
                let destination = snapshot_path(
                    &loaded.config_dir,
                    &loaded.settings.imports.nvim.snapshot,
                    "nvim.toml",
                );
                let write = !command.output.dry_run && !command.output.stdout;
                let result = import_nvim(
                    &loaded.settings,
                    command.profile.as_deref(),
                    &destination,
                    write,
                    command.output.verify,
                )?;
                print_result(
                    &result.snapshot,
                    &result.toml,
                    &result.diff,
                    &result.verification,
                    &command.output,
                );
            }
        }
        Ok(())
    }
}

fn snapshot_path(config_dir: &Path, configured: &str, default_name: &str) -> PathBuf {
    resolve_config_path(config_dir, &snapshot_or_default(configured, default_name))
}

fn print_result(
    snapshot: &ImportSnapshot,
    toml: &str,
    diff: &str,
    verification: &[crate::importer::VerificationResult],
    output: &ImportOutput,
) {
    if output.stdout {
        print!("{toml}");
    } else if !diff.is_empty() {
        print!("{diff}");
    }
    if !output.verify {
        return;
    }
    println!(
        "imported  {} custom commands, {} capture templates",
        snapshot.commands.len(),
        snapshot.settings.capture.len()
    );
    println!("  ✓ core  resolved editor settings normalized exactly");
    for result in verification {
        let mark = match result.status {
            ImportStatus::Exact => "✓",
            ImportStatus::Difference => "~",
            ImportStatus::Unsupported => "✗",
        };
        println!(
            "  {mark} {}  {}  {}",
            result.key, result.description, result.detail
        );
    }
    for (key, template) in &snapshot.settings.capture {
        let (mark, detail) = if template.enabled {
            ("✓", "declarative capture imported")
        } else {
            (
                "✗",
                template
                    .diagnostics
                    .first()
                    .map(String::as_str)
                    .unwrap_or("unsupported"),
            )
        };
        println!("  {mark} c {key}  {}  {detail}", template.desc);
    }
    for diagnostic in &snapshot.diagnostics {
        let mark = match diagnostic.status {
            ImportStatus::Exact => "✓",
            ImportStatus::Difference => "~",
            ImportStatus::Unsupported => "✗",
        };
        println!("  {mark} {}  {}", diagnostic.key, diagnostic.message);
    }
}
