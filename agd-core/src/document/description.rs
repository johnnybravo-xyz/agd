//! An entry's description: the first run of plain text in its body, what
//! people write under a heading. Drawers, blocks and lists before it are
//! skipped, so the log notes Org adds at the top of the body, in a LOGBOOK
//! or as list items, never hide it. Lines indented under a list item are
//! that item's, as in Org. The run ends at the next list item, drawer,
//! block, table, comment or keyword line; blank lines inside it belong to
//! it, blank lines after it do not.

use super::checklist::ITEM;
use super::{ByteSpan, HeadlineSpan};

/// Where an entry's description is, or where a new one goes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Description {
    /// From the first line's start to the last line's end; empty, at the
    /// first line after the leading drawers, when there is none.
    pub(crate) span: ByteSpan,
    /// The end of the blank lines after it, which removing it removes too.
    pub(crate) blanks_end: usize,
    /// The first line's indentation, which every line is written with.
    pub(crate) indent: String,
    /// The lines without that indentation; None when there is none.
    pub(crate) text: Option<String>,
}

/// True for a line a description may hold: text that Org does not read as
/// a list item, heading, drawer, block, table, comment or keyword.
pub(crate) fn is_plain_text(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty() && !ITEM.is_match(line) && !trimmed.starts_with(['#', '|', ':'])
}

/// `headline`'s description.
pub(crate) fn description(source: &str, headline: &HeadlineSpan) -> Description {
    let body = &source[headline.body_span.start..headline.body_span.end];
    let mut offset = headline.body_span.start;
    let mut in_drawer = false;
    let mut in_block = false;
    let mut first_content = None;
    let mut after_drawers = headline.body_span.start;
    let mut run: Option<ByteSpan> = None;
    let mut item_indent: Option<usize> = None;
    for line in body.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let text = line.trim_end_matches(['\n', '\r']);
        let trimmed = text.trim();
        let lower = trimmed.to_ascii_lowercase();
        if in_block {
            in_block = !lower.starts_with("#+end_");
            continue;
        }
        if in_drawer {
            in_drawer = !trimmed.eq_ignore_ascii_case(":END:");
            if !in_drawer && first_content.is_none() {
                after_drawers = offset;
            }
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        let indent = text.len() - text.trim_start().len();
        if item_indent.is_some_and(|item| indent > item) {
            continue;
        }
        item_indent = ITEM.is_match(text).then_some(indent);
        let drawer = trimmed.len() > 2 && trimmed.starts_with(':') && trimmed.ends_with(':');
        if is_plain_text(text) {
            first_content.get_or_insert(start);
            let span = run.get_or_insert(ByteSpan { start, end: offset });
            span.end = offset;
            continue;
        }
        if run.is_some() {
            break;
        }
        if drawer {
            in_drawer = true;
            continue;
        }
        first_content.get_or_insert(start);
        in_block = lower.starts_with("#+begin_");
    }
    let Some(span) = run else {
        let at = first_content.unwrap_or(after_drawers);
        return Description {
            span: ByteSpan { start: at, end: at },
            blanks_end: at,
            indent: String::new(),
            text: None,
        };
    };
    let lines = &source[span.start..span.end];
    let indent: String = lines
        .chars()
        .take_while(|character| *character == ' ' || *character == '\t')
        .collect();
    let text = lines
        .lines()
        .map(|line| {
            let line = line.trim_end_matches('\r');
            line.strip_prefix(indent.as_str())
                .unwrap_or(line.trim_start())
        })
        .collect::<Vec<_>>()
        .join("\n");
    let blanks_end = span.end
        + source[span.end..headline.body_span.end]
            .split_inclusive('\n')
            .take_while(|line| line.trim().is_empty())
            .map(str::len)
            .sum::<usize>();
    Description {
        span,
        blanks_end,
        indent,
        text: Some(text),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::Settings;
    use crate::document::OrgDocument;

    fn read(source: &str) -> Description {
        let document = OrgDocument::parse(
            PathBuf::from("tasks.org"),
            source.as_bytes().to_vec(),
            &Settings::default(),
        )
        .unwrap();
        description(document.source(), &document.headlines()[0])
    }

    #[test]
    fn text_after_drawers_and_log_notes_up_to_the_checklist() {
        let source = "* TODO Plan\n:PROPERTIES:\n:ID: a\n:END:\n:LOGBOOK:\n- Note taken on [2026-10-19 Mon 15:00] \\\\\n  first\n:END:\n- Note taken on [2026-10-19 Mon 16:00] \\\\\n  second\nVenue near the office.\n\nBudget 50k.\n\n- [ ] book venue\n";
        let description = read(source);
        assert_eq!(
            description.text.as_deref(),
            Some("Venue near the office.\n\nBudget 50k.")
        );
        assert_eq!(
            &source[description.span.start..description.span.end],
            "Venue near the office.\n\nBudget 50k.\n"
        );
        assert_eq!(&source[description.blanks_end..], "- [ ] book venue\n");
    }

    #[test]
    fn indentation_is_the_first_lines() {
        let description = read("* Note\n  First line\n  second\n");
        assert_eq!(description.indent, "  ");
        assert_eq!(description.text.as_deref(), Some("First line\nsecond"));
    }

    #[test]
    fn none_goes_after_the_leading_drawers_and_before_anything_else() {
        let source = "* TODO Plan\n:LOGBOOK:\n- State \"TODO\" from [2026-10-19 Mon 15:00]\n:END:\n- [ ] book venue\n";
        let description = read(source);
        assert_eq!(description.text, None);
        assert_eq!(&source[description.span.start..], "- [ ] book venue\n");

        let empty = "* TODO Plan\n:LOGBOOK:\n- note\n:END:\n";
        let description = read(empty);
        assert_eq!(description.span.start, empty.len());
    }

    #[test]
    fn blocks_tables_comments_and_keywords_are_not_text() {
        let description = read(
            "* Note\n#+BEGIN_SRC sh\nls\n#+END_SRC\n| a |\n# comment\n#+CAPTION: x\nThe text.\n",
        );
        assert_eq!(description.text.as_deref(), Some("The text."));
        assert!(!is_plain_text("  - item"));
        assert!(!is_plain_text("1. item"));
        assert!(!is_plain_text(": fixed width"));
        assert!(is_plain_text("Plain words: with a colon"));
    }
}
