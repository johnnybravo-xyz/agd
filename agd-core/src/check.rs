//! `agd setup check`: a read-only report ([`CheckReport`], schema
//! `agd.check/v1`) on the org files under the roots and on the configuration:
//! files that cannot be read or parsed, files the Org parser would not
//! reproduce byte for byte, repeated `:ID:`s, TODO keywords a file uses
//! without enabling them, and interrupted writes.
//!
//! It parses each file as the index does ([`IndexedDocument`](crate::IndexedDocument))
//! and never writes. Its problem `code`s are [`ErrorCode`](crate::ErrorCode)s,
//! the same code an agd error gives for the same failure: `io_failed`,
//! `walk_failed`, `invalid_utf8`, `parse_failed`, `id_conflict`,
//! `invalid_todo`, `invalid_path` and `journal_conflict`.

use std::collections::HashMap;
use std::fs;

use crate::{AgdError, CHECK_SCHEMA, ErrorCode, JournalRecord, RootSet, Settings};
use serde::Serialize;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CheckReport {
    pub schema: &'static str,
    pub files: Vec<CheckFileReport>,
    pub problems: Vec<CheckProblem>,
    pub summary: CheckSummary,
}

impl CheckReport {
    pub fn has_errors(&self) -> bool {
        self.summary.errors != 0
    }

    /// Adds problems found outside the files, such as journal conflicts.
    pub fn add_problems(&mut self, problems: Vec<CheckProblem>) {
        self.summary.errors += problems.len();
        self.problems.extend(problems);
    }
}

