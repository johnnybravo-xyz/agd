//! `agd setup check`: a read-only report ([`CheckReport`], schema
//! `agd.check/v1`) on the org files under the roots and on the configuration:
//! files that cannot be read or parsed, files the Org parser would not
//! reproduce byte for byte, repeated `:ID:`s, TODO keywords a file uses
//! without enabling them, and interrupted writes.
//!
//! It parses with its own light [`OrgDocument`] (`agd_core::OrgDocument`),
//! separate from the index's [`IndexedDocument`](crate::IndexedDocument), and
//! never writes. Its problem `code`s (`read_error`, `invalid_org`,
//! `byte_drift`, `non_utf8`, `duplicate_id`, `unknown_todo_keyword`,
//! `outside_roots`, `journal_conflict`) belong to agd.check/v1, not to
//! [`ErrorCode`](crate::ErrorCode).

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::string::FromUtf8Error;

use crate::{AgdError, CHECK_SCHEMA, JournalRecord, RootSet, Settings};
use ignore::WalkBuilder;
use orgize::{Org, ParseConfig, ast::Headline as OrgHeadline, rowan::ast::AstNode};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceSpan {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexedHeadline {
    pub line: usize,
    pub level: usize,
    pub span: SourceSpan,
    pub todo_keyword: Option<SourceSpan>,
    pub id: Option<SourceSpan>,
}

pub struct OrgDocument {
    source: String,
    org: Org,
    headlines: Vec<IndexedHeadline>,
}

impl OrgDocument {
    pub fn parse(bytes: Vec<u8>) -> Result<Self, DocumentParseError> {
        let source = String::from_utf8(bytes).map_err(DocumentParseError::InvalidUtf8)?;
        let org = parse_org(&source);
        let headlines = index_headlines(&source, &org);

        Ok(Self {
            source,
            org,
            headlines,
        })
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn headlines(&self) -> &[IndexedHeadline] {
        &self.headlines
    }

    pub fn text(&self, span: SourceSpan) -> &str {
        &self.source[span.start..span.end]
    }

    pub fn round_trips_exactly(&self) -> bool {
        self.org.to_org() == self.source
    }
}

#[derive(Debug)]
pub enum DocumentParseError {
    InvalidUtf8(FromUtf8Error),
}

impl fmt::Display for DocumentParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUtf8(_) => formatter.write_str("file is not valid UTF-8"),
        }
    }
}

impl std::error::Error for DocumentParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidUtf8(error) => Some(error),
        }
    }
}

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
            code: "journal_conflict",
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

pub fn check_paths(roots: &[PathBuf]) -> CheckReport {
    let mut paths = Vec::new();
    let mut problems = Vec::new();

    for root in roots {
        collect_files(root, &mut paths, &mut problems);
    }
    paths.sort_unstable();
    paths.dedup();

    let mut files = Vec::with_capacity(paths.len());
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
                    code: "read_error",
                    message: error.to_string(),
                });
                continue;
            }
        };
        let document = match OrgDocument::parse(bytes) {
            Ok(document) => document,
            Err(error) => {
                problems.push(CheckProblem {
                    file: display_path,
                    line: None,
                    code: "invalid_org",
                    message: error.to_string(),
                });
                continue;
            }
        };

        if !document.round_trips_exactly() {
            problems.push(CheckProblem {
                file: display_path.clone(),
                line: None,
                code: "byte_drift",
                message: "orgize did not reproduce the input byte-for-byte".to_owned(),
            });
        }

        for headline in document.headlines() {
            let Some(id_span) = headline.id else {
                continue;
            };
            let id = document.text(id_span);
            if let Some((first_file, first_line)) = ids.get(id) {
                problems.push(CheckProblem {
                    file: display_path.clone(),
                    line: Some(headline.line),
                    code: "duplicate_id",
                    message: format!("ID {id:?} first appears at {first_file}:{first_line}"),
                });
            } else {
                ids.insert(id.to_owned(), (display_path.clone(), headline.line));
            }
        }

        let headlines = document.headlines().len();
        headline_count += headlines;
        files.push(CheckFileReport {
            file: display_path,
            headlines,
        });
    }

    CheckReport {
        schema: CHECK_SCHEMA,
        summary: CheckSummary {
            files: files.len(),
            headlines: headline_count,
            errors: problems.len(),
        },
        files,
        problems,
    }
}

