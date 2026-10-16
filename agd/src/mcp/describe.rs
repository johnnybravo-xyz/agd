//! What an agent needs to know before it writes: the `describe` tool's
//! answer, and the facts the server instructions repeat (the date grammar,
//! the default capture template, the agent inbox).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use agd_core::{
    AgdError, CaptureTemplate, DESCRIBE_SCHEMA, IndexedDocument, Settings, TodoKeyword,
    WorkspaceIndex, capture_prompts,
};
use serde_json::{Value, json};

/// The date strings every date argument takes, as `parse_date_input` reads
/// them.
pub(super) const DATE_GRAMMAR: &str =
    "today, tomorrow, a weekday (fri), +2d (also +1w, +1m, +1y), 2026-10-30 or 2026-10-30T14:00";

/// The template key to capture with when nothing says otherwise: the agent
/// inbox when captures are gated to it, else the first configured one.
pub(super) fn default_template(settings: &Settings) -> Option<&str> {
    settings.agent_inbox.as_deref().or_else(|| {
        settings
            .captures
            .first()
            .map(|template| template.key.as_str())
    })
}

/// The `describe` tool's answer, schema agd.describe/v1, for the
/// configuration and the agenda files `index` read.
pub(super) fn describe(settings: &Settings, index: &WorkspaceIndex) -> Result<Value, AgdError> {
    let views: Vec<Value> = settings
        .all_views()
        .into_iter()
        .map(|view| json!({"key": view.key, "desc": view.desc, "start": view.key == settings.start_view}))
        .collect();
    Ok(json!({
        "schema": DESCRIBE_SCHEMA,
        "capture_templates": settings.captures.iter().map(template).collect::<Vec<_>>(),
        "default_template": default_template(settings),
        // The template agent captures are switched to, dates dropped; null
        // when the chosen template is used.
        "agent_inbox": settings.agent_inbox,
        "todo_keywords": todo_keywords(settings, index)?,
        "priorities": {
            "highest": settings.highest_priority,
            "lowest": settings.lowest_priority,
            "default": settings.default_priority,
        },
        "views": views,
        "tags": tags_in_use(index),
        "date_grammar": DATE_GRAMMAR,
    }))
}

/// A template's key and description, where it files (`inbox.org > Inbox`)
/// and the names its %^{name} prompts take in `capture`'s `prompts`.
fn template(template: &CaptureTemplate) -> Value {
    let mut target: Vec<String> = Vec::new();
    target.extend(template.file.clone());
    target.extend(template.id.as_ref().map(|id| format!("id:{id}")));
    target.extend(template.heading.clone());
    target.extend(template.olpath.clone().unwrap_or_default());
    let mut target = target.join(" > ");
    if template.datetree {
        target.push_str(" (datetree)");
    }
    let prompts: Vec<String> = capture_prompts(template)
        .into_iter()
        .map(|prompt| prompt.name)
        .collect();
    json!({"key": template.key, "desc": template.desc, "target": target, "prompts": prompts})
}

/// Each sequence as `TODO NEXT | DONE`: the configured default, then each
/// distinct #+TODO override with the files that use it.
fn todo_keywords(settings: &Settings, index: &WorkspaceIndex) -> Result<Value, AgdError> {
    // A file without #+TODO lines has exactly the configured keywords.
    let empty = IndexedDocument::parse(PathBuf::from("config.org"), Vec::new(), settings)?;
    let default = sequences(empty.todo_keywords());
    let mut overrides: BTreeMap<Vec<String>, Vec<String>> = BTreeMap::new();
    for document in index.documents() {
        let own = sequences(document.todo_keywords());
        if own != default {
            overrides
                .entry(own)
                .or_default()
                .push(document.path().display().to_string());
        }
    }
    let overrides: Vec<Value> = overrides
        .into_iter()
        .map(|(sequences, files)| json!({"sequences": sequences, "files": files}))
        .collect();
    Ok(json!({"default": default, "overrides": overrides}))
}

/// `TODO NEXT | DONE` per sequence, in sequence order.
fn sequences(keywords: &[TodoKeyword]) -> Vec<String> {
    let mut by_sequence: BTreeMap<usize, (Vec<&str>, Vec<&str>)> = BTreeMap::new();
    for keyword in keywords {
        let (active, done) = by_sequence.entry(keyword.sequence).or_default();
        if keyword.done {
            done.push(&keyword.name);
        } else {
            active.push(&keyword.name);
        }
    }
    by_sequence
        .into_values()
        .map(|(active, done)| format!("{} | {}", active.join(" "), done.join(" ")))
        .collect()
}

/// Every tag on a heading or a #+FILETAGS line, sorted.
fn tags_in_use(index: &WorkspaceIndex) -> Vec<String> {
    let tags: BTreeSet<&String> = index
        .items()
        .flat_map(|item| item.tags.iter().chain(&item.inherited_tags))
        .collect();
    tags.into_iter().cloned().collect()
}
