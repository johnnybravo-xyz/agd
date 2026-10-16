//! Byte patches: `Patch` and its journaled form `StoredPatch`, `apply_patches`,
//! the one patch between two texts, line helpers, and the unified diff a
//! mutation shows.
//!
//! Patches must not overlap, run past the source or split a UTF-8 character
//! (`invalid_patch`), and every byte outside them is copied exactly. A stored
//! patch keeps the text around it so undo can find it again.

use std::path::Path;

use serde::{Deserialize, Serialize};
use similar::TextDiff;

use crate::{AgdError, ByteSpan, ErrorCode};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Patch {
    pub span: ByteSpan,
    pub replacement: String,
}

/// A journaled replacement: `start..end` is the span of `after` in the
/// post-image; `context_before`/`context_after` are the post-image bytes
/// around it (the rest of its lines plus one line each way), used to find the
/// span again if the file changed elsewhere before undo.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StoredPatch {
    pub start: usize,
    pub end: usize,
    pub before: String,
    pub after: String,
    #[serde(default)]
    pub context_before: Option<String>,
    #[serde(default)]
    pub context_after: Option<String>,
}

pub fn apply_patches(
    source: &str,
    mut patches: Vec<Patch>,
) -> Result<(String, Vec<StoredPatch>), AgdError> {
    patches.sort_by_key(|patch| (patch.span.start, patch.span.end));
    let mut cursor = 0;
    let mut post_cursor = 0;
    let mut output = String::with_capacity(source.len());
    let mut stored = Vec::with_capacity(patches.len());
    for patch in patches {
        if patch.span.start < cursor
            || patch.span.end < patch.span.start
            || patch.span.end > source.len()
        {
            return Err(AgdError::new(
                ErrorCode::InvalidPatch,
                format!(
                    "patch {}..{} overlaps or exceeds the source",
                    patch.span.start, patch.span.end
                ),
            ));
        }
        if !source.is_char_boundary(patch.span.start) || !source.is_char_boundary(patch.span.end) {
            return Err(AgdError::new(
                ErrorCode::InvalidPatch,
                "patch is not on UTF-8 boundaries",
            ));
        }
        output.push_str(&source[cursor..patch.span.start]);
        post_cursor += patch.span.start - cursor;
        let post_start = post_cursor;
        let before = source[patch.span.start..patch.span.end].to_string();
        output.push_str(&patch.replacement);
        post_cursor += patch.replacement.len();
        stored.push(StoredPatch {
            start: post_start,
            end: post_cursor,
            before,
            after: patch.replacement,
            context_before: None,
            context_after: None,
        });
        cursor = patch.span.end;
    }
    output.push_str(&source[cursor..]);
    for patch in &mut stored {
        let context_start = line_start(&output, line_start(&output, patch.start).saturating_sub(1));
        let context_end = line_end(&output, line_end(&output, patch.end) + 1);
        patch.context_before = Some(output[context_start..patch.start].to_string());
        patch.context_after = Some(output[patch.end..context_end].to_string());
    }
    Ok((output, stored))
}

fn line_start(text: &str, offset: usize) -> usize {
    text[..offset.min(text.len())]
        .rfind('\n')
        .map_or(0, |index| index + 1)
}

/// Offset just past the newline that ends the line containing `offset`.
fn line_end(text: &str, offset: usize) -> usize {
    if offset >= text.len() {
        return text.len();
    }
    text[offset..]
        .find('\n')
        .map_or(text.len(), |index| offset + index + 1)
}

/// One patch turning `before` into `after`: the differing middle, widened
/// to whole lines so the journal reads like the diff.
pub(super) fn changed_lines_patch(before: &str, after: &str) -> Patch {
    let mut prefix = before
        .bytes()
        .zip(after.bytes())
        .take_while(|(left, right)| left == right)
        .count();
    prefix = line_start(before, prefix);
    let mut suffix = before
        .bytes()
        .rev()
        .zip(after.bytes().rev())
        .take_while(|(left, right)| left == right)
        .count()
        .min(before.len() - prefix)
        .min(after.len() - prefix);
    let tail = line_end_at_or_after(before, before.len() - suffix);
    suffix = before.len() - tail;
    while !before.is_char_boundary(before.len() - suffix)
        || !after.is_char_boundary(after.len() - suffix)
    {
        suffix -= 1;
    }
    Patch {
        span: ByteSpan {
            start: prefix,
            end: before.len() - suffix,
        },
        replacement: after[prefix..after.len() - suffix].to_string(),
    }
}

/// `offset` if a line starts there, else the start of the next line.
fn line_end_at_or_after(text: &str, offset: usize) -> usize {
    if offset == 0 || offset >= text.len() || text.as_bytes()[offset - 1] == b'\n' {
        return offset;
    }
    line_end(text, offset)
}

pub(super) fn line_ending(source: &str) -> &'static str {
    if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

pub(super) fn unified_diff(path: &Path, before: &str, after: &str) -> String {
    TextDiff::from_lines(before, after)
        .unified_diff()
        .header(&path.display().to_string(), &path.display().to_string())
        .to_string()
}