/// One problem per interrupted write whose file changed since (see
/// `WriteEngine::journal_conflicts`), naming the command that settles it.
pub fn journal_problems(conflicts: &[JournalRecord]) -> Vec<CheckProblem> {
    conflicts
        .iter()
        .map(|record| CheckProblem {
            file: record.file.clone(),
            line: None,
            code: ErrorCode::JournalConflict.as_str(),
            message: format!(
                "an agd write was interrupted (journal entry {}) and the file changed since; writes to it are refused until `agd setup check --resolve` keeps it as it is",
                record.id
            ),
        })
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CheckFileReport {
    pub file: String,
    pub headlines: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CheckProblem {
    pub file: String,
    pub line: Option<usize>,
    pub code: &'static str,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CheckSummary {
    pub files: usize,
    pub headlines: usize,
    pub errors: usize,
}

pub fn check_settings(settings: &Settings) -> Result<CheckReport, AgdError> {
    let roots = RootSet::new(&settings.roots, &settings.agenda_files)?;
    let org_files = roots.all_org_files()?;
    let paths = org_files.files;
    // The configured keywords, read as a file without #+TODO lines reads them.
    let configured_keywords: std::collections::HashSet<String> =
        crate::document::todo_keywords("", settings)
            .into_iter()
            .map(|keyword| keyword.name)
            .collect();
    let mut files = Vec::with_capacity(paths.len());
    let mut problems: Vec<CheckProblem> = org_files
        .skipped
        .into_iter()
        .map(|(path, reason)| CheckProblem {
            file: path.to_string_lossy().into_owned(),
            line: None,
            code: reason.code(),
            message: reason.message(),
        })
        .collect();
    let mut ids: HashMap<String, (String, usize)> = HashMap::new();
    let mut headline_count = 0;

    for path in paths {
        let display_path = path.to_string_lossy().into_owned();
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                problems.push(CheckProblem {
                    file: display_path,
                    line: None,
                    code: ErrorCode::IoFailed.as_str(),
                    message: error.to_string(),
                });
                continue;
            }
        };
        let document = match crate::document::OrgDocument::parse(path, bytes, settings) {
            Ok(document) => document,
            Err(error) => {
                problems.push(CheckProblem {
                    file: display_path,
                    line: None,
                    code: error.code().as_str(),
                    message: error.to_string(),
                });
                continue;
            }
        };

        if let Some(value) = crate::document::ignored_priorities_line(document.source()) {
            let line = document.source().lines().position(|line| {
                line.get(..13)
                    .is_some_and(|key| key.eq_ignore_ascii_case("#+PRIORITIES:"))
            });
            problems.push(CheckProblem {
                file: display_path.clone(),
                line: line.map(|line| line + 1),
                code: ErrorCode::InvalidPriority.as_str(),
                message: format!(
                    "agd ignores '#+PRIORITIES: {value}' and uses the configured {}: it takes three letters A–Z, highest, lowest and default; numeric priorities are not supported",
                    document.priorities().describe()
                ),
            });
        }

        let effective: std::collections::HashSet<&str> = document
            .todo_active()
            .iter()
            .chain(document.todo_done())
            .map(String::as_str)
            .collect();
        for headline in document.headlines() {
            if let Some(id) = &headline.item.id {
                if let Some((first_file, first_line)) = ids.get(id) {
                    problems.push(CheckProblem {
                        file: display_path.clone(),
                        line: Some(headline.item.line),
                        code: ErrorCode::IdConflict.as_str(),
                        message: format!("ID '{id}' first appears at {first_file}:{first_line}"),
                    });
                } else {
                    ids.insert(id.clone(), (display_path.clone(), headline.item.line));
                }
            }

            if headline.item.todo.is_none()
                && let Some(first) = headline.item.title.split_whitespace().next()
                && configured_keywords.contains(first)
                && !effective.contains(first)
            {
                problems.push(CheckProblem {
                    file: display_path.clone(),
                    line: Some(headline.item.line),
                    code: ErrorCode::InvalidTodo.as_str(),
                    message: format!(
                        "TODO keyword '{first}' is not enabled by this file's #+TODO settings"
                    ),
                });
            }
        }

        let headlines = document.headlines().len();
        headline_count += headlines;
        files.push(CheckFileReport {
            file: display_path,
            headlines,
        });
    }

    Ok(CheckReport {
        schema: CHECK_SCHEMA,
        summary: CheckSummary {
            files: files.len(),
            headlines: headline_count,
            errors: problems.len(),
        },
        files,
        problems,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_check_reports_a_priorities_line_agd_ignores() {
        let directory = tempfile::tempdir().expect("temporary directory");
        fs::write(
            directory.path().join("tasks.org"),
            "#+TITLE: Tasks\n#+PRIORITIES: 1 5 3\n* TODO [#2] Kept\n",
        )
        .expect("fixture");
        let settings = Settings {
            roots: vec![directory.path().to_string_lossy().into_owned()],
            ..Settings::default()
        };
        let report = check_settings(&settings).expect("check runs");
        assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
        let problem = &report.problems[0];
        assert_eq!((problem.code, problem.line), ("invalid_priority", Some(2)));
        assert!(
            problem
                .message
                .starts_with("agd ignores '#+PRIORITIES: 1 5 3' and uses the configured A–C"),
            "{}",
            problem.message
        );
    }

    #[cfg(unix)]
    #[test]
    fn settings_check_reports_symlinks_that_leave_the_roots() {
        let directory = tempfile::tempdir().expect("temporary directory");
        fs::write(directory.path().join("tasks.org"), "* TODO Kept\n").expect("fixture");
        let outside = tempfile::tempdir().expect("outside directory");
        std::os::unix::fs::symlink(outside.path(), directory.path().join("elsewhere"))
            .expect("symlink");
        let settings = Settings {
            roots: vec![directory.path().to_string_lossy().into_owned()],
            ..Settings::default()
        };

        let report = check_settings(&settings).expect("check runs");

        assert_eq!(report.summary.files, 1);
        assert_eq!(report.problems.len(), 1);
        assert_eq!(report.problems[0].code, "invalid_path");
        assert!(report.problems[0].file.ends_with("elsewhere"));
    }
}
