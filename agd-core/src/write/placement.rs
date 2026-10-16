//! Where a capture goes: org-capture's target types (`file`, `file+headline`,
//! `file+olp`, `id`, and `datetree` under them), the year, month and day
//! headings a datetree creates, and org-capture-place-entry's blank-line rules.
//!
//! The result matches what pinned Emacs org-capture writes for the same file,
//! target and day. Besides the entry, only blank lines at the insertion point
//! and the headings a target creates change.

use std::path::Path;
use std::sync::LazyLock;

use chrono::NaiveDate;
use regex::Regex;

use crate::{
    AgdError, ByteSpan, CaptureTemplate, ErrorCode, HeadlineSpan, IndexedDocument, Settings,
};

use super::engine::parse_text;
use super::patch::line_ending;

/// Where a template files its entry, as org-capture's target types.
pub(crate) enum CaptureTarget {
    /// `file`: a top-level entry at the end of the file.
    File,
    /// `file+headline`: under the first heading with this title, created
    /// at the end of the file when missing.
    Heading(String),
    /// `file+olp`: under the heading at this outline path, which must exist
    /// and be unique at every level.
    OutlinePath(Vec<String>),
    /// `id`: under the heading with this `:ID:`.
    Id(String),
}

impl CaptureTarget {
    pub(crate) fn of(template: &CaptureTemplate) -> Self {
        if let Some(id) = &template.id {
            Self::Id(id.clone())
        } else if let Some(outline) = &template.outline_path {
            Self::OutlinePath(outline.clone())
        } else if let Some(heading) = &template.heading {
            Self::Heading(heading.clone())
        } else {
            Self::File
        }
    }
}

/// The target file's text with any headings the capture creates, and where
/// the entry goes in it.
pub(super) struct CapturePlacement {
    pub(super) text: String,
    pub(super) offset: usize,
    pub(super) level: usize,
}

/// org-capture's target location: the base heading (found, or created for
/// `file+headline`), then with `datetree` the year, month and day headings
/// for `today` under it; the entry goes at the end of the last one's
/// subtree, one level down, or at the end of the file.
pub(super) fn place_capture(
    path: &Path,
    source: &str,
    template: &CaptureTemplate,
    today: NaiveDate,
    settings: &Settings,
) -> Result<CapturePlacement, AgdError> {
    let newline = line_ending(source);
    let mut text = source.to_string();
    let document = parse_text(path, &text, settings)?;
    let mut base = match CaptureTarget::of(template) {
        CaptureTarget::File => None,
        CaptureTarget::Heading(title) => Some(match heading_titled(&document, &title) {
            Some(headline) => headline.line_span.start,
            None => append_top_heading(&mut text, &title, newline),
        }),
        CaptureTarget::OutlinePath(outline) => Some(find_outline_path(&document, &outline)?),
        CaptureTarget::Id(id) => Some(
            document
                .headlines()
                .iter()
                .find(|headline| headline.item.id.as_deref() == Some(id.as_str()))
                .map(|headline| headline.line_span.start)
                .ok_or_else(|| {
                    AgdError::new(
                        ErrorCode::CaptureTargetNotFound,
                        format!("no heading in {} has ID '{id}'", path.display()),
                    )
                })?,
        ),
    };
    if template.datetree {
        base = Some(find_or_create_datetree(
            &mut text, base, today, newline, path, settings,
        )?);
    }
    let Some(base) = base else {
        let offset = text.len();
        return Ok(CapturePlacement {
            text,
            offset,
            level: 1,
        });
    };
    let document = parse_text(path, &text, settings)?;
    let headline = headline_starting_at(&document, base)?;
    Ok(CapturePlacement {
        offset: headline.subtree_span.end,
        level: headline.level + 1,
        text,
    })
}

/// A `file+headline` target: the first heading with this title, at any
/// level, as org-capture finds it.
pub(crate) fn heading_titled<'a>(
    document: &'a IndexedDocument,
    title: &str,
) -> Option<&'a HeadlineSpan> {
    document
        .headlines()
        .iter()
        .find(|headline| headline.item.title == title)
}

