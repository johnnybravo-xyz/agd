use std::path::PathBuf;

use anyhow::{Result, anyhow};
use clap::Args;
use org_core::{CheckReport, check_paths};

#[derive(Args)]
pub struct CheckCommand {
    /// Files or directories to check; defaults to the configured org directory
    #[arg(value_name = "PATH")]
    paths: Vec<PathBuf>,

    /// Emit a machine-readable report
    #[arg(long)]
    json: bool,
}

impl CheckCommand {
    pub fn has_explicit_paths(&self) -> bool {
        !self.paths.is_empty()
    }

    pub fn execute(&self, default_root: Option<PathBuf>) -> Result<bool> {
        let roots = if self.paths.is_empty() {
            vec![default_root.ok_or_else(|| anyhow!("no org directory configured"))?]
        } else {
            self.paths.clone()
        };
        let report = check_paths(&roots);

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
