//! What an agent needs to know before it writes: the `describe` tool's
//! answer, and the facts the server instructions repeat (the date grammar,
//! the default capture template, the agent inbox).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use agd_core::{
    AgdError, CaptureTemplate, DESCRIBE_SCHEMA, IndexedDocument, Settings, TodoKeyword,
    WorkspaceIndex, capture_prompts,
};
use serde::Serialize;

/// The date strings every date argument takes, as `parse_date_input` reads
/// them.
pub(super) const DATE_GRAMMAR: &str =
    "today, tomorrow, a weekday (fri), +2d (also +1w, +1m, +1y), 2026-10-30 or 2026-10-30T14:00";

/// The template every agent capture goes through when `agent_inbox` is set,
/// which also drops their dates; None when the agent's chosen template is used.
pub(super) fn agent_inbox(settings: &Settings) -> Option<&str> {
    settings.agent_inbox.as_deref()
}

/// The template key to capture with when nothing says otherwise: the agent
/// inbox when captures are gated to it, else the first configured one.
pub(super) fn default_template(settings: &Settings) -> Option<&str> {
    agent_inbox(settings).or_else(|| {
        settings
            .captures
            .first()
            .map(|template| template.key.as_str())
    })
}

/// The `describe` tool's answer, schema agd.describe/v1.
#[derive(Debug, Serialize)]
pub(super) struct Description {
    schema: &'static str,
    capture_templates: Vec<TemplateDescription>,
    default_template: Option<String>,
    /// The template agent captures are switched to, dates dropped; null
    /// when the chosen template is used.
    agent_inbox: Option<String>,
    todo_keywords: TodoKeywordsDescription,
    priorities: PrioritiesDescription,
    views: Vec<ViewDescription>,
    tags: Vec<String>,
    date_grammar: &'static str,
}

#[derive(Debug, Serialize)]
struct TemplateDescription {
    key: String,
    desc: String,
    /// Where it files, such as `inbox.org > Inbox`.
    target: String,
    /// Names its %^{name} prompts take in `capture`'s `prompts`.
    prompts: Vec<String>,
}

/// Each sequence as `TODO NEXT | DONE`: the configured default, then each
/// distinct #+TODO override with the files that use it.
#[derive(Debug, Serialize)]
struct TodoKeywordsDescription {
    default: Vec<String>,
    overrides: Vec<KeywordOverride>,
}

#[derive(Debug, Serialize)]
struct KeywordOverride {
    sequences: Vec<String>,
    files: Vec<String>,
}

#[derive(Debug, Serialize)]
struct PrioritiesDescription {
    highest: String,
    lowest: String,
    default: String,
}

#[derive(Debug, Serialize)]
struct ViewDescription {
    key: String,
    desc: String,
    /// The configured start_view.
    start: bool,
}

/// Describes the configuration and the agenda files `index` read.
pub(super) fn describe(
    settings: &Settings,
    index: &WorkspaceIndex,
) -> Result<Description, AgdError> {
    Ok(Description {
        schema: DESCRIBE_SCHEMA,
        capture_templates: settings.captures.iter().map(template).collect(),
        default_template: default_template(settings).map(str::to_string),
        agent_inbox: agent_inbox(settings).map(str::to_string),
        todo_keywords: todo_keywords(settings, index)?,
        priorities: PrioritiesDescription {
            highest: settings.highest_priority.clone(),
            lowest: settings.lowest_priority.clone(),
            default: settings.default_priority.clone(),
        },
        views: settings
            .all_views()
            .into_iter()
            .map(|view| ViewDescription {
                key: view.key.clone(),
                desc: view.desc.clone(),
                start: view.key == settings.start_view,
            })
            .collect(),
        tags: tags_in_use(index),
        date_grammar: DATE_GRAMMAR,
    })
}

fn template(template: &CaptureTemplate) -> TemplateDescription {
    let mut target: Vec<String> = Vec::new();
    target.extend(template.file.clone());
    target.extend(template.id.as_ref().map(|id| format!("id:{id}")));
    target.extend(template.heading.clone());
    target.extend(template.outline_path.clone().unwrap_or_default());
    let mut target = target.join(" > ");
    if template.datetree {
        target.push_str(" (datetree)");
    }
    TemplateDescription {
        key: template.key.clone(),
        desc: template.desc.clone(),
        target,
        prompts: capture_prompts(template)
            .into_iter()
            .map(|prompt| prompt.name)
            .collect(),
    }
}

fn todo_keywords(
    settings: &Settings,
    index: &WorkspaceIndex,
) -> Result<TodoKeywordsDescription, AgdError> {
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
    Ok(TodoKeywordsDescription {
        default,
        overrides: overrides
            .into_iter()
            .map(|(sequences, files)| KeywordOverride { sequences, files })
            .collect(),
    })
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