/// Appends `* title` as org-capture does for a missing `file+headline`
/// target, and returns where the heading starts.
fn append_top_heading(text: &mut String, title: &str, newline: &str) -> usize {
    if !text.is_empty() && !text.ends_with('\n') {
        text.push_str(newline);
    }
    let start = text.len();
    text.push_str(&format!("* {title}{newline}"));
    start
}

/// org-find-olp: each title must name exactly one heading on its level,
/// the first at the top level and each next one a direct child.
pub(crate) fn find_outline_path(
    document: &IndexedDocument,
    outline: &[String],
) -> Result<usize, AgdError> {
    let mut region = ByteSpan {
        start: 0,
        end: document.source().len(),
    };
    let mut level = 1;
    let mut found = None;
    for title in outline {
        let matches: Vec<&HeadlineSpan> = document
            .headlines()
            .iter()
            .filter(|headline| {
                headline.level == level
                    && headline.item.title == *title
                    && region.start <= headline.line_span.start
                    && headline.line_span.start < region.end
            })
            .collect();
        let headline = match matches.as_slice() {
            [headline] => *headline,
            [] => {
                return Err(AgdError::new(
                    ErrorCode::CaptureTargetNotFound,
                    format!("capture heading not found on level {level}: {title}"),
                ));
            }
            _ => {
                return Err(AgdError::new(
                    ErrorCode::CaptureTargetNotFound,
                    format!("capture heading not unique on level {level}: {title}"),
                ));
            }
        };
        region = headline.subtree_span;
        level = headline.level + 1;
        found = Some(headline.line_span.start);
    }
    found.ok_or_else(|| {
        AgdError::new(
            ErrorCode::CaptureTargetNotFound,
            "capture outline path is empty",
        )
    })
}

/// Datetree headings compare by the date in their title (Emacs's
/// org-datetree-comparefun-from-regex); titles without one are skipped.
static DATETREE_YEAR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([12][0-9]{3})").expect("datetree year regex"));
static DATETREE_MONTH_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([12][0-9]{3}-[01][0-9]) \w+").expect("datetree month regex"));
static DATETREE_DAY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"([12][0-9]{3}-[01][0-9]-[0123][0-9]) \w+").expect("datetree day regex")
});

/// org-datetree-find-date-create: finds or creates `* 2026`, `** 2026-10
/// October` and `*** 2026-10-16 Friday` under `base` (or at the top of the
/// file), each in date order among its siblings, and returns where the day
/// heading starts.
fn find_or_create_datetree(
    text: &mut String,
    base: Option<usize>,
    date: NaiveDate,
    newline: &str,
    path: &Path,
    settings: &Settings,
) -> Result<usize, AgdError> {
    let levels: [(String, &Regex); 3] = [
        (date.format("%Y").to_string(), &DATETREE_YEAR_RE),
        (date.format("%Y-%m %B").to_string(), &DATETREE_MONTH_RE),
        (date.format("%Y-%m-%d %A").to_string(), &DATETREE_DAY_RE),
    ];
    let mut parent = base;
    for (title, key) in levels {
        let heading = DatetreeHeading {
            title: &title,
            key,
            newline,
        };
        parent = Some(find_or_create_datetree_heading(
            text, parent, &heading, path, settings,
        )?);
    }
    Ok(parent.expect("the day heading"))
}

struct DatetreeHeading<'a> {
    title: &'a str,
    key: &'a Regex,
    newline: &'a str,
}

