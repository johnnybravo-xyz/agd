//! Headlines as the index sees them: `HeadlineSpan`, holding the `Item`, the
//! level and the byte spans of the headline, planning line, property drawer,
//! section and subtree, and the scan that builds them with tag and category
//! inheritance.
//!
//! Planning is read only from the line right after the headline, as Org reads
//! it. Tags and priority cookies are split off exactly as the parser reads
//! them (`split_trailing_tags`, `leading_priority_cookie`); writes use the same
//! functions to refuse a title that would read back differently.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use sha2::{Digest, Sha256};

use crate::{AgdError, ErrorCode, ITEM_SCHEMA, Item, Planning, PlanningTimestamp, Settings};

use super::source::{ByteSpan, SourceLine};
use super::timestamp::{TimestampSpan, parse_timestamp, scan_timestamps};

static HEADLINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\*+)[ \t]+(.*)$").expect("headline regex"));
static TAGS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|[ \t]+)(:[\w@#%:]+:)[ \t]*$").expect("tags regex"));
static PRIORITY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[#([[:alnum:]])\](?:[ \t]+|$)").expect("priority regex"));
static PROPERTY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^:([[:alnum:]_-]+):(?:[ \t]*(.*))?$").expect("property regex"));

#[derive(Clone, Debug)]
pub struct PropertySpan {
    pub key: String,
    pub value: String,
    pub line: ByteSpan,
    pub value_span: ByteSpan,
}

#[derive(Clone, Debug)]
pub struct HeadlineSpan {
    pub item: Item,
    pub level: usize,
    pub line_span: ByteSpan,
    pub subtree_span: ByteSpan,
    pub section_span: ByteSpan,
    pub planning_span: Option<ByteSpan>,
    pub drawer_span: Option<ByteSpan>,
    pub body_span: ByteSpan,
    pub properties: Vec<PropertySpan>,
    pub timestamps: Vec<TimestampSpan>,
    pub digest: String,
}

#[derive(Clone, Debug)]
struct RawHeadline {
    level: usize,
    line_index: usize,
    line_span: ByteSpan,
    section_end: usize,
    subtree_end: usize,
    keyword: Option<String>,
    done: bool,
    priority: Option<String>,
    title: String,
    tags: Vec<String>,
    planning: Planning,
    planning_span: Option<ByteSpan>,
    drawer_span: Option<ByteSpan>,
    properties: Vec<PropertySpan>,
    timestamps: Vec<TimestampSpan>,
    digest: String,
}

struct ParsedMetadata {
    planning: Planning,
    planning_span: Option<ByteSpan>,
    drawer_span: Option<ByteSpan>,
    properties: Vec<PropertySpan>,
    body_start: usize,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn scan_headlines(
    path: &Path,
    source: &str,
    lines: &[SourceLine],
    todo_active: &[String],
    todo_done: &[String],
    file_tags: &[String],
    file_category: &str,
    settings: &Settings,
) -> Result<Vec<HeadlineSpan>, AgdError> {
    let mut raw = Vec::new();
    for (line_index, line) in lines.iter().enumerate() {
        let text = &source[line.content.start..line.content.end];
        let Some(captures) = HEADLINE_RE.captures(text) else {
            continue;
        };
        let stars = captures.get(1).expect("stars");
        let content = captures.get(2).expect("headline content");
        let (keyword, done, priority, title, tags) =
            parse_headline_content(content.as_str(), todo_active, todo_done);
        let ParsedMetadata {
            planning,
            planning_span,
            drawer_span,
            properties,
            body_start,
        } = parse_metadata(source, lines, line_index, raw.len())?;
        let digest = headline_digest(text.as_bytes());
        raw.push(RawHeadline {
            level: stars.as_str().len(),
            line_index,
            line_span: line.content,
            section_end: source.len(),
            subtree_end: source.len(),
            keyword,
            done,
            priority,
            title,
            tags,
            planning,
            planning_span,
            drawer_span,
            properties,
            timestamps: Vec::new(),
            digest,
        });
        let current = raw.last_mut().expect("pushed headline");
        current.section_end = body_start;
    }

    for index in 0..raw.len() {
        let next_any = raw
            .get(index + 1)
            .map_or(source.len(), |next| next.line_span.start);
        raw[index].section_end = next_any;
        raw[index].subtree_end = raw[index + 1..]
            .iter()
            .find(|next| next.level <= raw[index].level)
            .map_or(source.len(), |next| next.line_span.start);
        let timestamp_start = raw[index]
            .planning_span
            .map_or(raw[index].line_span.end, |span| span.end);
        // Org lists active timestamps on the headline line itself, e.g.
        // `* Birthday <1993-10-16 Sat +1y>`, but never the planning line.
        let mut timestamps =
            scan_timestamps(source, raw[index].line_span.start, raw[index].line_span.end);
        timestamps.extend(scan_timestamps(source, timestamp_start, next_any));
        raw[index].timestamps = timestamps;
    }

    let excluded: BTreeSet<&str> = settings
        .tag_inheritance_exclusions
        .iter()
        .map(String::as_str)
        .collect();
    let file_path = path.to_string_lossy().into_owned();
    let mut result = Vec::with_capacity(raw.len());
    let mut ancestors: Vec<(usize, String, Vec<String>, String)> = Vec::new();
    for headline in raw {
        while ancestors
            .last()
            .is_some_and(|ancestor| ancestor.0 >= headline.level)
        {
            ancestors.pop();
        }
        // Org's order: #+FILETAGS, then each ancestor's tags from the top,
        // each tag once and none the headline carries itself.
        let mut inherited: Vec<String> = Vec::new();
        let ancestor_tags = ancestors.iter().flat_map(|(_, _, tags, _)| tags);
        for tag in file_tags.iter().chain(ancestor_tags) {
            if !excluded.contains(tag.as_str())
                && !headline.tags.contains(tag)
                && !inherited.contains(tag)
            {
                inherited.push(tag.clone());
            }
        }
        let own_category = headline
            .properties
            .iter()
            .find(|property| property.key.eq_ignore_ascii_case("CATEGORY"))
            .map(|property| property.value.clone());
        let category = own_category.unwrap_or_else(|| {
            ancestors
                .last()
                .map(|ancestor| ancestor.3.clone())
                .unwrap_or_else(|| file_category.to_string())
        });
        let id = headline
            .properties
            .iter()
            .find(|property| property.key.eq_ignore_ascii_case("ID"))
            .map(|property| property.value.clone());
        let mut olpath: Vec<String> = ancestors
            .iter()
            .map(|ancestor| ancestor.1.clone())
            .collect();
        olpath.push(headline.title.clone());
        let item_ref = id.clone().unwrap_or_else(|| {
            location_ref(path, lines[headline.line_index].number, &headline.digest)
        });
        let item = Item {
            schema: ITEM_SCHEMA.to_string(),
            item_ref,
            id,
            file: file_path.clone(),
            line: lines[headline.line_index].number,
            olpath,
            todo: headline.keyword.clone(),
            done: headline.done,
            priority: headline.priority.clone(),
            title: headline.title.clone(),
            tags: headline.tags.clone(),
            inherited_tags: inherited,
            category: category.clone(),
            planning: headline.planning.clone(),
        };
        let metadata_end = headline
            .drawer_span
            .or(headline.planning_span)
            .map_or(headline.line_span.end, |span| span.end);
        let body_start = line_end_after(source, metadata_end);
        let body_span = ByteSpan {
            start: body_start.min(headline.section_end),
            end: headline.section_end,
        };
        ancestors.push((
            headline.level,
            headline.title.clone(),
            headline.tags.clone(),
            category,
        ));
        result.push(HeadlineSpan {
            item,
            level: headline.level,
            line_span: headline.line_span,
            subtree_span: ByteSpan {
                start: headline.line_span.start,
                end: headline.subtree_end,
            },
            section_span: ByteSpan {
                start: headline.line_span.start,
                end: headline.section_end,
            },
            planning_span: headline.planning_span,
            drawer_span: headline.drawer_span,
            body_span,
            properties: headline.properties,
            timestamps: headline.timestamps,
            digest: headline.digest,
        });
    }
    Ok(result)
}

/// The `[#X]` cookie at the start of `text`, as the parser reads a
/// headline's priority.
pub(crate) fn leading_priority_cookie(text: &str) -> Option<&str> {
    let cookie = PRIORITY_RE.captures(text)?.get(0)?.as_str();
    Some(cookie.trim_end())
}

/// Splits a headline's trailing tag group (`:a:b:`) off as the parser reads
/// it: the text before it, trailing blanks removed, and the group.
pub(crate) fn split_trailing_tags(line: &str) -> Option<(&str, &str)> {
    let group = TAGS_RE.captures(line)?.get(1)?;
    Some((line[..group.start()].trim_end(), group.as_str()))
}

fn parse_headline_content(
    input: &str,
    todo_active: &[String],
    todo_done: &[String],
) -> (Option<String>, bool, Option<String>, String, Vec<String>) {
    let mut content = input.trim_end();
    let tags = TAGS_RE
        .captures(content)
        .and_then(|captures| captures.get(1))
        .map(|matched| {
            content = content[..matched.start()].trim_end();
            parse_colon_tags(matched.as_str())
        })
        .unwrap_or_default();

    let first_end = content.find(char::is_whitespace).unwrap_or(content.len());
    let first = &content[..first_end];
    let (keyword, done) = if todo_active.iter().any(|word| word == first) {
        content = content[first_end..].trim_start();
        (Some(first.to_string()), false)
    } else if todo_done.iter().any(|word| word == first) {
        content = content[first_end..].trim_start();
        (Some(first.to_string()), true)
    } else {
        (None, false)
    };

    let priority = PRIORITY_RE.captures(content).and_then(|captures| {
        let full = captures.get(0)?;
        let value = captures.get(1)?.as_str().to_string();
        content = content[full.end()..].trim_start();
        Some(value)
    });
    (keyword, done, priority, content.to_string(), tags)
}

fn parse_metadata(
    source: &str,
    lines: &[SourceLine],
    headline_line: usize,
    _headline_count: usize,
) -> Result<ParsedMetadata, AgdError> {
    let mut planning = Planning::default();
    // Org reads planning only from the line right after the headline; a
    // second SCHEDULED/DEADLINE line or one after a blank line is plain text
    // with an ordinary timestamp.
    let mut planning_span: Option<ByteSpan> = None;
    let mut cursor = headline_line + 1;
    if let Some(line) = lines.get(cursor) {
        let trimmed = source[line.content.start..line.content.end].trim_start();
        if is_planning_line(trimmed) {
            planning_span = Some(line.full);
            planning.scheduled = labeled_timestamp(trimmed, "SCHEDULED:");
            planning.deadline = labeled_timestamp(trimmed, "DEADLINE:");
            planning.closed = labeled_timestamp(trimmed, "CLOSED:");
            cursor += 1;
        }
    }

    let mut properties = Vec::new();
    let mut drawer_span = None;
    if let Some(line) = lines.get(cursor) {
        let text = &source[line.content.start..line.content.end];
        let trimmed = text.trim_start();
        if trimmed == ":PROPERTIES:" {
            let start = line.full.start;
            cursor += 1;
            let mut found_end = false;
            while let Some(property_line) = lines.get(cursor) {
                let property_text = &source[property_line.content.start..property_line.content.end];
                let property_trimmed = property_text.trim_start();
                if property_trimmed == ":END:" {
                    drawer_span = Some(ByteSpan {
                        start,
                        end: property_line.full.end,
                    });
                    cursor += 1;
                    found_end = true;
                    break;
                }
                if let Some(captures) = PROPERTY_RE.captures(property_trimmed) {
                    let key = captures.get(1).expect("property key");
                    let value = captures.get(2);
                    properties.push(PropertySpan {
                        key: key.as_str().to_string(),
                        value: value.map_or("", |matched| matched.as_str()).to_string(),
                        line: property_line.full,
                        value_span: value.map_or(
                            ByteSpan {
                                start: property_line.content.end,
                                end: property_line.content.end,
                            },
                            |matched| ByteSpan {
                                start: property_line.content.start + property_text.len()
                                    - property_trimmed.len()
                                    + matched.start(),
                                end: property_line.content.start + property_text.len()
                                    - property_trimmed.len()
                                    + matched.end(),
                            },
                        ),
                    });
                }
                cursor += 1;
            }
            if !found_end {
                return Err(AgdError::new(
                    ErrorCode::ParseFailed,
                    format!(
                        "unterminated property drawer after line {}",
                        headline_line + 1
                    ),
                ));
            }
        }
    }
    let body_start = lines
        .get(cursor)
        .map_or(source.len(), |line| line.full.start);
    Ok(ParsedMetadata {
        planning,
        planning_span,
        drawer_span,
        properties,
        body_start,
    })
}

pub(super) fn parse_colon_tags(value: &str) -> Vec<String> {
    value
        .trim()
        .trim_matches(':')
        .split(':')
        .filter(|tag| !tag.is_empty())
        .map(str::to_string)
        .collect()
}

// A planning line starts with one of the keywords; `: SCHEDULED: <…>` is
// fixed-width text, not planning.
fn is_planning_line(line: &str) -> bool {
    let line = line.trim_start();
    ["SCHEDULED:", "DEADLINE:", "CLOSED:"]
        .iter()
        .any(|label| line.starts_with(label))
}

fn labeled_timestamp(line: &str, label: &str) -> Option<PlanningTimestamp> {
    let after = line.split_once(label)?.1;
    parse_timestamp(after)
}

/// The ref of a heading without a unique ID: `file:line#sha256:digest`.
pub(super) fn location_ref(path: &Path, line: usize, digest: &str) -> String {
    format!("{}:{line}#sha256:{digest}", path.display())
}

fn headline_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn line_end_after(source: &str, offset: usize) -> usize {
    if offset >= source.len() {
        return source.len();
    }
    if source.as_bytes().get(offset) == Some(&b'\n') {
        offset + 1
    } else {
        offset
    }
}