pub fn check_settings(settings: &Settings) -> Result<CheckReport, AgdError> {
    let roots = RootSet::new(&settings.roots, &settings.agenda_files)?;
    let org_files = roots.all_org_files()?;
    let paths = org_files.files;
    let configured_keywords: std::collections::HashSet<&str> = settings
        .todo_keywords
        .iter()
        .flat_map(|line| line.split_whitespace())
        .filter(|keyword| *keyword != "|")
        .filter_map(|keyword| keyword.split('(').next())
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
                    code: "read_error",
                    message: error.to_string(),
                });
                continue;
            }
        };
        let document = match crate::document::OrgDocument::parse(path, bytes, settings) {
            Ok(document) => document,
            Err(error) => {
                let code = match error.code() {
                    "non_utf8" => "non_utf8",
                    "parse_drift" => "byte_drift",
                    _ => "invalid_org",
                };
                problems.push(CheckProblem {
                    file: display_path,
                    line: None,
                    code,
                    message: error.to_string(),
                });
                continue;
            }
        };

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
                        code: "duplicate_id",
                        message: format!("ID {id:?} first appears at {first_file}:{first_line}"),
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
                    code: "unknown_todo_keyword",
                    message: format!(
                        "TODO keyword {first:?} is not enabled by this file's #+TODO settings"
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

fn parse_org(source: &str) -> Org {
    let Some(todo_keywords) = todo_keywords(source) else {
        return Org::parse(source);
    };

    ParseConfig {
        todo_keywords,
        ..ParseConfig::default()
    }
    .parse(source)
}

fn todo_keywords(source: &str) -> Option<(Vec<String>, Vec<String>)> {
    let mut todo = Vec::new();
    let mut done = Vec::new();
    let mut found = false;

    for line in source.lines() {
        let Some(value) =
            directive_value(line, "TODO").or_else(|| directive_value(line, "SEQ_TODO"))
        else {
            continue;
        };
        found = true;
        let sequence_start = todo.len();
        let mut after_separator = false;

        for token in value.split_ascii_whitespace() {
            if token == "|" {
                after_separator = true;
                continue;
            }
            let keyword = token.split_once('(').map_or(token, |(keyword, _)| keyword);
            if keyword.is_empty() {
                continue;
            }
            if after_separator {
                done.push(keyword.to_owned());
            } else {
                todo.push(keyword.to_owned());
            }
        }

        if !after_separator
            && todo.len() > sequence_start
            && let Some(last) = todo.pop()
        {
            done.push(last);
        }
    }

    found.then_some((todo, done))
}

fn directive_value<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let rest = line.strip_prefix("#+")?;
    let (actual_name, value) = rest.split_once(':')?;
    actual_name
        .eq_ignore_ascii_case(name)
        .then(|| value.trim_ascii())
}

fn index_headlines(source: &str, org: &Org) -> Vec<IndexedHeadline> {
    let mut headlines = Vec::new();
    let mut scanned_to = 0;
    let mut line = 1;

    for headline in org
        .document()
        .syntax()
        .descendants()
        .filter_map(OrgHeadline::cast)
    {
        let start = usize::from(headline.start());
        line += source.as_bytes()[scanned_to..start]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count();
        scanned_to = start;

        let range = headline.text_range();
        let todo_keyword = headline.todo_keyword().map(|token| SourceSpan {
            start: usize::from(token.start()),
            end: usize::from(token.end()),
        });
        let id = headline
            .properties()
            .and_then(|properties| properties.get("ID"))
            .map(|token| SourceSpan {
                start: usize::from(token.start()),
                end: usize::from(token.end()),
            });

        headlines.push(IndexedHeadline {
            line,
            level: headline.level(),
            span: SourceSpan {
                start: usize::from(range.start()),
                end: usize::from(range.end()),
            },
            todo_keyword,
            id,
        });
    }

    headlines
}

