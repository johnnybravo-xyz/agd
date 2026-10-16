//! Positions in a document's source: `ByteSpan`, `SourceLine`, and the split of
//! the source into numbered lines.
//!
//! Spans are byte offsets into the exact source. A line's `content` leaves out
//! its `\n` or `\r\n` and its `full` span includes it.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ByteSpan {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug)]
pub struct SourceLine {
    pub number: usize,
    pub full: ByteSpan,
    pub content: ByteSpan,
}

pub(super) fn source_lines(source: &str) -> Vec<SourceLine> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (index, segment) in source.split_inclusive('\n').enumerate() {
        let end = start + segment.len();
        let content_end = if segment.ends_with("\r\n") {
            end - 2
        } else if segment.ends_with('\n') {
            end - 1
        } else {
            end
        };
        lines.push(SourceLine {
            number: index + 1,
            full: ByteSpan { start, end },
            content: ByteSpan {
                start,
                end: content_end,
            },
        });
        start = end;
    }
    if source.is_empty() {
        return lines;
    }
    if start < source.len() {
        lines.push(SourceLine {
            number: lines.len() + 1,
            full: ByteSpan {
                start,
                end: source.len(),
            },
            content: ByteSpan {
                start,
                end: source.len(),
            },
        });
    }
    lines
}
