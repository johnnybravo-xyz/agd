use std::io::Write;
use std::path::PathBuf;

use agd_core::{ImportStatus, Settings, default_config_path, parse_config_file};
use anyhow::{Result, bail};
use clap::Args;

use crate::commands::counted;
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
        if execution.diff.is_empty() {
            let message = format!("{} is already up to date", destination.display());
            if self.stdout {
                eprintln!("{message}");
            } else {
                println!("{message}");
            }
            return Ok(());
        }
        if !preview && !write {
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
        let editor = sources.editor.name();
        writeln!(
            out,
            "read {}:",
            counted(
                sources.files.len(),
                &format!("{editor} file"),
                &format!("{editor} files")
            )
        )?;
        for file in &sources.files {
            writeln!(out, "  {}", file.display())?;
        }
        for note in &sources.notes {
            writeln!(out, "  {note}")?;
        }
        writeln!(out, "{}", imported_summary(&execution.result.settings))?;
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

/// What the import keeps. Views count the user's own; the built-in `n`
/// is always there.
fn imported_summary(settings: &Settings) -> String {
    format!(
        "imported  roots {}, {}, {}, {}",
        settings.roots.join(", "),
        counted(
            settings.agenda_files.len(),
            "agenda entry",
            "agenda entries"
        ),
        counted(
            settings.captures.len(),
            "capture template",
            "capture templates"
        ),
        counted(settings.views.len(), "view", "views")
    )
}

fn mark(status: &ImportStatus) -> &'static str {
    match status {
        ImportStatus::Exact => "✓",
        ImportStatus::Difference => "~",
        ImportStatus::Unsupported => "✗",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_counts_user_views_with_plurals() {
        let mut settings = Settings {
            agenda_files: vec!["tasks.org".to_string()],
            ..Settings::default()
        };
        // The built-in n view is not counted.
        assert_eq!(
            imported_summary(&settings),
            "imported  roots ~/org, 1 agenda entry, 1 capture template, 0 views"
        );
        settings.agenda_files.push("work.org".to_string());
        settings.captures.push(settings.captures[0].clone());
        assert_eq!(
            imported_summary(&settings),
            "imported  roots ~/org, 2 agenda entries, 2 capture templates, 0 views"
        );
    }
}