fn collect_files(root: &Path, files: &mut Vec<PathBuf>, problems: &mut Vec<CheckProblem>) {
    let metadata = match fs::metadata(root) {
        Ok(metadata) => metadata,
        Err(error) => {
            problems.push(CheckProblem {
                file: root.to_string_lossy().into_owned(),
                line: None,
                code: "read_error",
                message: error.to_string(),
            });
            return;
        }
    };

    if metadata.is_file() {
        files.push(root.to_path_buf());
        return;
    }
    if !metadata.is_dir() {
        problems.push(CheckProblem {
            file: root.to_string_lossy().into_owned(),
            line: None,
            code: "read_error",
            message: "path is neither a regular file nor a directory".to_owned(),
        });
        return;
    }

    for entry in WalkBuilder::new(root).follow_links(false).build() {
        match entry {
            Ok(entry)
                if entry.file_type().is_some_and(|kind| kind.is_file())
                    && is_org_file(entry.path()) =>
            {
                files.push(entry.into_path());
            }
            Ok(_) => {}
            Err(error) => problems.push(CheckProblem {
                file: root.to_string_lossy().into_owned(),
                line: None,
                code: "read_error",
                message: error.to_string(),
            }),
        }
    }
}

fn is_org_file(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("org"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_multiple_in_file_todo_sequences_and_ids() {
        let source = b"#+TODO: OPEN(o) NEXT | CLOSED(c)\n#+SEQ_TODO: WAIT(w@/!) SOMEDAY | CANCELED(c@/!)\n\n* OPEN Parent\n:PROPERTIES:\n:ID: parent-id\n:END:\n** CANCELED Child\n"
            .to_vec();

        let document = OrgDocument::parse(source).expect("valid document");
        let headlines = document.headlines();

        assert_eq!(headlines.len(), 2);
        assert_eq!((headlines[0].line, headlines[0].level), (4, 1));
        assert_eq!(
            headlines[0].todo_keyword.map(|span| document.text(span)),
            Some("OPEN")
        );
        assert_eq!(
            headlines[0].id.map(|span| document.text(span)),
            Some("parent-id")
        );
        assert_eq!((headlines[1].line, headlines[1].level), (8, 2));
        assert_eq!(
            headlines[1].todo_keyword.map(|span| document.text(span)),
            Some("CANCELED")
        );
    }

    #[test]
    fn treats_the_last_state_as_done_without_a_separator() {
        let document = OrgDocument::parse(b"#+TODO: OPEN FINISHED\n* FINISHED Task\n".to_vec())
            .expect("valid document");
        let headline = document.org.first_node::<OrgHeadline>().expect("headline");

        assert_eq!(headline.todo_type(), Some(orgize::ast::TodoType::Done));
    }

    #[test]
    fn preserves_crlf_input_exactly() {
        let source =
            b"#+CATEGORY: work\r\n* TODO Task :tag:\r\nSCHEDULED: <2026-10-13 Tue>\r\n".to_vec();
        let expected = source.clone();

        let document = OrgDocument::parse(source).expect("valid document");

        assert_eq!(document.source().as_bytes(), expected);
        assert!(document.round_trips_exactly());
    }

    #[test]
    fn rejects_non_utf8_input() {
        let error = OrgDocument::parse(vec![b'*', b' ', 0xff])
            .err()
            .expect("parse error");

        assert_eq!(error.to_string(), "file is not valid UTF-8");
    }

    #[test]
    fn reports_duplicate_ids_across_org_files() {
        let directory = tempfile::tempdir().expect("temporary directory");
        fs::write(
            directory.path().join("one.org"),
            "* First\n:PROPERTIES:\n:ID: duplicate\n:END:\n",
        )
        .expect("first fixture");
        fs::write(
            directory.path().join("two.org"),
            "* Second\n:PROPERTIES:\n:ID: duplicate\n:END:\n",
        )
        .expect("second fixture");
        fs::write(directory.path().join("ignored.txt"), "* Not org\n").expect("ignored fixture");

        let report = check_paths(&[directory.path().to_path_buf()]);

        assert_eq!(report.summary.files, 2);
        assert_eq!(report.summary.headlines, 2);
        assert_eq!(report.summary.errors, 1);
        assert_eq!(report.problems[0].code, "duplicate_id");
        assert_eq!(report.problems[0].line, Some(1));
        assert!(report.problems[0].message.contains("one.org:1"));
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
        assert_eq!(report.problems[0].code, "outside_roots");
        assert!(report.problems[0].file.ends_with("elsewhere"));
    }
}
