use std::path::Path;

use agd_core::{
    CheckReport, JournalRecord, WriteContext, WriteEngine, check_settings, journal_problems,
    load_settings,
};
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct CheckCommand {
    /// Emit a machine-readable report
    #[arg(long)]
    json: bool,
    /// Settle interrupted writes whose files changed since, keeping those
    /// files exactly as they are on disk
    #[arg(long)]
    resolve: bool,
}

impl CheckCommand {
    pub fn execute(self, config: Option<String>, root_directory: Option<String>) -> Result<bool> {
        let loaded = load_settings(
            config.as_deref().map(Path::new),
            root_directory.as_deref().map(Path::new),
        )?;
        let engine = WriteEngine::with_default_journal(loaded.settings.clone())?;
        if self.resolve {
            let resolved = engine.resolve_conflicts(&check_context())?;
            print_resolved(&resolved, self.json);
        }
        let mut report = check_settings(&loaded.settings)?;
        report.add_problems(journal_problems(&engine.journal_conflicts()?));
        if self.json {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print_human_report(&report);
        }
        Ok(report.has_errors())
    }
}

fn check_context() -> WriteContext {
    WriteContext {
        actor: "cli".to_string(),
        project: std::env::current_dir()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "unknown".to_string()),
        now: None,
    }
}

/// One line per settled conflict; on stderr with --json so stdout stays
/// the report.
fn print_resolved(resolved: &[JournalRecord], json: bool) {
    let lines: Vec<String> = if resolved.is_empty() {
        vec!["no interrupted writes to resolve".to_string()]
    } else {
        resolved
            .iter()
            .map(|record| {
                format!(
                    "resolved journal entry {}: kept {} as it is on disk",
                    record.id, record.file
                )
            })
            .collect()
    };
    for line in lines {
        if json {
            eprintln!("{line}");
        } else {
            println!("{line}");
        }
    }
}

fn print_human_report(report: &CheckReport) {
    for problem in &report.problems {
        if let Some(line) = problem.line {
            println!(
                "{}:{line}: {}: {}",
                problem.file, problem.code, problem.message
            );
        } else {
            println!("{}: {}: {}", problem.file, problem.code, problem.message);
        }
    }
    println!(
        "checked {} files and {} headlines: {} errors",
        report.summary.files, report.summary.headlines, report.summary.errors
    );
}
