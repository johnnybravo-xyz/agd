use std::io::Write;
use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::Args;
use org_core::{ImportStatus, Settings, default_config_path, parse_config_file};

use crate::importer::{ImportExecution, import_config};

/// Where Emacs and Neovim configs usually live; offered when no path is given.
const USUAL_LOCATIONS: [&str; 7] = [
    "~/.config/nvim",
    "~/.doom.d",
    "~/.config/doom",
    "~/.emacs.d",
    "~/.config/emacs",
    "~/.emacs",
    "~/.spacemacs",
];

#[derive(Args, Debug)]
pub struct ImportCommand {
    /// Your Emacs or Neovim config: a directory (~/.doom.d, ~/.config/nvim)
    /// or files (init.el, lua/plugins/orgmode.lua)
    paths: Vec<PathBuf>,
    /// Show the config diff without writing it
    #[arg(long)]
    dry_run: bool,
    /// Print the generated config instead of writing it
    #[arg(long)]
    stdout: bool,
    /// Replace an existing config file
    #[arg(long)]
    force: bool,
    /// Emacs only: compare each imported view's rows with Emacs's own agenda
    #[arg(long)]
    verify: bool,
}

impl ImportCommand {
    pub fn execute(self, config: Option<String>, _root_directory: Option<String>) -> Result<()> {
        if self.paths.is_empty() {
            let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
            let found: Vec<&str> = USUAL_LOCATIONS
                .iter()
                .copied()
                .filter(|location| home.join(&location[2..]).exists())
                .collect();
            if found.is_empty() {
                bail!("name your Emacs or Neovim config: agd setup import ~/.config/nvim");
            }
            bail!(
                "name the config to import, for example: agd setup import {}\nfound: {}",
                found[0],
                found.join(", ")
            );
        }
        let destination = match config {
            Some(path) => PathBuf::from(path),
            None => default_config_path()?,
        };
        let exists = destination.exists();
        let base = if exists {
            parse_config_file(&destination)?
        } else {
            Settings::default()
        };
        let preview = self.dry_run || self.stdout;
        let write = !preview && (!exists || self.force);
        let execution = import_config(
            &self.paths,
            &base,
            !exists,
            &destination,
            write,
            self.verify,
        )?;
        self.print(&execution)?;
        if !preview && !write && !execution.diff.is_empty() {
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

    /// With --stdout the config is the only stdout output, so it can be
    /// redirected to a file; the report goes to stderr.
    fn print(&self, execution: &ImportExecution) -> Result<()> {
        let (mut stdout, mut stderr) = (std::io::stdout(), std::io::stderr());
        let out: &mut dyn Write = if self.stdout {
            print!("{}", execution.toml);
            &mut stderr
        } else {
            print!("{}", execution.diff);
            &mut stdout
        };
        let sources = &execution.sources;
        writeln!(
            out,
            "read {} {} file(s):",
            sources.editor.name(),
            sources.files.len()
        )?;
        for file in &sources.files {
            writeln!(out, "  {}", file.display())?;
        }
        for note in &sources.notes {
            writeln!(out, "  {note}")?;
        }
        let settings = &execution.result.settings;
        writeln!(
            out,
            "imported  roots {}, {} agenda entries, {} capture templates, {} views",
            settings.roots.join(", "),
            settings.agenda_files.len(),
            settings.captures.len(),
            settings.views.len()
        )?;
        for diagnostic in &execution.result.diagnostics {
            writeln!(
                out,
                "  {} {}  {}",
                mark(&diagnostic.status),
                diagnostic.key,
                diagnostic.message
            )?;
        }
        for result in &execution.verification {
            writeln!(
                out,
                "  {} view {}  {}  {}",
                mark(&result.status),
                result.key,
                result.description,
                result.detail
            )?;
        }
        Ok(())
    }
}

fn mark(status: &ImportStatus) -> &'static str {
    match status {
        ImportStatus::Exact => "✓",
        ImportStatus::Difference => "~",
        ImportStatus::Unsupported => "✗",
    }
}
