//! `WriteEngine` and `WriteContext`, and what every write shares: the dry-run
//! journal id, a computed file image (`PreparedFile`) and its journal image,
//! parsing written text again, and the `Mutation` result.
//!
//! The operations live by concern: `edit`, `capture`, `refile`, `undo`,
//! `journal` (review and interrupted writes) and `file` (locks and atomic
//! writes). Opening an engine only reads.

use std::path::{Path, PathBuf};

use chrono::{Local, NaiveDateTime};

use crate::{
    AgdError, ErrorCode, IndexedDocument, Item, MUTATION_SCHEMA, Mutation, RootSet, Settings,
};

use super::patch::{StoredPatch, unified_diff};

#[derive(Clone, Debug)]
pub struct WriteContext {
    pub actor: String,
    pub project: String,
    /// Clock for timestamps written into Org text (CLOSED, LOGBOOK, %U);
    /// None uses the local time. Tests and oracles pin it.
    pub now: Option<NaiveDateTime>,
}

impl WriteContext {
    pub fn org_now(&self) -> NaiveDateTime {
        self.now.unwrap_or_else(|| Local::now().naive_local())
    }

    pub fn is_agent(&self) -> bool {
        agent_name(&self.actor).is_some()
    }
}

/// How an MCP client's writes are journaled: `mcp:<client name>`. Every
/// other actor is a person using the CLI or the TUI.
const AGENT_ACTOR_PREFIX: &str = "mcp:";

/// The agent an actor names, `codex` for `mcp:codex`; None for a person.
pub fn agent_name(actor: &str) -> Option<&str> {
    actor.strip_prefix(AGENT_ACTOR_PREFIX)
}

/// Who an actor is as agd names them: the agent (`codex`), or `you`.
pub fn actor_name(actor: &str) -> &str {
    agent_name(actor).unwrap_or("you")
}

/// The actor an MCP client's writes are journaled under: `mcp:<client>`,
/// without the `-mcp-client` suffix Codex sends (`codex-mcp-client` is
/// `mcp:codex`); other names are kept as sent.
pub fn agent_actor(client_name: &str) -> String {
    let name = client_name
        .strip_suffix("-mcp-client")
        .filter(|name| !name.is_empty())
        .unwrap_or(client_name);
    format!("{AGENT_ACTOR_PREFIX}{name}")
}

#[derive(Debug)]
pub struct WriteEngine {
    pub(super) settings: Settings,
    pub(super) roots: RootSet,
    pub(super) journal_path: PathBuf,
}

/// The `journal_id` of a dry-run mutation, which writes no journal entry.
pub(super) const DRY_RUN_JOURNAL_ID: &str = "dry-run";

/// One file's computed write: its bytes now, its bytes after, and the
/// journal patches and diff between them.
pub(super) struct PreparedFile {
    pub(super) path: PathBuf,
    pre_image: String,
    pub(super) post_image: String,
    patches: Vec<StoredPatch>,
    pub(super) diff: String,
}

impl PreparedFile {
    pub(super) fn new(
        path: PathBuf,
        pre_image: String,
        post_image: String,
        patches: Vec<StoredPatch>,
    ) -> Self {
        let diff = unified_diff(&path, &pre_image, &post_image);
        Self {
            path,
            pre_image,
            post_image,
            patches,
            diff,
        }
    }

    pub(super) fn journal_image<'a>(&'a self, item_ref: Option<&'a str>) -> JournalImage<'a> {
        JournalImage {
            path: &self.path,
            item_ref,
            pre_image: &self.pre_image,
            post_image: &self.post_image,
            patches: &self.patches,
            diff: &self.diff,
        }
    }
}

pub(super) struct JournalImage<'a> {
    pub(super) path: &'a Path,
    pub(super) item_ref: Option<&'a str>,
    pub(super) pre_image: &'a str,
    pub(super) post_image: &'a str,
    pub(super) patches: &'a [StoredPatch],
    pub(super) diff: &'a str,
}

impl WriteEngine {
    /// An engine writing under the configured roots and journaling to
    /// `journal_path`. Opening it only reads; interrupted writes are settled
    /// by the next write to their file, or by `resolve_conflicts`.
    pub fn new(settings: Settings, journal_path: PathBuf) -> Result<Self, AgdError> {
        let roots = RootSet::new(&settings.roots, &settings.agenda_files)?;
        Ok(Self {
            settings,
            roots,
            journal_path,
        })
    }

    pub fn with_default_journal(settings: Settings) -> Result<Self, AgdError> {
        Self::new(settings, crate::path::data_dir()?.join("journal.jsonl"))
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }
}

pub(super) fn parse_text(
    path: &Path,
    text: &str,
    settings: &Settings,
) -> Result<IndexedDocument, AgdError> {
    IndexedDocument::parse(path.to_path_buf(), text.as_bytes().to_vec(), settings)
}

pub(super) fn parse_updated_item(
    path: &Path,
    source: &str,
    line_hint: usize,
    settings: &Settings,
) -> Result<Item, AgdError> {
    let document =
        IndexedDocument::parse(path.to_path_buf(), source.as_bytes().to_vec(), settings)?;
    document
        .headline_at_line(line_hint)
        .or_else(|| {
            document
                .headlines()
                .iter()
                .min_by_key(|headline| headline.item.line.abs_diff(line_hint))
        })
        .map(|headline| headline.item.clone())
        .ok_or_else(|| AgdError::new(ErrorCode::ParseError, "write produced no target headline"))
}

/// A successful mutation result.
pub(super) fn mutation(item: Item, diff: String, journal_id: &str) -> Mutation {
    Mutation {
        schema: MUTATION_SCHEMA.to_string(),
        ok: true,
        item,
        diff,
        journal_id: journal_id.to_string(),
        repeated: None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use super::*;
    use crate::write::test_support::{context, settings, tree};
    use crate::{CaptureDraft, Change, EditIntent, WorkspaceIndex};

    #[test]
    fn dry_runs_create_no_directory_lock_journal_or_file() {
        let org = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        fs::write(org.path().join("tasks.org"), "* TODO Task\n").unwrap();
        let mut settings = settings(org.path());
        settings.captures[0].file = Some("new/inbox.org".to_string());
        let engine = WriteEngine::new(settings.clone(), data.path().join("journal.jsonl")).unwrap();
        let draft = CaptureDraft {
            title: "Preview only".to_string(),
            ..CaptureDraft::default()
        };
        let preview = engine
            .capture("t", &draft, "", &BTreeMap::new(), true, &context())
            .unwrap();
        assert!(
            preview.diff.contains("TODO Preview only"),
            "{}",
            preview.diff
        );
        let item_ref = WorkspaceIndex::load(settings)
            .unwrap()
            .items()
            .next()
            .unwrap()
            .item_ref
            .clone();
        let done = EditIntent {
            state: Change::Set("DONE".to_string()),
            ..EditIntent::default()
        };
        engine.edit(&item_ref, &done, true, &context()).unwrap();
        assert_eq!(tree(org.path()), ["tasks.org"]);
        assert!(tree(data.path()).is_empty(), "{:?}", tree(data.path()));
    }
}
