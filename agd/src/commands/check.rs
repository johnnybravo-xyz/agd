use std::path::Path;

use agd_core::{CheckReport, check_settings, load_settings};
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct CheckCommand {
    /// Emit a machine-readable report
    #[arg(long)]
    json: bool,
}

impl CheckCommand {
    pub fn execute(self, config: Option<String>, root_directory: Option<String>) -> Result<bool> {
        let loaded = load_settings(
            config.as_deref().map(Path::new),
            root_directory.as_deref().map(Path::new),
        )?;
        let report = check_settings(&loaded.settings)?;
        if self.json {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print_human_report(&report);
        }
        Ok(report.has_errors())
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
