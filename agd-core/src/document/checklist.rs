//! An entry's checklist: the `- [ ] step` lines of its body, as Org's
//! org-toggle-checkbox and org-update-checkbox-count read them.
//!
//! agd checks off a flat checklist only. A nested list, or a statistics
//! cookie in the body or on an item, makes Org update parent boxes and
//! counts that agd does not reproduce, so such an entry is refused and
//! edited in $EDITOR instead.

use std::sync::LazyLock;

use regex::Regex;

use super::{ByteSpan, HeadlineSpan};
use crate::{AgdError, ErrorCode, Progress};

/// A list item: indentation, bullet (`-`, `+`, an indented `*`, `1.` or
/// `1)`; Org's default allows no letters), an optional `[@3]` counter,
/// then the rest of the line.
static ITEM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([ \t]*)(?:[-+]|\*|[0-9]+[.)])(?:[ \t]+(?:\[@(?:start:)?[0-9]+\][ \t]*)?(.*))?$")
        .expect("list item pattern")
});

/// A checkbox at the start of an item's text: `[ ]`, `[X]` or `[-]`.
static CHECKBOX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[([ X-])\](?:[ \t]+|$)").expect("checkbox pattern"));

/// Org's statistics cookie: `[2/5]`, `[40%]`, `[/]` or `[%]`.
pub(crate) static COOKIE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[(?:[0-9]*%|[0-9]*/[0-9]*)\]").expect("cookie pattern"));

/// One checklist line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChecklistItem {
    /// The text after the box.
    pub text: String,
    /// `[X]`; `[ ]` and `[-]` are open.
    pub checked: bool,
    /// The one byte inside the brackets, which checking off replaces.
    pub(crate) mark: ByteSpan,
}

/// The checklist in `headline`'s body, in order; empty when it has none.
/// Lines in drawers and blocks are not list items.
pub fn checklist(source: &str, headline: &HeadlineSpan) -> Result<Vec<ChecklistItem>, AgdError> {
    let mut items = Vec::new();
    let mut indents = Vec::new();
    let mut in_drawer = false;
    let mut in_block = false;
    let mut start = headline.body_span.start;
    let body = &source[headline.body_span.start..headline.body_span.end];
    for line in body.split_inclusive('\n') {
        let line_start = start;
        start += line.len();
        let text = line.trim_end_matches(['\n', '\r']);
        let trimmed = text.trim();
        let lower = trimmed.to_ascii_lowercase();
        if in_block {
            in_block = !lower.starts_with("#+end_");
            continue;
        }
        if in_drawer {
            in_drawer = !trimmed.eq_ignore_ascii_case(":END:");
            continue;
        }
        if lower.starts_with("#+begin_") {
            in_block = true;
            continue;
        }
        if trimmed.len() > 2 && trimmed.starts_with(':') && trimmed.ends_with(':') {
            in_drawer = true;
            continue;
        }
        let Some(item) = ITEM.captures(text) else {
            if COOKIE.is_match(text) {
                return Err(unsupported("a statistics cookie in its body"));
            }
            continue;
        };
        let indent = item.get(1).map_or("", |indent| indent.as_str());
        // A `*` bullet at the margin is a heading, not an item.
        if indent.is_empty() && trimmed.starts_with('*') {
            continue;
        }
        indents.push(indent.len());
        let Some(rest) = item.get(2) else {
            continue;
        };
        let Some(checkbox) = CHECKBOX.captures(rest.as_str()) else {
            if COOKIE.is_match(rest.as_str()) {
                return Err(unsupported("a statistics cookie on a list item"));
            }
            continue;
        };
        let mark = checkbox.get(1).expect("checkbox mark");
        let item_text = rest.as_str()[checkbox.get(0).expect("checkbox").end()..].to_string();
        if COOKIE.is_match(&item_text) {
            return Err(unsupported("a statistics cookie on a list item"));
        }
        let mark_start = line_start + rest.start() + mark.start();
        items.push(ChecklistItem {
            text: item_text,
            checked: mark.as_str() == "X",
            mark: ByteSpan {
                start: mark_start,
                end: mark_start + 1,
            },
        });
    }
    if indents.iter().any(|indent| Some(indent) != indents.first()) {
        return Err(unsupported("a nested list"));
    }
    Ok(items)
}

/// What lists show for `headline`'s checklist: checked of all boxes. None
/// without a checklist, or when the heading has its own cookie, which
/// already says it.
pub(crate) fn progress(source: &str, headline: &HeadlineSpan) -> Option<Progress> {
    if COOKIE.is_match(&headline.item.title) {
        return None;
    }
    let items = checklist(source, headline).ok()?;
    (!items.is_empty()).then(|| Progress {
        done: items.iter().filter(|item| item.checked).count(),
        total: items.len(),
    })
}

fn unsupported(what: &str) -> AgdError {
    AgdError::new(
        ErrorCode::InvalidEdit,
        format!("this entry has {what}; check it off in $EDITOR"),
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::Settings;
    use crate::document::OrgDocument;

    fn items(source: &str) -> Result<Vec<(String, bool)>, AgdError> {
        let document = OrgDocument::parse(
            PathBuf::from("tasks.org"),
            source.as_bytes().to_vec(),
            &Settings::default(),
        )
        .unwrap();
        checklist(document.source(), &document.headlines()[0]).map(|items| {
            items
                .into_iter()
                .map(|item| (item.text, item.checked))
                .collect()
        })
    }

    #[test]
    fn a_checklist_skips_drawers_and_blocks_and_stops_at_the_next_heading() {
        let source = "* TODO Plan [1/2]\n:LOGBOOK:\n- [ ] not an item\n:END:\n\
#+begin_example\n- [ ] not one either\n#+end_example\n\
- [X] book venue\n- [ ] send invites\n- plain item\n* TODO Next\n- [ ] other\n";
        assert_eq!(
            items(source).unwrap(),
            [
                ("book venue".to_string(), true),
                ("send invites".to_string(), false)
            ]
        );
    }

    #[test]
    fn an_item_carries_its_progress_unless_its_heading_has_a_cookie() {
        let progress = |source: &str| {
            OrgDocument::parse(
                PathBuf::from("tasks.org"),
                source.as_bytes().to_vec(),
                &Settings::default(),
            )
            .unwrap()
            .headlines()[0]
                .item
                .checklist
        };
        let steps = "- [X] venue\n- [ ] invites\n- [ ] lunch\n";
        assert_eq!(
            progress(&format!("* TODO Plan\n{steps}")),
            Some(crate::Progress { done: 1, total: 3 })
        );
        assert_eq!(progress(&format!("* TODO Plan [1/3]\n{steps}")), None);
        assert_eq!(progress("* TODO Plan\nNo steps.\n"), None);
    }

    #[test]
    fn a_nested_list_or_a_cookie_outside_the_heading_is_left_to_the_editor() {
        let nested = "* TODO Plan\n- [ ] venue\n  - [ ] call\n";
        let in_body = "* TODO Plan\nDone [1/2] so far.\n- [ ] venue\n";
        let on_item = "* TODO Plan\n- [ ] venue [0/1]\n";
        for source in [nested, in_body, on_item] {
            let error = items(source).unwrap_err();
            assert_eq!(error.code(), ErrorCode::InvalidEdit, "{source}");
            assert!(error.to_string().contains("$EDITOR"), "{error}");
        }
    }
}