/// org-datetree--find-create-subheading within the parent's subtree as
/// org-narrow-to-subtree narrows it (or the whole file): the first
/// sibling whose date is not earlier is the heading itself or the one to
/// insert before; otherwise the heading goes at the end of the region.
/// Returns the heading's start.
fn find_or_create_datetree_heading(
    text: &mut String,
    parent: Option<usize>,
    heading: &DatetreeHeading<'_>,
    path: &Path,
    settings: &Settings,
) -> Result<usize, AgdError> {
    let document = parse_text(path, text, settings)?;
    let (region, level) = match parent {
        None => ((0, text.len()), 1),
        Some(start) => {
            let parent = headline_starting_at(&document, start)?;
            (
                narrowed_subtree(parent, text.len(), heading.newline),
                parent.level + 1,
            )
        }
    };
    let key = datetree_key(heading.key, heading.title).unwrap_or_default();
    let sibling = document
        .headlines()
        .iter()
        .filter(|headline| {
            headline.level == level
                && region.0 <= headline.line_span.start
                && headline.line_span.start < region.1
        })
        .find_map(|headline| {
            datetree_key(heading.key, &headline.item.title)
                .filter(|sibling| *sibling >= key)
                .map(|sibling| (headline.line_span.start, sibling == key))
        });
    if let Some((start, true)) = sibling {
        return Ok(start);
    }
    let point = sibling.map_or(region.1, |(start, _)| start);
    let deletion_start = region.0.max(
        text[..point]
            .trim_end_matches([' ', '\t', '\n', '\r'])
            .len(),
    );
    text.replace_range(deletion_start..point, "");
    let region_end = region.1 - (point - deletion_start);
    let blank = blank_before_heading(text, (region.0, region_end), deletion_start);
    let line = format!("{} {}{}", "*".repeat(level), heading.title, heading.newline);
    let insertion = format!(
        "{}{}{line}",
        if blank { heading.newline } else { "" },
        heading.newline
    );
    text.insert_str(deletion_start, &insertion);
    Ok(deletion_start + insertion.len() - line.len())
}

fn datetree_key(key: &Regex, title: &str) -> Option<String> {
    key.captures(title)
        .and_then(|captures| captures.get(1))
        .map(|matched| matched.as_str().to_string())
}

/// The region org-narrow-to-subtree leaves: the subtree without its final
/// newline unless it ends the file.
fn narrowed_subtree(headline: &HeadlineSpan, text_len: usize, newline: &str) -> (usize, usize) {
    let end = headline.subtree_span.end;
    let end = if end == text_len {
        end
    } else {
        end.saturating_sub(newline.len())
            .max(headline.line_span.start)
    };
    (headline.subtree_span.start, end)
}

/// Inserts the entry as org-capture-place-entry does with no
/// `:empty-lines`: blank lines before the insertion point are removed, one
/// is added when Org wants a blank line before a new heading here
/// (org--blank-before-heading-p) and the line before is not already blank,
/// and blank lines right after the entry are removed. Returns the new text
/// and where the entry starts.
pub(super) fn insert_capture_entry(
    text: &str,
    offset: usize,
    entry: &str,
    newline: &str,
) -> (String, usize) {
    let mut before = text[..offset].to_string();
    let after = &text[offset..];
    if !before.is_empty() && !before.ends_with('\n') {
        before.push_str(newline);
    }
    let whole = format!("{before}{after}");
    let point = before.len();
    let blank = blank_before_heading(&whole, (0, whole.len()), point)
        && !previous_line_blank(&whole, 0, point);
    before.truncate(back_over_empty_lines(&before, point));
    if blank {
        before.push_str(newline);
    }
    let entry_start = before.len();
    let after = trim_leading_blank_lines(after);
    (format!("{before}{entry}{after}"), entry_start)
}

/// org-back-over-empty-lines: the start of the blank lines that end
/// `text[..pos]`.
fn back_over_empty_lines(text: &str, pos: usize) -> usize {
    let content_end = text[..pos].trim_end_matches([' ', '\t', '\n', '\r']).len();
    let next_line = text[content_end..]
        .find('\n')
        .map_or(text.len(), |index| content_end + index + 1);
    next_line.min(pos)
}

