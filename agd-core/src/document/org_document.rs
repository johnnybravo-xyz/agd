//! `OrgDocument`: one org file parsed from its bytes, keeping the source
//! exactly, with its headlines and the TODO keywords, logging, priorities, file
//! tags and category its directives give.
//!
//! It refuses a file that is not UTF-8 (`invalid_utf8`) or that orgize would
//! not round-trip byte for byte (`parse_failed`), so every span points into bytes
//! that are exactly the file's.

use std::path::{Path, PathBuf};

use orgize::ParseConfig;

use crate::{AgdError, ErrorCode, Settings};

use super::directive::{
    LogSettings, PriorityRange, TodoKeyword, directive, log_settings, priority_range, todo_keywords,
};
use super::headline::{HeadlineSpan, parse_colon_tags, scan_headlines};
use super::source::source_lines;

#[derive(Clone, Debug)]
pub struct OrgDocument {
    path: PathBuf,
    source: String,
    pub(super) headlines: Vec<HeadlineSpan>,
    todo_active: Vec<String>,
    todo_done: Vec<String>,
    todo_keywords: Vec<TodoKeyword>,
    logging: LogSettings,
    priorities: PriorityRange,
}

impl OrgDocument {
    pub fn parse(path: PathBuf, bytes: Vec<u8>, settings: &Settings) -> Result<Self, AgdError> {
        let source = String::from_utf8(bytes).map_err(|error| {
            AgdError::with_source(
                ErrorCode::InvalidUtf8,
                format!("org file is not UTF-8: {}", path.display()),
                error,
            )
        })?;
        let lines = source_lines(&source);
        let todo_keywords = todo_keywords(&source, settings);
        let todo_active: Vec<String> = todo_keywords
            .iter()
            .filter(|keyword| !keyword.done)
            .map(|keyword| keyword.name.clone())
            .collect();
        let todo_done: Vec<String> = todo_keywords
            .iter()
            .filter(|keyword| keyword.done)
            .map(|keyword| keyword.name.clone())
            .collect();
        let logging = log_settings(&source, settings);
        let priorities = priority_range(&source, settings);
        let file_tags = directive(&source, "FILETAGS")
            .map(parse_colon_tags)
            .unwrap_or_default();
        let file_category = directive(&source, "CATEGORY")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or("org")
                    .to_string()
            });

        let parse_config = ParseConfig {
            todo_keywords: (todo_active.clone(), todo_done.clone()),
            ..Default::default()
        };
        let parsed = parse_config.parse(&source);
        if parsed.to_org() != source {
            return Err(AgdError::new(
                ErrorCode::ParseFailed,
                format!(
                    "orgize changed bytes during a no-op round trip: {}",
                    path.display()
                ),
            ));
        }

        let raw = scan_headlines(
            &path,
            &source,
            &lines,
            &todo_active,
            &todo_done,
            &file_tags,
            &file_category,
            settings,
        )?;
        Ok(Self {
            path,
            source,
            headlines: raw,
            todo_active,
            todo_done,
            todo_keywords,
            logging,
            priorities,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn headlines(&self) -> &[HeadlineSpan] {
        &self.headlines
    }

    pub fn headline_at_line(&self, line: usize) -> Option<&HeadlineSpan> {
        self.headlines
            .iter()
            .find(|headline| headline.item.line == line)
    }

    pub fn todo_active(&self) -> &[String] {
        &self.todo_active
    }

    pub fn todo_done(&self) -> &[String] {
        &self.todo_done
    }

    pub fn todo_keywords(&self) -> &[TodoKeyword] {
        &self.todo_keywords
    }

    pub fn todo_keyword(&self, name: &str) -> Option<&TodoKeyword> {
        self.todo_keywords
            .iter()
            .find(|keyword| keyword.name == name)
    }

    pub fn logging(&self) -> LogSettings {
        self.logging
    }

    /// This file's priority range: `#+PRIORITIES`, else the configured one.
    pub fn priorities(&self) -> PriorityRange {
        self.priorities
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::test_support::settings;

    #[test]
    fn indexes_refs_inheritance_and_in_file_todo_without_drift() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("work.org");
        let source = "#+TODO: OPEN(o) | SHUT(s!)\r\n#+FILETAGS: :work:\r\n* Parent :project:\r\n** OPEN [#A] Task :report:\r\nSCHEDULED: <2026-10-13 Tue +1w> DEADLINE: <2026-10-16 Fri -5d>\r\n:PROPERTIES:\r\n:ID: exact-id\r\n:CATEGORY: reporting\r\n:END:\r\nbody unchanged\r\n";
        std::fs::write(&path, source).unwrap();
        let document = OrgDocument::parse(
            path.clone(),
            std::fs::read(&path).unwrap(),
            &settings(temp.path()),
        )
        .unwrap();
        assert_eq!(document.source(), source);
        let item = &document.headlines()[1].item;
        assert_eq!(item.item_ref, "exact-id");
        assert_eq!(item.todo.as_deref(), Some("OPEN"));
        assert_eq!(item.priority.as_deref(), Some("A"));
        // File tags first, then the parent's, as Org lists them.
        assert_eq!(item.inherited_tags, vec!["work", "project"]);
        assert_eq!(item.category, "reporting");
        assert_eq!(
            item.planning.deadline.as_ref().unwrap().warning.as_deref(),
            Some("-5d")
        );
    }
}
