use std::io::Write;
use std::path::{Path, PathBuf};

use agd_core::{
    ImportStatus, Notation, Settings, default_config_path, import_root, parse_config_file,
};
use anyhow::{Result, bail};
use clap::Args;

use crate::importer::{ImportExecution, import_config};
use crate::words::counted;

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
    /// `root_directory` (`-r DIR`) becomes the imported config's only root
    /// instead of one derived from the editor's org-directory or agenda
    /// files, or the existing config's roots.
    pub fn execute(self, config: Option<String>, root_directory: Option<String>) -> Result<()> {
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
        let mut base = if exists {
            parse_config_file(&destination)?
        } else {
            Settings::default()
        };
        if let Some(root) = &root_directory {
            base.roots = vec![import_root(Path::new(root))?];
        }
        let derive_roots = !exists && root_directory.is_none();
        let preview = self.dry_run || self.stdout;
        let write = !preview && (!exists || self.force);
        let execution = import_config(
            &self.paths,
            &base,
            derive_roots,
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
            super::write_stdout(&format!("wrote {}\n", destination.display()))?;
        }
        Ok(())
    }

    /// With --stdout the config is the only stdout output, so it can be
    /// redirected to a file; the report goes to stderr.
    fn print(&self, execution: &ImportExecution) -> Result<()> {
        let (mut stdout, mut stderr) = (std::io::stdout(), std::io::stderr());
        let out: &mut dyn Write = if self.stdout {
            super::write_stdout(&execution.toml)?;
            &mut stderr
        } else {
            super::write_stdout(&execution.diff)?;
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
        if let Some(hint) = notation_hint(&execution.result.settings) {
            writeln!(out, "{hint}")?;
        }
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

/// One line offering Org's notation, which an import leaves to the person:
/// agd's `#work` and `!A` until config.toml says `notation = "org"`.
fn notation_hint(settings: &Settings) -> Option<&'static str> {
    (settings.notation == Notation::Agd).then_some(
        "tip  notation = \"org\" in config.toml types and shows tags and priorities as Org writes them (:work:, #A), not #work and !A",
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

    #[test]
    fn the_report_offers_orgs_notation_until_it_is_set() {
        let mut settings = Settings::default();
        let hint = notation_hint(&settings).unwrap();
        assert!(hint.contains("notation = \"org\""), "{hint}");
        settings.notation = Notation::Org;
        assert_eq!(notation_hint(&settings), None);
    }
}