fn trim_leading_blank_lines(mut text: &str) -> &str {
    loop {
        let line_end = text.find('\n');
        match line_end {
            Some(index) if text[..index].trim_matches([' ', '\t', '\r']).is_empty() => {
                text = &text[index + 1..];
            }
            _ => return text,
        }
    }
}

/// org--blank-before-heading-p with org-blank-before-new-entry's `auto`,
/// within `region`: whether the heading at or before `point` (else the
/// next one) is preceded by a blank line. The region's first heading
/// defers to the heading after it.
fn blank_before_heading(text: &str, region: (usize, usize), point: usize) -> bool {
    let headings = heading_line_starts(text, region);
    let heading = headings
        .iter()
        .rev()
        .find(|start| **start <= point)
        .or_else(|| headings.iter().find(|start| **start > point));
    let Some(&heading) = heading else {
        return false;
    };
    if heading != region.0 {
        return previous_line_blank(text, region.0, heading);
    }
    headings
        .iter()
        .find(|start| **start > heading)
        .is_some_and(|next| previous_line_blank(text, region.0, *next))
}

/// Starts of the heading lines (`*` followed by a space or tab) that begin
/// inside `region`.
pub(super) fn heading_line_starts(text: &str, region: (usize, usize)) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        if start < region.0 || start >= region.1 {
            continue;
        }
        let stars = line.bytes().take_while(|byte| *byte == b'*').count();
        if stars > 0 && matches!(line.as_bytes().get(stars), Some(b' ' | b'\t')) {
            starts.push(start);
        }
    }
    starts
}

/// org-previous-line-empty-p for the line starting at `line_start`.
fn previous_line_blank(text: &str, region_start: usize, line_start: usize) -> bool {
    if line_start <= region_start {
        return false;
    }
    let previous_end = line_start - 1;
    let previous_start = text[..previous_end]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    text[previous_start.max(region_start)..previous_end]
        .trim_matches([' ', '\t', '\r'])
        .is_empty()
}

