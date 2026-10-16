//! TODO keyword colors as agd draws them: a keyword's `todo_keyword_colors`
//! entry (by default WAIT yellow and CANCELLED dim-red), else green when
//! done, orange for the first keyword of its sequence, and blue otherwise.
//!
//! Colors are names from `COLOR_NAMES`, never terminal codes; the TUI maps
//! them onto its palette. A keyword two files order differently takes the
//! first file's color, so the result never depends on hash order.

use std::collections::BTreeMap;

use crate::{Item, Settings, WorkspaceIndex};

/// A TODO keyword's color name (COLOR_NAMES) wherever agd draws it: the
/// configured `todo_keyword_colors` entry, else green when done, orange for
/// the first keyword of its sequence (TODO), and blue for the rest and for
/// headings without a keyword.
pub fn keyword_color(index: &WorkspaceIndex, item: &Item) -> String {
    let Some(name) = item.todo.as_deref() else {
        return "blue".to_string();
    };
    let keywords = index
        .document(std::path::Path::new(&item.file))
        .map(|document| document.todo_keywords())
        .unwrap_or_default();
    color_in_sequence(index.settings(), keywords, name, item.done)
}

/// Every TODO keyword the agenda files use, with its `keyword_color`. A
/// keyword two files order differently takes the first file's color.
pub fn keyword_colors(index: &WorkspaceIndex) -> BTreeMap<String, String> {
    let mut colors = BTreeMap::new();
    for document in index.documents() {
        let keywords = document.todo_keywords();
        for keyword in keywords {
            colors.entry(keyword.name.clone()).or_insert_with(|| {
                color_in_sequence(index.settings(), keywords, &keyword.name, keyword.done)
            });
        }
    }
    colors
}

fn color_in_sequence(
    settings: &Settings,
    keywords: &[crate::document::TodoKeyword],
    name: &str,
    done: bool,
) -> String {
    if let Some(color) = settings.todo_keyword_colors.get(name) {
        return color.clone();
    }
    if done {
        return "green".to_string();
    }
    let first = keywords
        .iter()
        .find(|keyword| keyword.name == name)
        .and_then(|keyword| {
            keywords
                .iter()
                .find(|head| head.sequence == keyword.sequence)
        })
        .is_some_and(|head| head.name == name);
    if first { "orange" } else { "blue" }.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agenda::test_support::fixture;

    fn colors(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, color)| (name.to_string(), color.to_string()))
            .collect()
    }

    #[test]
    fn the_default_workflow_has_the_designs_colors_until_a_map_replaces_them() {
        let (temp, index) = fixture("* TODO a\n* NEXT b\n* WAIT c\n* DONE d\n* CANCELLED e\n");
        assert_eq!(
            keyword_colors(&index),
            colors(&[
                ("CANCELLED", "dim-red"),
                ("DONE", "green"),
                ("NEXT", "blue"),
                ("TODO", "orange"),
                ("WAIT", "yellow"),
            ])
        );
        let settings = Settings {
            todo_keyword_colors: colors(&[("NEXT", "teal")]),
            ..index.settings().clone()
        };
        let index = WorkspaceIndex::load(settings).unwrap();
        assert_eq!(
            keyword_colors(&index),
            colors(&[
                ("CANCELLED", "green"),
                ("DONE", "green"),
                ("NEXT", "teal"),
                ("TODO", "orange"),
                ("WAIT", "blue"),
            ])
        );
        drop(temp);
    }
}
