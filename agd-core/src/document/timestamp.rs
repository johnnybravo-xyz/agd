//! Org timestamps in text: `parse_timestamp` reads one (date, time range,
//! repeater, warning and raw text), and `scan_timestamps` finds the ones Org
//! would list in a span of source.
//!
//! Timestamps in verbatim blocks (src, example, export, comment), fixed-width
//! lines and link targets are skipped, as Org skips them.

use std::sync::LazyLock;

use regex::Regex;

use crate::PlanningTimestamp;

use super::source::ByteSpan;

static LINK_TARGET_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[[^\]\n]*\]").expect("link target regex"));
static TIMESTAMP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"[<\[](\d{4}-\d{2}-\d{2})(?:[ \t]+[^\s\d+\->\]]+)?(?:[ \t]+(\d{1,2}:\d{2})(?:-(\d{1,2}:\d{2}))?)?([^>\]]*)[>\]]",
    )
    .expect("timestamp regex")
});

#[derive(Clone, Debug)]
pub struct TimestampSpan {
    pub timestamp: PlanningTimestamp,
    pub span: ByteSpan,
    pub active: bool,
}

pub fn parse_timestamp(input: &str) -> Option<PlanningTimestamp> {
    let captures = TIMESTAMP_RE.captures(input)?;
    let suffix = captures.get(4).map_or("", |matched| matched.as_str());
    let repeater = suffix
        .split_whitespace()
        .find(|token| token.starts_with('+') || token.starts_with(".+"))
        .map(str::to_string);
    let warning = suffix
        .split_whitespace()
        .find(|token| token.starts_with('-'))
        .map(str::to_string);
    Some(PlanningTimestamp {
        date: captures.get(1)?.as_str().to_string(),
        time: captures.get(2).map(|matched| matched.as_str().to_string()),
        end_time: captures.get(3).map(|matched| matched.as_str().to_string()),
        repeater,
        warning,
        raw: captures.get(0).map(|matched| matched.as_str().to_string()),
    })
}

// Org ignores timestamps in src/example/export/comment blocks, in
// fixed-width lines (`: text`) and inside link targets.
pub(super) fn scan_timestamps(source: &str, start: usize, end: usize) -> Vec<TimestampSpan> {
    if start >= end || end > source.len() {
        return Vec::new();
    }
    let mut found = Vec::new();
    let mut in_verbatim_block = false;
    let mut line_start = start;
    for line in source[start..end].split_inclusive('\n') {
        let offset = line_start;
        line_start += line.len();
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        if in_verbatim_block {
            in_verbatim_block = !lower.starts_with("#+end_");
            continue;
        }
        if [
            "#+begin_src",
            "#+begin_example",
            "#+begin_export",
            "#+begin_comment",
        ]
        .iter()
        .any(|opening| lower.starts_with(opening))
        {
            in_verbatim_block = true;
            continue;
        }
        if trimmed == ":" || trimmed.starts_with(": ") {
            continue;
        }
        let masked = LINK_TARGET_RE.replace_all(line, |captures: &regex::Captures| {
            " ".repeat(captures[0].len())
        });
        for captures in TIMESTAMP_RE.captures_iter(&masked) {
            let Some(matched) = captures.get(0) else {
                continue;
            };
            let Some(timestamp) = parse_timestamp(matched.as_str()) else {
                continue;
            };
            found.push(TimestampSpan {
                timestamp,
                span: ByteSpan {
                    start: offset + matched.start(),
                    end: offset + matched.end(),
                },
                active: matched.as_str().starts_with('<'),
            });
        }
    }
    found
}