fn headline_starting_at(
    document: &IndexedDocument,
    start: usize,
) -> Result<&HeadlineSpan, AgdError> {
    document
        .headlines()
        .iter()
        .find(|headline| headline.line_span.start == start)
        .ok_or_else(|| {
            AgdError::new(
                ErrorCode::ParseFailed,
                "capture target heading moved while placing",
            )
        })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use super::*;
    use crate::write::test_support::{context, settings};
    use crate::{CaptureDraft, WriteContext, WriteEngine};

    /// Captures `* TODO Entry` on 2026-10-16 into `input` (None: no file)
    /// through a template with this target, returning the file or the error
    /// code.
    fn capture_into(
        input: Option<&str>,
        heading: Option<&str>,
        outline: Option<&[&str]>,
        datetree: bool,
    ) -> Result<String, String> {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("input.org");
        if let Some(input) = input {
            fs::write(&path, input).unwrap();
        }
        let mut settings = settings(temp.path());
        settings.captures = vec![CaptureTemplate {
            key: "t".to_string(),
            file: Some("input.org".to_string()),
            heading: heading.map(str::to_string),
            outline_path: outline.map(|path| path.iter().map(|title| title.to_string()).collect()),
            datetree,
            template: "* TODO Entry".to_string(),
            ..CaptureTemplate::default()
        }];
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl")).unwrap();
        let context = WriteContext {
            now: Some(
                NaiveDate::from_ymd_opt(2026, 10, 16)
                    .unwrap()
                    .and_hms_opt(14, 22, 0)
                    .unwrap(),
            ),
            ..context()
        };
        let draft = CaptureDraft {
            title: "Entry".to_string(),
            ..CaptureDraft::default()
        };
        let preview = engine.capture("t", &draft, "", &BTreeMap::new(), true, &context);
        let written = engine.capture("t", &draft, "", &BTreeMap::new(), false, &context);
        match (preview, written) {
            (Ok(preview), Ok(written)) => {
                assert_eq!(preview.diff, written.diff);
                assert_eq!(written.item.title, "Entry");
                Ok(fs::read_to_string(&path).unwrap())
            }
            (Err(preview), Err(written)) => {
                assert_eq!(preview.code(), written.code());
                assert_eq!(fs::read_to_string(&path).ok().as_deref(), input);
                Err(written.code().to_string())
            }
            (preview, written) => panic!("dry run and write disagree: {preview:?} {written:?}"),
        }
    }

    // Expected bytes are Emacs 31.1 / Org 9.8.7 org-capture results for the
    // same file, target and day (template "* TODO Entry", :immediate-finish).
    #[test]
    fn headline_and_outline_targets_place_entries_as_org_capture() {
        let ok = |text: &str| Ok(text.to_string());
        let inbox = Some("Inbox");
        assert_eq!(
            capture_into(None, inbox, None, false),
            ok("* Inbox\n** TODO Entry\n")
        );
        assert_eq!(
            capture_into(Some(""), inbox, None, false),
            ok("* Inbox\n** TODO Entry\n")
        );
        assert_eq!(
            capture_into(Some("* Other\nbody"), inbox, None, false),
            ok("* Other\nbody\n* Inbox\n** TODO Entry\n")
        );
        assert_eq!(
            capture_into(Some("* A\n\n"), inbox, None, false),
            ok("* A\n\n* Inbox\n\n** TODO Entry\n")
        );
        assert_eq!(
            capture_into(Some("* Inbox\n** a\n\n* Next\n"), inbox, None, false),
            ok("* Inbox\n** a\n** TODO Entry\n* Next\n")
        );
        assert_eq!(
            capture_into(Some("* A\n\n\n"), None, None, false),
            ok("* A\n* TODO Entry\n")
        );
        let olp: &[&str] = &["Projects", "Inbox"];
        assert_eq!(
            capture_into(
                Some("* Projects\n** Inbox\n*** a\n\n* Other\n"),
                None,
                Some(olp),
                false
            ),
            ok("* Projects\n** Inbox\n*** a\n*** TODO Entry\n* Other\n")
        );
        assert_eq!(
            capture_into(Some("* Projects\n"), None, Some(olp), false),
            Err("capture_target_not_found".to_string())
        );
    }

    #[test]
    fn datetree_targets_create_year_month_and_day_as_org_capture() {
        let ok = |text: &str| Ok(text.to_string());
        let tree = "* 2026\n** 2026-10 October\n*** 2026-10-16 Friday\n**** TODO Entry\n";
        assert_eq!(
            capture_into(None, None, None, true),
            ok(&format!("\n{tree}"))
        );
        assert_eq!(
            capture_into(Some(""), None, None, true),
            ok(&format!("\n{tree}"))
        );
        assert_eq!(
            capture_into(Some("* Inbox\n** TODO a\n"), None, None, true),
            ok(&format!("* Inbox\n** TODO a\n{tree}"))
        );
        assert_eq!(
            capture_into(
                Some("#+TITLE: J\n\n* Notes\n\nbody\n\n\n"),
                None,
                None,
                true
            ),
            ok(&format!("#+TITLE: J\n\n* Notes\n\nbody\n\n{tree}"))
        );
        assert_eq!(
            capture_into(
                Some(
                    "* 2026\n** 2026-10 October\n*** 2026-10-14 Wednesday\n**** a\n*** 2026-10-20 Tuesday\n**** b\n"
                ),
                None,
                None,
                true
            ),
            ok(
                "* 2026\n** 2026-10 October\n*** 2026-10-14 Wednesday\n**** a\n*** 2026-10-16 Friday\n**** TODO Entry\n*** 2026-10-20 Tuesday\n**** b\n"
            )
        );
        assert_eq!(
            capture_into(
                Some("* 2025\n** 2025-12 December\n* 2027\n"),
                None,
                None,
                true
            ),
            ok(
                "* 2025\n** 2025-12 December\n* 2026\n** 2026-10 October\n*** 2026-10-16 Friday\n**** TODO Entry\n* 2027\n"
            )
        );
        assert_eq!(
            capture_into(
                Some(
                    "* 2026\n\n** 2026-09 September\n\n*** 2026-09-01 Tuesday\n\n** 2026-11 November\n\n"
                ),
                None,
                None,
                true
            ),
            ok(
                "* 2026\n\n** 2026-09 September\n\n*** 2026-09-01 Tuesday\n\n** 2026-10 October\n*** 2026-10-16 Friday\n**** TODO Entry\n** 2026-11 November\n\n"
            )
        );
        assert_eq!(
            capture_into(
                Some(
                    "* 2026\n** 2026-10 October\n*** 2026-10-16 Friday\n**** a\n\n*** 2026-10-17 Saturday\n"
                ),
                None,
                None,
                true
            ),
            ok(
                "* 2026\n** 2026-10 October\n*** 2026-10-16 Friday\n**** a\n**** TODO Entry\n*** 2026-10-17 Saturday\n"
            )
        );
        assert_eq!(
            capture_into(
                Some("* Inbox\n\n* 2025\n\n** 2025-01 January\n"),
                None,
                None,
                true
            ),
            ok(&format!(
                "* Inbox\n\n* 2025\n\n** 2025-01 January\n\n{tree}"
            ))
        );
        let journal: &[&str] = &["Journal"];
        assert_eq!(
            capture_into(
                Some("* Inbox\n* Journal\n** 2025\n* After\n"),
                None,
                Some(journal),
                true
            ),
            ok(
                "* Inbox\n* Journal\n** 2025\n** 2026\n*** 2026-10 October\n**** 2026-10-16 Friday\n***** TODO Entry\n* After\n"
            )
        );
        assert_eq!(
            capture_into(
                Some("* Journal\n** 2026\n*** 2026-09 September\n* After\n"),
                None,
                Some(journal),
                true
            ),
            ok(
                "* Journal\n** 2026\n*** 2026-09 September\n*** 2026-10 October\n**** 2026-10-16 Friday\n***** TODO Entry\n* After\n"
            )
        );
        assert_eq!(
            capture_into(Some("* Inbox\n"), None, Some(journal), true),
            Err("capture_target_not_found".to_string())
        );
    }

    #[test]
    fn an_id_target_captures_under_that_heading_in_any_agenda_file() {
        let temp = tempfile::tempdir().unwrap();
        let projects = temp.path().join("projects.org");
        fs::write(
            &projects,
            "* Launch\n:PROPERTIES:\n:ID: launch-id\n:END:\n** TODO Existing\n* Other\n",
        )
        .unwrap();
        let mut settings = settings(temp.path());
        settings.captures = vec![
            CaptureTemplate {
                key: "l".to_string(),
                file: None,
                heading: None,
                id: Some("launch-id".to_string()),
                ..CaptureTemplate::default()
            },
            CaptureTemplate {
                key: "m".to_string(),
                file: None,
                heading: None,
                id: Some("missing-id".to_string()),
                ..CaptureTemplate::default()
            },
        ];
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl")).unwrap();
        let draft = CaptureDraft {
            title: "Under launch".to_string(),
            ..CaptureDraft::default()
        };
        let mutation = engine
            .capture("l", &draft, "", &BTreeMap::new(), false, &context())
            .unwrap();
        assert_eq!(mutation.item.olpath, ["Launch", "Under launch"]);
        let written = fs::read_to_string(&projects).unwrap();
        assert!(
            written.starts_with("* Launch\n:PROPERTIES:\n:ID: launch-id\n:END:\n** TODO Existing\n** TODO Under launch\n"),
            "{written}"
        );
        assert!(written.ends_with("* Other\n"));
        let error = engine
            .capture("m", &draft, "", &BTreeMap::new(), false, &context())
            .unwrap_err();
        assert_eq!(error.code().as_str(), "capture_target_not_found");
    }
}
