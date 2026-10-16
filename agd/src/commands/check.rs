use std::path::Path;

use agd_core::{CheckReport, check_settings, load_settings};
use anyhow::Result;
use clap::Args;

use crate::commands::counted;

#[derive(Args)]
pub struct CheckCommand {
    /// Emit a machine-readable report
    #[arg(long)]
    pub json: bool,
}

impl CheckCommand {
    pub fn execute(self, config: Option<String>, root_directory: Option<String>) -> Result<bool> {
        let loaded = load_settings(
            config.as_deref().map(Path::new),
            root_directory.as_deref().map(Path::new),
        )?;
        let report = check_settings(&loaded.settings)?;
        let output = if self.json {
            format!("{}\n", serde_json::to_string_pretty(&report)?)
        } else {
            human_report(&report)
        };
        super::write_stdout(&output)?;
        Ok(report.has_errors())
    }
}

fn human_report(report: &CheckReport) -> String {
    let mut output = String::new();
    for problem in &report.problems {
        if let Some(line) = problem.line {
            output.push_str(&format!(
                "{}:{line}: {}: {}\n",
                problem.file, problem.code, problem.message
            ));
        } else {
            output.push_str(&format!(
                "{}: {}: {}\n",
                problem.file, problem.code, problem.message
            ));
        }
    }
    output.push_str(&format!(
        "checked {} and {}: {}\n",
        counted(report.summary.files, "file", "files"),
        counted(report.summary.headlines, "headline", "headlines"),
        counted(report.summary.errors, "error", "errors")
    ));
    output
}
