use std::path::Path;

use agd_core::{
    CheckReport, JournalRecord, WriteEngine, check_settings, journal_problems, load_settings,
};
use anyhow::Result;
use clap::Args;

use crate::words::counted;

#[derive(Args)]
pub struct CheckCommand {
    /// Emit a machine-readable report
    #[arg(long)]
    pub json: bool,
    /// Settle interrupted writes whose files changed since, keeping those
    /// files exactly as they are on disk
    #[arg(long)]
    resolve: bool,
}

impl CheckCommand {
    pub fn execute(self, config: Option<String>, root_directory: Option<String>) -> Result<bool> {
        let settings = load_settings(
            config.as_deref().map(Path::new),
            root_directory.as_deref().map(Path::new),
        )?;
        let engine = WriteEngine::with_default_journal(settings.clone())?;
        let mut output = String::new();
        if self.resolve {
            let resolved = resolved_lines(&engine.resolve_conflicts(&super::cli_context())?);
            // With --json, stdout stays the report.
            if self.json {
                eprint!("{resolved}");
            } else {
                output.push_str(&resolved);
            }
        }
        let mut report = check_settings(&settings)?;
        report.add_problems(journal_problems(&engine.journal_conflicts()?));
        if self.json {
            output.push_str(&format!("{}\n", serde_json::to_string_pretty(&report)?));
        } else {
            output.push_str(&human_report(&report));
        }
        super::write_stdout(&output)?;
        Ok(report.has_errors())
    }
}

/// One line per settled conflict.
fn resolved_lines(resolved: &[JournalRecord]) -> String {
    if resolved.is_empty() {
        return "no interrupted writes to resolve\n".to_string();
    }
    resolved
        .iter()
        .map(|record| {
            format!(
                "resolved journal entry {}: kept {} as it is on disk\n",
                record.id, record.file
            )
        })
        .collect()
}

/// `file:line: message` per problem, then the counts; codes are for JSON.
fn human_report(report: &CheckReport) -> String {
    let mut output = String::new();
    for problem in &report.problems {
        match problem.line {
            Some(line) => {
                output.push_str(&format!("{}:{line}: {}\n", problem.file, problem.message))
            }
            None => output.push_str(&format!("{}: {}\n", problem.file, problem.message)),
        }
    }
    output.push_str(&format!(
        "checked {} and {}: {}\n",
        counted(report.summary.files, "file", "files"),
        counted(report.summary.headlines, "heading", "headings"),
        counted(report.summary.errors, "error", "errors")
    ));
    output
}
