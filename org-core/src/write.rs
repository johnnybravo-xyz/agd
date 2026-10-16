use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use std::sync::LazyLock;

use chrono::{Duration, Local, Months, NaiveDate, NaiveDateTime};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use similar::TextDiff;

use crate::{
    AgdError, ByteSpan, CaptureDraft, CaptureTemplate, ErrorCode, HeadlineSpan, IdPolicy,
    IndexedDocument, Item, LogKind, MUTATION_SCHEMA, Mutation, PlanningTimestamp, Repeat, RootSet,
    Settings, WorkspaceIndex, format_active_timestamp, render_capture_template,
};

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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum Change<T> {
    #[default]
    Keep,
    Set(T),
    Clear,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EditIntent {
    pub state: Change<String>,
    pub title: Change<String>,
    pub priority: Change<String>,
    pub tags_add: BTreeSet<String>,
    pub tags_remove: BTreeSet<String>,
    pub scheduled: Change<PlanningTimestamp>,
    pub deadline: Change<PlanningTimestamp>,
    pub properties: BTreeMap<String, Option<String>>,
    pub note: Option<String>,
}

/// One item and the edit to apply to it, for [`WriteEngine::edit_batch`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemEdit {
    pub item_ref: String,
    pub intent: EditIntent,
}

/// What [`WriteEngine::edit_batch`] wrote: one mutation per edit, in the
/// order given, all under one journal transaction; undoing any of its
/// journal ids, or the transaction id, reverts them all. A dry run's
/// transaction and journal ids are `dry-run`.
#[derive(Clone, Debug)]
pub struct BatchMutation {
    pub transaction: String,
    pub mutations: Vec<Mutation>,
}

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

    // MCP clients are journaled as "mcp:<client name>"; every other actor is a
    // person using the CLI or TUI.
    pub fn is_agent(&self) -> bool {
        self.actor.starts_with("mcp:")
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalStatus {
    Pending,
    Committed,
    Unapplied,
    Conflict,
    Reviewed,
    Undone,
    /// A conflict the user settled with `agd setup check --resolve`; the
    /// file was kept as it was.
    Resolved,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct JournalRecord {
    pub schema: String,
    pub id: String,
    pub status: JournalStatus,
    pub timestamp: String,
    pub actor: String,
    pub project: String,
    pub file: String,
    pub item_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transaction: Option<String>,
    pub pre_hash: String,
    pub post_hash: String,
    pub patches: Vec<StoredPatch>,
    pub diff: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UndoResult {
    pub schema: String,
    pub ok: bool,
    pub journal_id: String,
    pub file: String,
    pub diff: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RefileResult {
    pub schema: String,
    pub ok: bool,
    pub item: Item,
    pub diffs: Vec<String>,
    pub journal_ids: Vec<String>,
}

#[derive(Debug)]
pub struct WriteEngine {
    settings: Settings,
    roots: RootSet,
    journal_path: PathBuf,
}

/// The `journal_id` of a dry-run mutation, which writes no journal entry.
const DRY_RUN_JOURNAL_ID: &str = "dry-run";

/// One file's computed write: its bytes now, its bytes after, and the
/// journal patches and diff between them.
struct PreparedFile {
    path: PathBuf,
    pre_image: String,
    post_image: String,
    patches: Vec<StoredPatch>,
    diff: String,
}

impl PreparedFile {
    fn new(
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

    fn journal_image<'a>(&'a self, item_ref: Option<&'a str>) -> JournalImage<'a> {
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

/// What a capture renders, gathered so it can be prepared twice: once to
/// validate and preview, once under the write lock.
struct CaptureInput<'a> {
    template: &'a CaptureTemplate,
    draft: &'a CaptureDraft,
    body: &'a str,
    prompts: &'a BTreeMap<String, String>,
    now: NaiveDateTime,
}

struct PreparedCapture {
    file: PreparedFile,
    item: Item,
}

/// An edited item as it reads after the edit, and whether marking it done
/// repeated it.
struct EditedItem {
    item: Item,
    repeated: Option<Repeat>,
}

impl EditedItem {
    /// A done state requested for an item that is still not done after
    /// the edit means a repeater moved its dates on.
    fn new(item: Item, intent: &EditIntent, document: &IndexedDocument) -> Self {
        let repeated = match &intent.state {
            Change::Set(state) if document.todo_done().contains(state) && !item.done => {
                let repeating = |timestamp: &Option<PlanningTimestamp>| {
                    timestamp
                        .as_ref()
                        .filter(|timestamp| timestamp.repeater.is_some())
                        .map(|timestamp| timestamp.date.clone())
                };
                repeating(&item.planning.scheduled)
                    .or_else(|| repeating(&item.planning.deadline))
                    .map(|next_date| Repeat {
                        done_state: state.clone(),
                        next_date,
                    })
            }
            _ => None,
        };
        Self { item, repeated }
    }

    fn into_mutation(self, diff: String, journal_id: &str) -> Mutation {
        Mutation {
            repeated: self.repeated,
            ..mutation(self.item, diff, journal_id)
        }
    }
}

/// A refile's file images: one, or the source and then the target.
struct PreparedRefile {
    files: Vec<PreparedFile>,
    item: Item,
}

impl PreparedRefile {
    fn paths(&self) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = self.files.iter().map(|file| file.path.clone()).collect();
        paths.sort();
        paths
    }

    fn result(self, journal_ids: Vec<String>) -> RefileResult {
        RefileResult {
            schema: "agd.refile/v1".to_string(),
            ok: true,
            item: self.item,
            diffs: self.files.into_iter().map(|file| file.diff).collect(),
            journal_ids,
        }
    }
}

/// A batch's file images, sorted by path, and each edit's updated item and
/// file, in the order the edits were given.
struct PreparedBatch {
    files: Vec<PreparedFile>,
    /// The first edited item's ref in each file, for its journal record.
    file_refs: Vec<String>,
    items: Vec<Option<EditedItem>>,
    file_of: Vec<usize>,
}

impl PreparedBatch {
    /// The batch's result; `records` are the committed files' records, in
    /// file order, or empty for a dry run.
    fn result(self, transaction: String, records: &[JournalRecord]) -> BatchMutation {
        let mutations = self
            .items
            .into_iter()
            .zip(self.file_of)
            .map(|(item, file)| {
                let journal_id = records
                    .get(file)
                    .map_or(transaction.as_str(), |record| record.id.as_str());
                item.expect("every edit has an item")
                    .into_mutation(self.files[file].diff.clone(), journal_id)
            })
            .collect();
        BatchMutation {
            transaction,
            mutations,
        }
    }
}

struct JournalImage<'a> {
    path: &'a Path,
    item_ref: Option<&'a str>,
    pre_image: &'a str,
    post_image: &'a str,
    patches: &'a [StoredPatch],
    diff: &'a str,
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
        Self::new(settings, crate::paths::data_dir()?.join("journal.jsonl"))
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Applies one edit. A dry run computes the same diff from the current
    /// bytes and touches nothing: no lock, journal entry or file.
    pub fn edit(
        &self,
        item_ref: &str,
        intent: &EditIntent,
        dry_run: bool,
        context: &WriteContext,
    ) -> Result<Mutation, AgdError> {
        validate_intent(intent)?;
        let now = context.org_now();
        let initial = WorkspaceIndex::load(self.settings.clone())?;
        let (preview, item) = self.prepare_edit(&initial, item_ref, intent, now)?;
        let path = preview.path.clone();
        self.roots.resolve_write(&path, false)?;
        refuse_emacs_lock(&path)?;
        if dry_run {
            return Ok(item.into_mutation(preview.diff, DRY_RUN_JOURNAL_ID));
        }

        let _locks = self.lock_paths(std::slice::from_ref(&path))?;
        let fresh = WorkspaceIndex::load(self.settings.clone())?;
        let (prepared, item) = self.prepare_edit(&fresh, item_ref, intent, now)?;
        if prepared.path != path {
            return Err(AgdError::new(
                "stale_ref",
                "item resolved to a different file while acquiring the write lock",
            ));
        }
        let record = self.commit_image(prepared.journal_image(Some(item_ref)), context)?;
        Ok(item.into_mutation(prepared.diff, &record.id))
    }

    /// The edit's patches against the item's file as `index` read it.
    fn prepare_edit(
        &self,
        index: &WorkspaceIndex,
        item_ref: &str,
        intent: &EditIntent,
        now: NaiveDateTime,
    ) -> Result<(PreparedFile, EditedItem), AgdError> {
        let (document, headline) = index.resolve(item_ref)?;
        let path = document.path().to_path_buf();
        validate_intent_for_file(intent, document)?;
        validate_id_change(intent, headline, index)?;
        let patches = build_edit_patches(document, headline, intent, &self.settings, now)?;
        let (post_image, stored) = apply_patches(document.source(), patches)?;
        if post_image == document.source() {
            return Err(AgdError::new("no_change", "edit does not change the item"));
        }
        let item = parse_updated_item(&path, &post_image, headline.item.line, &self.settings)?;
        let prepared = PreparedFile::new(path, document.source().to_string(), post_image, stored);
        Ok((prepared, EditedItem::new(item, intent, document)))
    }

    /// Captures through a template. A dry run renders the same diff and
    /// touches nothing: no directory, lock, journal entry or file. A real
    /// capture is computed once before any directory is created, so a
    /// refused capture leaves nothing behind either.
    pub fn capture(
        &self,
        template_key: &str,
        draft: &CaptureDraft,
        body: &str,
        prompts: &BTreeMap<String, String>,
        dry_run: bool,
        context: &WriteContext,
    ) -> Result<Mutation, AgdError> {
        let template = self
            .settings
            .capture_template(template_key)
            .ok_or_else(|| {
                let keys: Vec<&str> = self
                    .settings
                    .captures
                    .iter()
                    .map(|t| t.key.as_str())
                    .collect();
                AgdError::new(
                    "unknown_template",
                    format!(
                        "capture template '{template_key}' is not configured; templates: {}",
                        keys.join(", ")
                    ),
                )
            })?;
        let (template, draft) = apply_agent_capture_gate(template, draft, &self.settings, context);
        validate_capture_input(&draft, body, prompts)?;
        let input = CaptureInput {
            template: &template,
            draft: &draft,
            body,
            prompts,
            now: context.org_now(),
        };
        let path = self.capture_path(&template)?;
        let preview = self.prepare_capture(&path, &input)?;
        refuse_emacs_lock(&path)?;
        if dry_run {
            return Ok(mutation(
                preview.item,
                preview.file.diff,
                DRY_RUN_JOURNAL_ID,
            ));
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(AgdError::from)?;
        }
        self.roots.resolve_write(&path, true)?;
        let _locks = self.lock_paths(std::slice::from_ref(&path))?;
        let prepared = self.prepare_capture(&path, &input)?;
        let record = self.commit_image(
            prepared.file.journal_image(Some(&prepared.item.item_ref)),
            context,
        )?;
        Ok(mutation(prepared.item, prepared.file.diff, &record.id))
    }

    /// The canonical path a template captures into, confined to the roots,
    /// whether or not the file or its directories exist yet. An `id` target
    /// is the file of the one heading with that ID.
    fn capture_path(&self, template: &CaptureTemplate) -> Result<PathBuf, AgdError> {
        if let Some(id) = &template.id {
            let index = WorkspaceIndex::load(self.settings.clone())?;
            let (document, _) = index
                .resolve(id)
                .map_err(|error| capture_id_error(&template.key, id, error))?;
            return self.roots.resolve_write(document.path(), false);
        }
        let file = template.file.as_deref().ok_or_else(|| {
            AgdError::new(
                "unsupported_target",
                format!(
                    "capture template '{}' names no file or id target",
                    template.key
                ),
            )
        })?;
        // `~` expands as in roots, before the path is confined to them.
        let file = shellexpand::tilde(file);
        let path = self.roots.resolve_write(Path::new(file.as_ref()), true)?;
        // Canonical like every indexed path, so the journal, refs and the
        // index agree (on macOS /var is /private/var).
        let path = crate::paths::resolve_existing_prefix(&path)?;
        self.roots.resolve_write(&path, true)
    }

    /// The capture's insertion into `path` as it is on disk now.
    fn prepare_capture(
        &self,
        path: &Path,
        input: &CaptureInput<'_>,
    ) -> Result<PreparedCapture, AgdError> {
        let pre_image = read_org_file(path)?;
        validate_capture_for_file(input.draft, &parse_text(path, &pre_image, &self.settings)?)?;
        let rendered = render_capture_template(
            input.template,
            input.draft,
            input.body,
            input.prompts,
            input.now,
        )?;
        let newline = line_ending(&pre_image);
        let rendered =
            append_body_without_escape(rendered, &input.template.template, input.body, newline);
        let placement = place_capture(
            path,
            &pre_image,
            input.template,
            input.now.date(),
            &self.settings,
        )?;
        let entry = capture_entry(&rendered, placement.level, newline)?;
        let entry = align_first_line_tags(&entry, self.settings.tags_column);
        let (post_image, entry_start) =
            insert_capture_entry(&placement.text, placement.offset, &entry, newline);
        let patch = changed_lines_patch(&pre_image, &post_image);
        let (post_image, stored) = apply_patches(&pre_image, vec![patch])?;
        let line = post_image[..entry_start]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1;
        let item = parse_updated_item(path, &post_image, line, &self.settings)?;
        if let Some(id) = &item.id {
            refuse_used_id(id, &WorkspaceIndex::load(self.settings.clone())?)?;
        }
        Ok(PreparedCapture {
            file: PreparedFile::new(path.to_path_buf(), pre_image, post_image, stored),
            item,
        })
    }

    /// Moves an item's subtree under another heading, in one file or
    /// across two as one journal transaction (the target written first, so
    /// an interruption never loses the item). A dry run touches nothing.
    pub fn refile(
        &self,
        item_ref: &str,
        target_ref: &str,
        dry_run: bool,
        context: &WriteContext,
    ) -> Result<RefileResult, AgdError> {
        let initial = WorkspaceIndex::load(self.settings.clone())?;
        let preview = self.prepare_refile(&initial, item_ref, target_ref)?;
        let paths = preview.paths();
        for path in &paths {
            self.roots.resolve_write(path, false)?;
            refuse_emacs_lock(path)?;
        }
        if dry_run {
            return Ok(preview.result(vec![DRY_RUN_JOURNAL_ID.to_string()]));
        }

        let _locks = self.lock_paths(&paths)?;
        let fresh = WorkspaceIndex::load(self.settings.clone())?;
        let prepared = self.prepare_refile(&fresh, item_ref, target_ref)?;
        if prepared.paths() != paths {
            return Err(AgdError::new(
                "stale_ref",
                "refile resolved to different files while acquiring the write locks",
            ));
        }
        let journal_ids = match prepared.files.as_slice() {
            [same_file] => vec![
                self.commit_image(same_file.journal_image(Some(item_ref)), context)?
                    .id,
            ],
            [source, target] => {
                let records = self.commit_transaction(
                    &[
                        target.journal_image(Some(target_ref)),
                        source.journal_image(Some(item_ref)),
                    ],
                    context,
                )?;
                vec![records[1].id.clone(), records[0].id.clone()]
            }
            _ => unreachable!("a refile writes one or two files"),
        };
        Ok(prepared.result(journal_ids))
    }

    /// The refile's file images (source first) against `index`.
    fn prepare_refile(
        &self,
        index: &WorkspaceIndex,
        item_ref: &str,
        target_ref: &str,
    ) -> Result<PreparedRefile, AgdError> {
        let (source_document, source_headline) = index.resolve(item_ref)?;
        let (target_document, target_headline) = index.resolve(target_ref)?;
        let same_file = source_document.path() == target_document.path();
        if same_file
            && target_headline.line_span.start >= source_headline.subtree_span.start
            && target_headline.line_span.start < source_headline.subtree_span.end
        {
            return Err(AgdError::new(
                "invalid_refile",
                "an item cannot be refiled into itself or one of its descendants",
            ));
        }
        let moved = &source_document.source()
            [source_headline.subtree_span.start..source_headline.subtree_span.end];
        let moved = relevel_subtree(moved, target_headline.level + 1, source_headline.level)?;
        let moved = normalize_insertion(
            target_document.source(),
            target_headline.subtree_span.end,
            moved,
        );
        let removal = Patch {
            span: source_headline.subtree_span,
            replacement: String::new(),
        };
        let insertion = Patch {
            span: ByteSpan {
                start: target_headline.subtree_span.end,
                end: target_headline.subtree_span.end,
            },
            replacement: moved,
        };
        let prepare = |document: &IndexedDocument, patches: Vec<Patch>| {
            apply_patches(document.source(), patches).map(|(post_image, stored)| {
                PreparedFile::new(
                    document.path().to_path_buf(),
                    document.source().to_string(),
                    post_image,
                    stored,
                )
            })
        };
        let files = if same_file {
            vec![prepare(source_document, vec![removal, insertion])?]
        } else {
            vec![
                prepare(source_document, vec![removal])?,
                prepare(target_document, vec![insertion])?,
            ]
        };
        let target_file = files.last().expect("a refile writes a file");
        let item = find_refiled_item(
            &target_file.path,
            &target_file.post_image,
            &source_headline.item,
            &target_headline.item,
            &self.settings,
        )?;
        Ok(PreparedRefile { files, item })
    }

    /// Applies several edits as one transaction: every target is resolved
    /// and its edit computed first, and if any is refused nothing is
    /// written and the error names that target. Otherwise each file is
    /// written once and journaled under one transaction, so one undo
    /// reverts them all. A dry run touches nothing.
    pub fn edit_batch(
        &self,
        edits: &[ItemEdit],
        dry_run: bool,
        context: &WriteContext,
    ) -> Result<BatchMutation, AgdError> {
        if edits.is_empty() {
            return Err(AgdError::new("invalid_edit", "nothing to edit"));
        }
        for edit in edits {
            validate_intent(&edit.intent).map_err(|error| error.with_context(&edit.item_ref))?;
        }
        let now = context.org_now();
        let initial = WorkspaceIndex::load(self.settings.clone())?;
        let preview = self.prepare_batch(&initial, edits, now)?;
        let paths: Vec<PathBuf> = preview.files.iter().map(|file| file.path.clone()).collect();
        for path in &paths {
            self.roots.resolve_write(path, false)?;
            refuse_emacs_lock(path)?;
        }
        if dry_run {
            return Ok(preview.result(DRY_RUN_JOURNAL_ID.to_string(), &[]));
        }

        let _locks = self.lock_paths(&paths)?;
        let fresh = WorkspaceIndex::load(self.settings.clone())?;
        let prepared = self.prepare_batch(&fresh, edits, now)?;
        let fresh_paths: Vec<PathBuf> = prepared
            .files
            .iter()
            .map(|file| file.path.clone())
            .collect();
        if fresh_paths != paths {
            return Err(AgdError::new(
                "stale_ref",
                "targets resolved to different files while acquiring the write locks",
            ));
        }
        let images: Vec<JournalImage<'_>> = prepared
            .files
            .iter()
            .zip(&prepared.file_refs)
            .map(|(file, item_ref)| file.journal_image(Some(item_ref)))
            .collect();
        let records = self.commit_transaction(&images, context)?;
        let transaction = records
            .first()
            .and_then(|record| record.transaction.clone())
            .unwrap_or_default();
        Ok(prepared.result(transaction, &records))
    }

    /// Every edit's patches, grouped per file, against `index`.
    fn prepare_batch(
        &self,
        index: &WorkspaceIndex,
        edits: &[ItemEdit],
        now: NaiveDateTime,
    ) -> Result<PreparedBatch, AgdError> {
        let mut batch = PreparedBatch {
            files: Vec::new(),
            file_refs: Vec::new(),
            items: (0..edits.len()).map(|_| None).collect(),
            file_of: vec![0; edits.len()],
        };
        for (path, targets) in resolve_batch_targets(index, edits)? {
            let document = index.document(&path).expect("a resolved item's document");
            let (file, items) = self.prepare_batch_file(document, &targets, edits, now)?;
            for (position, item) in items {
                batch.items[position] = Some(item);
                batch.file_of[position] = batch.files.len();
            }
            batch.file_refs.push(edits[targets[0].0].item_ref.clone());
            batch.files.push(file);
        }
        Ok(batch)
    }

    /// One file's edits applied together: each target's own patches, all
    /// applied at once, and each edited item found again in the result.
    fn prepare_batch_file(
        &self,
        document: &IndexedDocument,
        targets: &[(usize, &HeadlineSpan)],
        edits: &[ItemEdit],
        now: NaiveDateTime,
    ) -> Result<(PreparedFile, Vec<(usize, EditedItem)>), AgdError> {
        let mut patches = Vec::new();
        for (position, headline) in targets {
            let label = |error: AgdError| error.with_context(&headline.item.title);
            let intent = &edits[*position].intent;
            let own = build_edit_patches(document, headline, intent, &self.settings, now)
                .map_err(label)?;
            let (alone, _) = apply_patches(document.source(), own.clone()).map_err(label)?;
            if alone == document.source() {
                return Err(label(AgdError::new(
                    "no_change",
                    "edit does not change the item",
                )));
            }
            patches.extend(own);
        }
        let starts: Vec<(usize, usize)> = targets
            .iter()
            .map(|(position, headline)| {
                (
                    *position,
                    post_image_offset(&patches, headline.line_span.start),
                )
            })
            .collect();
        let (post_image, stored) = apply_patches(document.source(), patches)?;
        let updated = parse_text(document.path(), &post_image, &self.settings)?;
        let mut items = Vec::with_capacity(starts.len());
        for (position, start) in starts {
            let line = post_image[..start]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1;
            let item = updated
                .headline_at_line(line)
                .map(|headline| headline.item.clone())
                .ok_or_else(|| AgdError::new("parse_error", "edit produced no target headline"))?;
            items.push((
                position,
                EditedItem::new(item, &edits[position].intent, document),
            ));
        }
        let file = PreparedFile::new(
            document.path().to_path_buf(),
            document.source().to_string(),
            post_image,
            stored,
        );
        Ok((file, items))
    }

    /// Reverts one journal entry, or every file of its transaction, given
    /// the entry's id or the transaction id. A change is reverted only where
    /// the file still holds what it wrote; otherwise nothing is written and
    /// the error says what changed and what to do.
    pub fn undo(&self, journal_id: &str, context: &WriteContext) -> Result<UndoResult, AgdError> {
        let paths: Vec<PathBuf> = self
            .journal_group(journal_id)?
            .iter()
            .map(|record| PathBuf::from(&record.file))
            .collect();
        for path in &paths {
            self.roots.resolve_write(path, false)?;
        }
        let _locks = self.lock_paths(&paths)?;
        let group = self.journal_group(journal_id)?;
        if let Some(record) = group.iter().find(|record| {
            !matches!(
                record.status,
                JournalStatus::Committed | JournalStatus::Reviewed
            )
        }) {
            return Err(AgdError::new(
                ErrorCode::NothingToUndo,
                format!(
                    "journal entry {} is {:?}, not a committed change; nothing to undo",
                    record.id, record.status
                ),
            ));
        }
        let reverts: Vec<PreparedFile> = group
            .iter()
            .map(|record| self.prepare_revert(record))
            .collect::<Result<_, _>>()?;
        let images: Vec<JournalImage<'_>> = reverts
            .iter()
            .map(|file| file.journal_image(None))
            .collect();
        replace_all_or_restore(&images)?;
        let timestamp = Local::now().to_rfc3339();
        for mut record in group.clone() {
            record.status = JournalStatus::Undone;
            record.timestamp = timestamp.clone();
            record.actor = context.actor.clone();
            record.project = context.project.clone();
            self.append_record(&record)?;
        }
        Ok(UndoResult {
            schema: "agd.undo/v1".to_string(),
            ok: true,
            journal_id: group[0]
                .transaction
                .clone()
                .unwrap_or_else(|| group[0].id.clone()),
            file: group
                .iter()
                .map(|record| record.file.as_str())
                .collect::<Vec<_>>()
                .join(","),
            diff: reverts
                .into_iter()
                .map(|file| file.diff)
                .collect::<Vec<_>>()
                .join("\n"),
        })
    }

    /// The latest records of one entry, or of every file in its
    /// transaction, by entry or transaction id, sorted by file.
    fn journal_group(&self, journal_id: &str) -> Result<Vec<JournalRecord>, AgdError> {
        let records = self.latest_records()?;
        let transaction = match records.get(journal_id) {
            Some(record) => record.transaction.clone(),
            None => Some(journal_id.to_string()),
        };
        let mut group: Vec<JournalRecord> = match &transaction {
            Some(transaction) => records
                .values()
                .filter(|record| record.transaction.as_ref() == Some(transaction))
                .cloned()
                .collect(),
            None => vec![records[journal_id].clone()],
        };
        if group.is_empty() {
            return Err(AgdError::new(
                ErrorCode::JournalNotFound,
                format!("journal entry not found: {journal_id}"),
            ));
        }
        group.sort_by(|left, right| left.file.cmp(&right.file));
        Ok(group)
    }

    /// The file with the record's change reverted, from its bytes now.
    fn prepare_revert(&self, record: &JournalRecord) -> Result<PreparedFile, AgdError> {
        let path = PathBuf::from(&record.file);
        let current = fs::read_to_string(&path).map_err(|error| {
            AgdError::with_source(
                "io",
                format!("cannot read {}: {error}", path.display()),
                error,
            )
        })?;
        let inverse = relocate_inverse(&current, record)
            .map_err(|_| self.undo_conflict_error(record, &current))?;
        let (reverted, stored) = apply_patches(&current, inverse)?;
        if content_hash(&current) == record.post_hash && content_hash(&reverted) != record.pre_hash
        {
            return Err(AgdError::new(
                "journal_corrupt",
                format!(
                    "inverse patch for {} does not reproduce its pre-image",
                    record.id
                ),
            ));
        }
        Ok(PreparedFile::new(path, current, reverted, stored))
    }

    /// `can't undo: Rotate certs changed after codex edited it
    /// (work.org:42); edit it by hand`: the heading the change touched, as
    /// the file reads now, and who made the change.
    fn undo_conflict_error(&self, record: &JournalRecord, current: &str) -> AgdError {
        let path = Path::new(&record.file);
        let location = self
            .roots
            .roots()
            .iter()
            .find_map(|root| path.strip_prefix(root).ok())
            .unwrap_or(path)
            .display()
            .to_string();
        let author = record.actor.strip_prefix("mcp:").unwrap_or("you");
        let message = match touched_heading(path, current, record, &self.settings) {
            Some((title, line)) => format!(
                "can't undo: {title} changed after {author} edited it ({location}:{line}); edit it by hand"
            ),
            None => {
                format!("can't undo: {location} changed after {author} edited it; edit it by hand")
            }
        };
        AgdError::new(ErrorCode::UndoConflict, message)
    }

    pub fn review(&self, journal_id: &str) -> Result<(), AgdError> {
        let records = self.latest_records()?;
        let mut record = records.get(journal_id).cloned().ok_or_else(|| {
            AgdError::new(
                "journal_not_found",
                format!("journal entry not found: {journal_id}"),
            )
        })?;
        if record.status != JournalStatus::Committed {
            return Err(AgdError::new(
                "invalid_journal_state",
                format!("journal entry {journal_id} is not awaiting review"),
            ));
        }
        record.status = JournalStatus::Reviewed;
        record.timestamp = Local::now().to_rfc3339();
        self.append_record(&record)
    }

    pub fn journal(&self) -> Result<Vec<JournalRecord>, AgdError> {
        let mut records: Vec<JournalRecord> = self.latest_records()?.into_values().collect();
        records.sort_by(|left, right| right.timestamp.cmp(&left.timestamp));
        Ok(records)
    }

    /// Writes several files as one journal transaction, in the order given:
    /// every pending record first, then each file; when a file cannot be
    /// written the ones before it are restored. Returns the committed
    /// records in the same order.
    fn commit_transaction(
        &self,
        images: &[JournalImage<'_>],
        context: &WriteContext,
    ) -> Result<Vec<JournalRecord>, AgdError> {
        let transaction = journal_id();
        let mut records: Vec<JournalRecord> = images
            .iter()
            .enumerate()
            .map(|(position, image)| {
                pending_record(
                    format!("{transaction}:{position}"),
                    Some(transaction.clone()),
                    image,
                    context,
                )
            })
            .collect();
        for record in &records {
            self.append_record(record)?;
        }
        replace_all_or_restore(images)?;
        let timestamp = Local::now().to_rfc3339();
        for record in &mut records {
            record.status = JournalStatus::Committed;
            record.timestamp = timestamp.clone();
            self.append_record(record)?;
        }
        Ok(records)
    }

    /// Locks `paths` in sorted order (so two writers never deadlock),
    /// refuses Emacs-locked files, and settles interrupted writes to them.
    fn lock_paths(&self, paths: &[PathBuf]) -> Result<Vec<File>, AgdError> {
        let mut sorted = paths.to_vec();
        sorted.sort();
        sorted.dedup();
        let mut locks = Vec::with_capacity(sorted.len());
        for path in &sorted {
            locks.push(self.lock_target(path)?);
            refuse_emacs_lock(path)?;
        }
        self.recover_files(&sorted)?;
        Ok(locks)
    }

    fn commit_image(
        &self,
        image: JournalImage<'_>,
        context: &WriteContext,
    ) -> Result<JournalRecord, AgdError> {
        let mut record = pending_record(journal_id(), None, &image, context);
        self.append_record(&record)?;
        atomic_replace(image.path, image.post_image.as_bytes())?;
        record.status = JournalStatus::Committed;
        record.timestamp = Local::now().to_rfc3339();
        self.append_record(&record)?;
        Ok(record)
    }

    /// Takes the advisory write lock for `path`. Lock files live in agd's
    /// data directory (`locks/<sha256 of the path>.lock` beside the journal),
    /// never in the org directories, so synced folders stay clean.
    fn lock_target(&self, path: &Path) -> Result<File, AgdError> {
        let directory = self.lock_directory();
        create_private_directory(&directory)?;
        let lock_path = directory.join(format!(
            "{}.lock",
            sha256_hex(path.as_os_str().as_encoded_bytes())
        ));
        let lock = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|error| {
                AgdError::with_source(
                    "lock_failed",
                    format!("cannot open write lock {}: {error}", lock_path.display()),
                    error,
                )
            })?;
        lock.lock().map_err(|error| {
            AgdError::with_source(
                "lock_failed",
                format!("cannot lock {}: {error}", lock_path.display()),
                error,
            )
        })?;
        Ok(lock)
    }

    fn lock_directory(&self) -> PathBuf {
        self.journal_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("locks")
    }

    fn append_record(&self, record: &JournalRecord) -> Result<(), AgdError> {
        if let Some(parent) = self.journal_path.parent() {
            create_private_directory(parent)?;
        }
        let serialized = serde_json::to_vec(record).map_err(|error| {
            AgdError::new(
                "journal_error",
                format!("cannot serialize journal record: {error}"),
            )
        })?;
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&self.journal_path).map_err(AgdError::from)?;
        file.write_all(&serialized).map_err(AgdError::from)?;
        file.write_all(b"\n").map_err(AgdError::from)?;
        file.sync_all().map_err(AgdError::from)
    }

    fn latest_records(&self) -> Result<BTreeMap<String, JournalRecord>, AgdError> {
        let source = match fs::read_to_string(&self.journal_path) {
            Ok(source) => source,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(BTreeMap::new());
            }
            Err(error) => return Err(AgdError::from(error)),
        };
        let mut records = BTreeMap::new();
        for (line, raw) in source.lines().enumerate() {
            let record: JournalRecord = serde_json::from_str(raw).map_err(|error| {
                AgdError::new(
                    "journal_corrupt",
                    format!("invalid journal JSON at line {}: {error}", line + 1),
                )
            })?;
            records.insert(record.id.clone(), record);
        }
        Ok(records)
    }

    /// Settles interrupted writes to `paths`, which the caller holds
    /// locked: a pending entry whose file holds its post-image was
    /// committed, one whose file still holds its pre-image never applied.
    /// Any other pending entry is a conflict only the user can settle, so
    /// the write is refused with the command that settles it.
    fn recover_files(&self, paths: &[PathBuf]) -> Result<(), AgdError> {
        for record in self.unsettled_records()? {
            if !paths.iter().any(|path| Path::new(&record.file) == path) {
                continue;
            }
            let record = self.settle(record)?;
            if record.status == JournalStatus::Conflict {
                return Err(journal_conflict_error(&record));
            }
        }
        Ok(())
    }

    /// Interrupted writes whose file now holds neither their pre- nor their
    /// post-image. Reads the journal and the files only.
    pub fn journal_conflicts(&self) -> Result<Vec<JournalRecord>, AgdError> {
        let mut conflicts = Vec::new();
        for record in self.unsettled_records()? {
            if settled_status(&record)? == JournalStatus::Conflict {
                conflicts.push(record);
            }
        }
        Ok(conflicts)
    }

    /// Settles every interrupted write: recoverable ones as committed or
    /// unapplied, and conflicts as resolved, keeping each file exactly as
    /// it is on disk. Returns the entries marked resolved.
    pub fn resolve_conflicts(
        &self,
        context: &WriteContext,
    ) -> Result<Vec<JournalRecord>, AgdError> {
        let mut resolved = Vec::new();
        for record in self.unsettled_records()? {
            let _lock = self.lock_target(Path::new(&record.file))?;
            let mut record = self.settle(record)?;
            if record.status != JournalStatus::Conflict {
                continue;
            }
            record.status = JournalStatus::Resolved;
            record.timestamp = Local::now().to_rfc3339();
            record.actor = context.actor.clone();
            record.project = context.project.clone();
            self.append_record(&record)?;
            resolved.push(record);
        }
        Ok(resolved)
    }

    /// Latest records still pending or in conflict, oldest first.
    fn unsettled_records(&self) -> Result<Vec<JournalRecord>, AgdError> {
        let mut records: Vec<JournalRecord> = self
            .latest_records()?
            .into_values()
            .filter(|record| {
                matches!(
                    record.status,
                    JournalStatus::Pending | JournalStatus::Conflict
                )
            })
            .collect();
        records.sort_by(|left, right| left.timestamp.cmp(&right.timestamp));
        Ok(records)
    }

    /// Records the status the file's bytes give an unsettled record.
    fn settle(&self, mut record: JournalRecord) -> Result<JournalRecord, AgdError> {
        let status = settled_status(&record)?;
        if status != record.status {
            record.status = status;
            record.timestamp = Local::now().to_rfc3339();
            self.append_record(&record)?;
        }
        Ok(record)
    }
}

/// What the file on disk says about an interrupted write: its post-image
/// means committed, its pre-image unapplied, anything else a conflict.
fn settled_status(record: &JournalRecord) -> Result<JournalStatus, AgdError> {
    let current = content_hash(&read_org_file(Path::new(&record.file))?);
    Ok(if current == record.post_hash {
        JournalStatus::Committed
    } else if current == record.pre_hash {
        JournalStatus::Unapplied
    } else {
        JournalStatus::Conflict
    })
}

fn journal_conflict_error(record: &JournalRecord) -> AgdError {
    AgdError::new(
        ErrorCode::JournalConflict,
        format!(
            "{} changed after an interrupted agd write (journal entry {}); check the file, then run `agd setup check --resolve` to keep it as it is",
            record.file, record.id
        ),
    )
}

fn pending_record(
    id: String,
    transaction: Option<String>,
    image: &JournalImage<'_>,
    context: &WriteContext,
) -> JournalRecord {
    JournalRecord {
        schema: "agd.journal/v1".to_string(),
        id,
        status: JournalStatus::Pending,
        timestamp: Local::now().to_rfc3339(),
        actor: context.actor.clone(),
        project: context.project.clone(),
        file: image.path.to_string_lossy().into_owned(),
        item_ref: image.item_ref.map(str::to_string),
        transaction,
        pre_hash: content_hash(image.pre_image),
        post_hash: content_hash(image.post_image),
        patches: image.patches.to_vec(),
        diff: image.diff.to_string(),
    }
}

fn relevel_subtree(
    source: &str,
    target_level: usize,
    source_level: usize,
) -> Result<String, AgdError> {
    let delta = target_level as isize - source_level as isize;
    let mut output = String::with_capacity(source.len());
    for line in source.split_inclusive('\n') {
        let stars = line.bytes().take_while(|byte| *byte == b'*').count();
        if stars > 0
            && line
                .as_bytes()
                .get(stars)
                .is_some_and(u8::is_ascii_whitespace)
        {
            let level = stars as isize + delta;
            if level < 1 {
                return Err(AgdError::new(
                    "invalid_refile",
                    "refile would produce an invalid headline level",
                ));
            }
            output.push_str(&"*".repeat(level as usize));
            output.push_str(&line[stars..]);
        } else {
            output.push_str(line);
        }
    }
    Ok(output)
}

fn normalize_insertion(target: &str, offset: usize, moved: String) -> String {
    let newline = line_ending(target);
    let mut insertion = String::new();
    if offset > 0 && !target[..offset].ends_with('\n') {
        insertion.push_str(newline);
    }
    insertion.push_str(&moved);
    if !insertion.ends_with('\n') {
        insertion.push_str(newline);
    }
    insertion
}

fn find_refiled_item(
    path: &Path,
    source: &str,
    moved: &Item,
    target: &Item,
    settings: &Settings,
) -> Result<Item, AgdError> {
    let document =
        IndexedDocument::parse(path.to_path_buf(), source.as_bytes().to_vec(), settings)?;
    if let Some(id) = &moved.id
        && let Some(headline) = document
            .headlines()
            .iter()
            .find(|headline| headline.item.id.as_ref() == Some(id))
    {
        return Ok(headline.item.clone());
    }
    let mut expected = target.olpath.clone();
    expected.push(moved.title.clone());
    document
        .headlines()
        .iter()
        .find(|headline| headline.item.olpath == expected)
        .map(|headline| headline.item.clone())
        .ok_or_else(|| AgdError::new("parse_error", "refiled item was not found at its target"))
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
                "invalid_patch",
                format!(
                    "patch {}..{} overlaps or exceeds the source",
                    patch.span.start, patch.span.end
                ),
            ));
        }
        if !source.is_char_boundary(patch.span.start) || !source.is_char_boundary(patch.span.end) {
            return Err(AgdError::new(
                "invalid_patch",
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

/// Inverse patches against `current`. An unchanged file uses the recorded
/// spans; otherwise each patch must be found exactly once by its context and
/// replaced bytes, so undo never touches text someone changed since.
fn relocate_inverse(current: &str, record: &JournalRecord) -> Result<Vec<Patch>, AgdError> {
    if content_hash(current) == record.post_hash {
        return Ok(inverse_patches(&record.patches));
    }
    let conflict = |detail: String| {
        AgdError::new(
            "undo_conflict",
            format!(
                "{} changed after journal entry {}: {detail}",
                record.file, record.id
            ),
        )
    };
    let mut patches = Vec::with_capacity(record.patches.len());
    for patch in &record.patches {
        let (Some(before), Some(after)) = (&patch.context_before, &patch.context_after) else {
            return Err(conflict("entry predates context-aware undo".to_string()));
        };
        let needle = format!("{before}{}{after}", patch.after);
        let mut found = current.match_indices(&needle);
        let (Some((position, _)), None) = (found.next(), found.next()) else {
            return Err(conflict(format!(
                "the edited text near byte {} changed",
                patch.start
            )));
        };
        let start = position + before.len();
        patches.push(Patch {
            span: ByteSpan {
                start,
                end: start + patch.after.len(),
            },
            replacement: patch.before.clone(),
        });
    }
    Ok(patches)
}

fn inverse_patches(patches: &[StoredPatch]) -> Vec<Patch> {
    patches
        .iter()
        .map(|patch| Patch {
            span: ByteSpan {
                start: patch.start,
                end: patch.end,
            },
            replacement: patch.before.clone(),
        })
        .collect()
}

fn build_edit_patches(
    document: &IndexedDocument,
    headline: &HeadlineSpan,
    intent: &EditIntent,
    settings: &Settings,
    now: NaiveDateTime,
) -> Result<Vec<Patch>, AgdError> {
    let source = document.source();
    let logging = document.logging();
    let stamp = format!("[{}]", now.format("%Y-%m-%d %a %H:%M"));
    let mut patches = Vec::new();
    let previous_state = headline.item.todo.clone();
    let mut state = change_value(&headline.item.todo, &intent.state);
    if let Some(requested) = &state
        && !document.todo_active().contains(requested)
        && !document.todo_done().contains(requested)
    {
        return Err(AgdError::new(
            "invalid_todo",
            format!("TODO state '{requested}' is not enabled for this file"),
        ));
    }
    let mut scheduled = planning_change(&headline.item.planning.scheduled, &intent.scheduled);
    let mut deadline = planning_change(&headline.item.planning.deadline, &intent.deadline);
    let mut closed = headline.item.planning.closed.clone();
    let mut properties = intent.properties.clone();
    if settings.ids == IdPolicy::AddOnWrite
        && headline.item.id.is_none()
        && !properties.contains_key("ID")
    {
        // Same shape as Emacs org-id with the default uuid method.
        properties.insert("ID".to_string(), Some(uuid::Uuid::new_v4().to_string()));
    }
    // LOGBOOK entries in Org's org-log-note-headings format, newest first.
    let mut log: Vec<LogEntry> = Vec::new();
    let mut note = intent.note.clone();
    for (old, new, logged, moved, removed) in [
        (
            &headline.item.planning.scheduled,
            &scheduled,
            logging.reschedule,
            "Rescheduled from",
            "Not scheduled, was",
        ),
        (
            &headline.item.planning.deadline,
            &deadline,
            logging.redeadline,
            "New deadline from",
            "Removed deadline, was",
        ),
    ] {
        let (Some(kind), Some(old)) = (logged, old) else {
            continue;
        };
        let heading = match new {
            Some(new) if format_active_timestamp(new)? == format_active_timestamp(old)? => {
                continue;
            }
            Some(_) => format!("{moved} \"{}\" on {stamp}", inactive_timestamp(old)?),
            None => format!("{removed} \"{}\" on {stamp}", inactive_timestamp(old)?),
        };
        log.push(LogEntry::new(heading, kind, &mut note));
    }

    let state_changed = !matches!(intent.state, Change::Keep) && state != previous_state;
    let requested_done = state_changed
        && state
            .as_ref()
            .is_some_and(|value| done_keywords(document).contains(value));
    let repeating = scheduled
        .as_ref()
        .is_some_and(|timestamp| timestamp.repeater.is_some())
        || deadline
            .as_ref()
            .is_some_and(|timestamp| timestamp.repeater.is_some());
    let state_heading = |to: &str| {
        let from = previous_state
            .as_deref()
            .map(|value| format!("\"{value}\""))
            .unwrap_or_default();
        format!("State {:<12} from {from:<12} {stamp}", format!("\"{to}\""))
    };
    // A keyword's own `!`/`@` marker on entering, or the previous one's on
    // leaving, decides whether the change is logged (org-todo-log-states).
    let keyword_log = state.as_deref().and_then(|value| {
        document
            .todo_keyword(value)
            .and_then(|keyword| keyword.log_enter)
            .or_else(|| {
                previous_state
                    .as_deref()
                    .and_then(|value| document.todo_keyword(value))
                    .and_then(|keyword| keyword.log_leave)
            })
    });
    if requested_done && repeating {
        // org-auto-repeat-maybe: shift the dates, return to the head of the
        // previous keyword's sequence, drop CLOSED, log only per org-log-repeat.
        if let Some(timestamp) = scheduled.as_mut()
            && timestamp.repeater.is_some()
        {
            advance_repeater(timestamp, now.date())?;
        }
        if let Some(timestamp) = deadline.as_mut()
            && timestamp.repeater.is_some()
        {
            advance_repeater(timestamp, now.date())?;
        }
        let done_word = state.clone().unwrap_or_default();
        state = sequence_head(document, previous_state.as_deref());
        closed = None;
        if let Some(kind) = logging.repeat {
            properties.insert("LAST_REPEAT".to_string(), Some(stamp.clone()));
            log.insert(0, LogEntry::new(state_heading(&done_word), kind, &mut note));
        }
    } else if requested_done {
        if logging.done.is_some() {
            closed = Some(PlanningTimestamp {
                date: now.format("%Y-%m-%d").to_string(),
                time: Some(now.format("%H:%M").to_string()),
                end_time: None,
                repeater: None,
                warning: None,
                raw: None,
            });
        }
        let done_word = state.clone().unwrap_or_default();
        if let Some(kind) = keyword_log {
            log.insert(0, LogEntry::new(state_heading(&done_word), kind, &mut note));
        } else if logging.done == Some(LogKind::Note) {
            log.insert(
                0,
                LogEntry::new(format!("CLOSING NOTE {stamp}"), LogKind::Note, &mut note),
            );
        }
    } else if state_changed {
        closed = None;
        if let Some(kind) = keyword_log {
            let to = state.clone().unwrap_or_default();
            log.insert(0, LogEntry::new(state_heading(&to), kind, &mut note));
        }
    }
    if let Some(text) = note.take() {
        log.insert(
            0,
            LogEntry {
                heading: format!("Note taken on {stamp}"),
                note: Some(text),
            },
        );
    }

    if !matches!(intent.state, Change::Keep)
        || !matches!(intent.title, Change::Keep)
        || !matches!(intent.priority, Change::Keep)
        || !intent.tags_add.is_empty()
        || !intent.tags_remove.is_empty()
    {
        let title = change_value(&Some(headline.item.title.clone()), &intent.title)
            .ok_or_else(|| AgdError::new("invalid_edit", "title cannot be cleared"))?;
        let priority = change_value(&headline.item.priority, &intent.priority);
        let mut tags: Vec<String> = headline
            .item
            .tags
            .iter()
            .filter(|tag| !intent.tags_remove.contains(*tag))
            .cloned()
            .collect();
        for tag in &intent.tags_add {
            if !tags.contains(tag) {
                tags.push(tag.clone());
            }
        }
        let line = align_tags(
            render_headline(
                headline.level,
                state.as_deref(),
                priority.as_deref(),
                &title,
                &tags,
            ),
            !tags.is_empty(),
            settings.tags_column,
        );
        patches.push(Patch {
            span: headline.line_span,
            replacement: line,
        });
    }

    if !matches!(intent.scheduled, Change::Keep)
        || !matches!(intent.deadline, Change::Keep)
        || closed != headline.item.planning.closed
        || repeating
    {
        let replacement =
            render_planning_line(scheduled.as_ref(), deadline.as_ref(), closed.as_ref())?;
        let indent = headline
            .planning_span
            .map_or("", |span| leading_indent(source, span.start));
        patches.push(Patch {
            span: headline.planning_span.unwrap_or(ByteSpan {
                start: after_headline_line(source, headline.line_span.end),
                end: after_headline_line(source, headline.line_span.end),
            }),
            replacement: replacement
                .map(|line| format!("{indent}{line}{}", line_ending(source)))
                .unwrap_or_default(),
        });
    }

    if !properties.is_empty() {
        let replacement = render_properties(source, headline, &properties, line_ending(source));
        let insert = headline.drawer_span.unwrap_or(ByteSpan {
            start: headline.planning_span.map_or(
                after_headline_line(source, headline.line_span.end),
                |span| span.end,
            ),
            end: headline.planning_span.map_or(
                after_headline_line(source, headline.line_span.end),
                |span| span.end,
            ),
        });
        patches.push(Patch {
            span: insert,
            replacement,
        });
    }

    if !log.is_empty() {
        patches.push(log_patch(source, headline, &log, logging.into_drawer)?);
    }
    Ok(patches)
}

/// One `- heading` line in a LOGBOOK, with an optional note below it.
struct LogEntry {
    heading: String,
    note: Option<String>,
}

impl LogEntry {
    /// A `@` (note) entry takes the edit's note text, if any; a `!` (time)
    /// entry leaves it for a separate "Note taken" entry.
    fn new(heading: String, kind: LogKind, note: &mut Option<String>) -> Self {
        let note = if kind == LogKind::Note {
            note.take()
        } else {
            None
        };
        Self { heading, note }
    }
}

/// Inserts log entries where org-log-beginning would: at the top of the
/// entry's LOGBOOK drawer (created after planning and properties when
/// missing) with org-log-into-drawer, otherwise right after them.
fn log_patch(
    source: &str,
    headline: &HeadlineSpan,
    entries: &[LogEntry],
    into_drawer: bool,
) -> Result<Patch, AgdError> {
    let newline = line_ending(source);
    let start = headline.drawer_span.or(headline.planning_span).map_or_else(
        || after_headline_line(source, headline.line_span.end),
        |span| span.end,
    );
    let indent = headline
        .drawer_span
        .or(headline.planning_span)
        .map_or("", |span| leading_indent(source, span.start));
    let mut lines = String::new();
    for entry in entries {
        let Some(note) = &entry.note else {
            lines.push_str(&format!("{indent}- {}{newline}", entry.heading));
            continue;
        };
        if note.contains(['\n', '\r']) {
            return Err(AgdError::new("invalid_note", "log notes must be one line"));
        }
        lines.push_str(&format!(
            "{indent}- {} \\\\{newline}{indent}  {note}{newline}",
            entry.heading
        ));
    }
    if into_drawer {
        let section = &source[start..headline.section_span.end];
        if let Some(offset) = logbook_line_end(section) {
            let at = start + offset;
            return Ok(Patch {
                span: ByteSpan { start: at, end: at },
                replacement: lines,
            });
        }
        lines = format!("{indent}:LOGBOOK:{newline}{lines}{indent}:END:{newline}");
    }
    Ok(Patch {
        span: ByteSpan { start, end: start },
        replacement: lines,
    })
}

/// End of the `:LOGBOOK:` line within a section, if the entry has one.
fn logbook_line_end(section: &str) -> Option<usize> {
    let mut offset = 0;
    for line in section.split_inclusive('\n') {
        offset += line.len();
        if line.trim().eq_ignore_ascii_case(":LOGBOOK:") {
            return Some(offset);
        }
    }
    None
}

/// Org quotes the old timestamp of a reschedule as an inactive one.
fn inactive_timestamp(timestamp: &PlanningTimestamp) -> Result<String, AgdError> {
    let active = format_active_timestamp(timestamp)?;
    Ok(format!("[{}]", &active[1..active.len() - 1]))
}

/// Setting a new SCHEDULED or DEADLINE date keeps the old repeater and
/// warning cookie unless the new value has its own, as org-schedule does.
fn planning_change(
    current: &Option<PlanningTimestamp>,
    change: &Change<PlanningTimestamp>,
) -> Option<PlanningTimestamp> {
    match (change, current) {
        (Change::Set(new), Some(old)) => {
            let mut new = new.clone();
            new.repeater = new.repeater.or_else(|| old.repeater.clone());
            new.warning = new.warning.or_else(|| old.warning.clone());
            Some(new)
        }
        _ => change_value(current, change),
    }
}

/// The first keyword of the sequence `state` belongs to; a repeating task
/// returns there when marked done (org-todo-kwd-alist head).
fn sequence_head(document: &IndexedDocument, state: Option<&str>) -> Option<String> {
    let sequence = state
        .and_then(|value| document.todo_keyword(value))
        .map_or(0, |keyword| keyword.sequence);
    document
        .todo_keywords()
        .iter()
        .find(|keyword| keyword.sequence == sequence)
        .or_else(|| document.todo_keywords().first())
        .map(|keyword| keyword.name.clone())
}

/// The edit's fields that need no file: a title, tags and properties
/// that write back as exactly one heading with those fields.
fn validate_intent(intent: &EditIntent) -> Result<(), AgdError> {
    if intent == &EditIntent::default() {
        return Err(AgdError::new("invalid_edit", "nothing to edit"));
    }
    if let Change::Set(title) = &intent.title {
        validate_title_text(title)?;
    }
    for tag in intent.tags_add.iter().chain(&intent.tags_remove) {
        validate_tag(tag)?;
    }
    for (key, value) in &intent.properties {
        validate_property(key, value.as_deref())?;
    }
    Ok(())
}

/// An `ID` is identity: an existing one may not change or go, and a new
/// one must not be in use anywhere agd indexes.
fn validate_id_change(
    intent: &EditIntent,
    headline: &HeadlineSpan,
    index: &WorkspaceIndex,
) -> Result<(), AgdError> {
    let Some((_, requested)) = intent
        .properties
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("ID"))
    else {
        return Ok(());
    };
    match (&headline.item.id, requested) {
        (Some(current), Some(requested)) if current == requested => Ok(()),
        (Some(current), _) => Err(AgdError::new(
            ErrorCode::IdImmutable,
            format!(
                "IDs are identity: links and refs point at ID '{current}', so it cannot be changed or removed"
            ),
        )),
        (None, Some(requested)) => refuse_used_id(requested, index),
        (None, None) => Ok(()),
    }
}

/// Refuses an ID some heading under the roots already has.
fn refuse_used_id(id: &str, index: &WorkspaceIndex) -> Result<(), AgdError> {
    if index.id_in_use(id) {
        return Err(AgdError::new(
            ErrorCode::DuplicateId,
            format!("ID '{id}' is already used under the roots; IDs must be unique"),
        ));
    }
    Ok(())
}

/// The edit's fields checked against the item's file: a title must not
/// start with one of its TODO keywords, a priority must be in its range.
fn validate_intent_for_file(
    intent: &EditIntent,
    document: &IndexedDocument,
) -> Result<(), AgdError> {
    if let Change::Set(title) = &intent.title {
        validate_title_keyword(title, document)?;
    }
    if let Change::Set(priority) = &intent.priority {
        validate_priority(priority, document)?;
    }
    Ok(())
}

/// A capture's fields that need no file, checked before anything is read.
fn validate_capture_input(
    draft: &CaptureDraft,
    body: &str,
    prompts: &BTreeMap<String, String>,
) -> Result<(), AgdError> {
    validate_title_text(&draft.title)?;
    for tag in &draft.tags {
        validate_tag(tag)?;
    }
    for (key, value) in &draft.properties {
        validate_property(key, Some(value))?;
    }
    for (name, answer) in prompts {
        if answer.contains(['\n', '\r']) {
            return Err(AgdError::new(
                ErrorCode::InvalidPrompt,
                format!("the answer to prompt '{name}' must be one line"),
            ));
        }
    }
    validate_body(body)
}

/// A capture's fields checked against the target file (or, for a new
/// file, the configuration).
fn validate_capture_for_file(
    draft: &CaptureDraft,
    document: &IndexedDocument,
) -> Result<(), AgdError> {
    validate_title_keyword(&draft.title, document)?;
    if let Some(priority) = &draft.priority {
        validate_priority(priority, document)?;
    }
    Ok(())
}

/// One non-empty line that the parser reads back as the whole title: no
/// leading priority cookie and no trailing tag group.
fn validate_title_text(title: &str) -> Result<(), AgdError> {
    if title.trim().is_empty() || title.contains(['\n', '\r']) {
        return Err(AgdError::new(
            ErrorCode::InvalidTitle,
            "title must be one non-empty line",
        ));
    }
    if let Some(cookie) = crate::document::leading_priority_cookie(title.trim_start()) {
        return Err(AgdError::new(
            ErrorCode::InvalidTitle,
            format!("title starts with the priority cookie {cookie}; set the priority instead"),
        ));
    }
    if let Some((_, tags)) = crate::document::split_trailing_tags(title) {
        return Err(AgdError::new(
            ErrorCode::InvalidTitle,
            format!("title ends with the tag group {tags}; give tags separately"),
        ));
    }
    Ok(())
}

/// A title starting with one of the file's TODO keywords would be read
/// back as that state.
fn validate_title_keyword(title: &str, document: &IndexedDocument) -> Result<(), AgdError> {
    let first = title.split_whitespace().next().unwrap_or_default();
    if document.todo_keyword(first).is_some() {
        return Err(AgdError::new(
            ErrorCode::InvalidTitle,
            format!("title starts with the TODO keyword {first}; set the state instead"),
        ));
    }
    Ok(())
}

/// Org's tag characters: letters, digits, `_`, `@`, `#` and `%`.
fn validate_tag(tag: &str) -> Result<(), AgdError> {
    if tag.is_empty()
        || !tag
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '@' | '#' | '%'))
    {
        return Err(AgdError::new(
            ErrorCode::InvalidTag,
            format!("invalid tag '{tag}': use letters, digits, _, @, # or %"),
        ));
    }
    Ok(())
}

/// One cookie character within the file's `#+PRIORITIES` range (else the
/// configured one).
fn validate_priority(priority: &str, document: &IndexedDocument) -> Result<(), AgdError> {
    let range = document.priorities();
    if !range.contains(priority) {
        return Err(AgdError::new(
            ErrorCode::InvalidPriority,
            format!(
                "priority '{priority}' is outside the range {} of {}",
                range.describe(),
                document.path().display()
            ),
        ));
    }
    Ok(())
}

/// A property key of `[A-Za-z0-9_-]` and a one-line value.
fn validate_property(key: &str, value: Option<&str>) -> Result<(), AgdError> {
    if key.is_empty()
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(AgdError::new(
            ErrorCode::InvalidProperty,
            format!("invalid property key '{key}': use letters, digits, _ or -"),
        ));
    }
    if value.is_some_and(|value| value.contains(['\n', '\r'])) {
        return Err(AgdError::new(
            ErrorCode::InvalidProperty,
            format!("property {key} must be one line"),
        ));
    }
    Ok(())
}

/// A body line that would start a heading (`*` then a space) would split
/// the entry in two.
fn validate_body(body: &str) -> Result<(), AgdError> {
    for (index, line) in body.lines().enumerate() {
        let stars = line.bytes().take_while(|byte| *byte == b'*').count();
        if stars > 0 && line[stars..].starts_with(char::is_whitespace) {
            return Err(AgdError::new(
                ErrorCode::InvalidBody,
                format!("body line {} would start a heading: {line}", index + 1),
            ));
        }
    }
    Ok(())
}

fn change_value<T: Clone>(current: &Option<T>, change: &Change<T>) -> Option<T> {
    match change {
        Change::Keep => current.clone(),
        Change::Set(value) => Some(value.clone()),
        Change::Clear => None,
    }
}

fn done_keywords(document: &IndexedDocument) -> BTreeSet<String> {
    document.todo_done().iter().cloned().collect()
}

fn render_headline(
    level: usize,
    state: Option<&str>,
    priority: Option<&str>,
    title: &str,
    tags: &[String],
) -> String {
    let mut fields = vec!["*".repeat(level)];
    if let Some(state) = state {
        fields.push(state.to_string());
    }
    if let Some(priority) = priority {
        fields.push(format!("[#{priority}]"));
    }
    fields.push(title.to_string());
    let mut line = fields.join(" ");
    if !tags.is_empty() {
        line.push_str(&format!(" :{}:", tags.join(":")));
    }
    line
}

/// Aligns tags to org-tags-column the way Emacs does whenever it rewrites a
/// headline: a negative column right-aligns them to end there, a positive
/// one starts them there, and a long title keeps a single space.
fn align_tags(rendered: String, has_tags: bool, column: i64) -> String {
    use unicode_width::UnicodeWidthStr;

    let Some(tags_start) = rendered.rfind(" :").filter(|_| has_tags) else {
        return rendered;
    };
    let (head, tags) = rendered.split_at(tags_start);
    let tags = tags.trim_start();
    let start = if column < 0 {
        column.unsigned_abs() as usize - tags.width().min(column.unsigned_abs() as usize)
    } else {
        column as usize
    };
    let gap = start.saturating_sub(head.width()).max(1);
    format!("{head}{}{tags}", " ".repeat(gap))
}

fn render_planning_line(
    scheduled: Option<&PlanningTimestamp>,
    deadline: Option<&PlanningTimestamp>,
    closed: Option<&PlanningTimestamp>,
) -> Result<Option<String>, AgdError> {
    let mut fields = Vec::new();
    if let Some(timestamp) = scheduled {
        fields.push(format!("SCHEDULED: {}", render_timestamp(timestamp, true)?));
    }
    if let Some(timestamp) = deadline {
        fields.push(format!("DEADLINE: {}", render_timestamp(timestamp, true)?));
    }
    if let Some(timestamp) = closed {
        fields.push(format!("CLOSED: {}", render_timestamp(timestamp, false)?));
    }
    Ok((!fields.is_empty()).then(|| fields.join(" ")))
}

fn render_timestamp(timestamp: &PlanningTimestamp, active: bool) -> Result<String, AgdError> {
    if active {
        return format_active_timestamp(timestamp);
    }
    let active = format_active_timestamp(timestamp)?;
    Ok(format!("[{}]", &active[1..active.len() - 1]))
}

fn render_properties(
    source: &str,
    headline: &HeadlineSpan,
    changes: &BTreeMap<String, Option<String>>,
    newline: &str,
) -> String {
    let mut properties: Vec<(String, String)> = headline
        .properties
        .iter()
        .map(|property| (property.key.clone(), property.value.clone()))
        .collect();
    for (key, value) in changes {
        let position = properties
            .iter()
            .position(|(existing, _)| existing.eq_ignore_ascii_case(key));
        match (position, value) {
            (Some(index), Some(value)) => properties[index].1 = value.clone(),
            (Some(index), None) => {
                properties.remove(index);
            }
            (None, Some(value)) => properties.push((key.clone(), value.clone())),
            (None, None) => {}
        }
    }
    if properties.is_empty() {
        return String::new();
    }
    let indent = headline
        .drawer_span
        .or(headline.planning_span)
        .map_or("", |span| leading_indent(source, span.start));
    let mut output = format!("{indent}:PROPERTIES:{newline}");
    for (key, value) in properties {
        output.push_str(&format!("{indent}:{key}: {value}{newline}"));
    }
    output.push_str(&format!("{indent}:END:{newline}"));
    output
}

fn advance_repeater(timestamp: &mut PlanningTimestamp, today: NaiveDate) -> Result<(), AgdError> {
    let repeater = timestamp
        .repeater
        .as_deref()
        .ok_or_else(|| AgdError::new("invalid_repeater", "timestamp has no repeater"))?;
    let (kind, amount_and_unit) = if let Some(value) = repeater.strip_prefix("++") {
        ("++", value)
    } else if let Some(value) = repeater.strip_prefix(".+") {
        (".+", value)
    } else if let Some(value) = repeater.strip_prefix('+') {
        ("+", value)
    } else {
        return Err(AgdError::new(
            "invalid_repeater",
            format!("invalid repeater: {repeater}"),
        ));
    };
    let split = amount_and_unit.len().saturating_sub(1);
    let amount = amount_and_unit[..split]
        .parse::<u64>()
        .map_err(|_| AgdError::new("invalid_repeater", format!("invalid repeater: {repeater}")))?;
    let unit = &amount_and_unit[split..];
    let original = NaiveDate::parse_from_str(&timestamp.date, "%Y-%m-%d")
        .map_err(|_| AgdError::new("invalid_date", format!("invalid date: {}", timestamp.date)))?;
    let mut next = if kind == ".+" {
        add_period(today, amount, unit)?
    } else {
        add_period(original, amount, unit)?
    };
    if kind == "++" {
        while next <= today {
            next = add_period(next, amount, unit)?;
        }
    }
    timestamp.date = next.format("%Y-%m-%d").to_string();
    timestamp.raw = None;
    Ok(())
}

fn add_period(date: NaiveDate, amount: u64, unit: &str) -> Result<NaiveDate, AgdError> {
    let result = match unit {
        "d" => date.checked_add_signed(Duration::days(amount as i64)),
        "w" => date.checked_add_signed(Duration::weeks(amount as i64)),
        "m" => date.checked_add_months(Months::new(amount as u32)),
        "y" => date.checked_add_months(Months::new((amount * 12) as u32)),
        _ => None,
    };
    result.ok_or_else(|| AgdError::new("invalid_repeater", "repeater date is out of range"))
}

/// Where a template files its entry, as org-capture's target types.
enum CaptureTarget {
    /// `file`: a top-level entry at the end of the file.
    File,
    /// `file+headline`: under the first heading with this title, created
    /// at the end of the file when missing.
    Heading(String),
    /// `file+olp`: under the heading at this outline path, which must exist
    /// and be unique at every level.
    OutlinePath(Vec<String>),
    /// `id`: under the heading with this `:ID:`.
    Id(String),
}

impl CaptureTarget {
    fn of(template: &CaptureTemplate) -> Self {
        if let Some(id) = &template.id {
            Self::Id(id.clone())
        } else if let Some(outline) = &template.outline_path {
            Self::OutlinePath(outline.clone())
        } else if let Some(heading) = &template.heading {
            Self::Heading(heading.clone())
        } else {
            Self::File
        }
    }
}

/// The target file's text with any headings the capture creates, and where
/// the entry goes in it.
struct CapturePlacement {
    text: String,
    offset: usize,
    level: usize,
}

/// org-capture's target location: the base heading (found, or created for
/// `file+headline`), then with `datetree` the year, month and day headings
/// for `today` under it; the entry goes at the end of the last one's
/// subtree, one level down, or at the end of the file.
fn place_capture(
    path: &Path,
    source: &str,
    template: &CaptureTemplate,
    today: NaiveDate,
    settings: &Settings,
) -> Result<CapturePlacement, AgdError> {
    let newline = line_ending(source);
    let mut text = source.to_string();
    let document = parse_text(path, &text, settings)?;
    let mut base = match CaptureTarget::of(template) {
        CaptureTarget::File => None,
        CaptureTarget::Heading(title) => Some(
            match document
                .headlines()
                .iter()
                .find(|headline| headline.item.title == title)
            {
                Some(headline) => headline.line_span.start,
                None => append_top_heading(&mut text, &title, newline),
            },
        ),
        CaptureTarget::OutlinePath(outline) => Some(find_outline_path(&document, &outline)?),
        CaptureTarget::Id(id) => Some(
            document
                .headlines()
                .iter()
                .find(|headline| headline.item.id.as_deref() == Some(id.as_str()))
                .map(|headline| headline.line_span.start)
                .ok_or_else(|| {
                    AgdError::new(
                        "capture_target_not_found",
                        format!("no heading in {} has ID '{id}'", path.display()),
                    )
                })?,
        ),
    };
    if template.datetree {
        base = Some(find_or_create_datetree(
            &mut text, base, today, newline, path, settings,
        )?);
    }
    let Some(base) = base else {
        let offset = text.len();
        return Ok(CapturePlacement {
            text,
            offset,
            level: 1,
        });
    };
    let document = parse_text(path, &text, settings)?;
    let headline = headline_starting_at(&document, base)?;
    Ok(CapturePlacement {
        offset: headline.subtree_span.end,
        level: headline.level + 1,
        text,
    })
}

/// Appends `* title` as org-capture does for a missing `file+headline`
/// target, and returns where the heading starts.
fn append_top_heading(text: &mut String, title: &str, newline: &str) -> usize {
    if !text.is_empty() && !text.ends_with('\n') {
        text.push_str(newline);
    }
    let start = text.len();
    text.push_str(&format!("* {title}{newline}"));
    start
}

/// org-find-olp: each title must name exactly one heading on its level,
/// the first at the top level and each next one a direct child.
fn find_outline_path(document: &IndexedDocument, outline: &[String]) -> Result<usize, AgdError> {
    let mut region = ByteSpan {
        start: 0,
        end: document.source().len(),
    };
    let mut level = 1;
    let mut found = None;
    for title in outline {
        let matches: Vec<&HeadlineSpan> = document
            .headlines()
            .iter()
            .filter(|headline| {
                headline.level == level
                    && headline.item.title == *title
                    && region.start <= headline.line_span.start
                    && headline.line_span.start < region.end
            })
            .collect();
        let headline = match matches.as_slice() {
            [headline] => *headline,
            [] => {
                return Err(AgdError::new(
                    "capture_target_not_found",
                    format!("capture heading not found on level {level}: {title}"),
                ));
            }
            _ => {
                return Err(AgdError::new(
                    "capture_target_not_found",
                    format!("capture heading not unique on level {level}: {title}"),
                ));
            }
        };
        region = headline.subtree_span;
        level = headline.level + 1;
        found = Some(headline.line_span.start);
    }
    found.ok_or_else(|| AgdError::new("capture_target_not_found", "capture outline path is empty"))
}

/// Datetree headings compare by the date in their title (Emacs's
/// org-datetree-comparefun-from-regex); titles without one are skipped.
static DATETREE_YEAR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([12][0-9]{3})").expect("datetree year regex"));
static DATETREE_MONTH_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([12][0-9]{3}-[01][0-9]) \w+").expect("datetree month regex"));
static DATETREE_DAY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"([12][0-9]{3}-[01][0-9]-[0123][0-9]) \w+").expect("datetree day regex")
});

/// org-datetree-find-date-create: finds or creates `* 2026`, `** 2026-10
/// October` and `*** 2026-10-16 Friday` under `base` (or at the top of the
/// file), each in date order among its siblings, and returns where the day
/// heading starts.
fn find_or_create_datetree(
    text: &mut String,
    base: Option<usize>,
    date: NaiveDate,
    newline: &str,
    path: &Path,
    settings: &Settings,
) -> Result<usize, AgdError> {
    let levels: [(String, &Regex); 3] = [
        (date.format("%Y").to_string(), &DATETREE_YEAR_RE),
        (date.format("%Y-%m %B").to_string(), &DATETREE_MONTH_RE),
        (date.format("%Y-%m-%d %A").to_string(), &DATETREE_DAY_RE),
    ];
    let mut parent = base;
    for (title, key) in levels {
        let heading = DatetreeHeading {
            title: &title,
            key,
            newline,
        };
        parent = Some(find_or_create_datetree_heading(
            text, parent, &heading, path, settings,
        )?);
    }
    Ok(parent.expect("the day heading"))
}

struct DatetreeHeading<'a> {
    title: &'a str,
    key: &'a Regex,
    newline: &'a str,
}

/// org-datetree--find-create-subheading within the parent's subtree as
/// org-narrow-to-subtree narrows it (or the whole file): the first
/// sibling whose date is not earlier is the heading itself or the one to
/// insert before; otherwise the heading goes at the end of the region.
/// Returns the heading's start.
fn find_or_create_datetree_heading(
    text: &mut String,
    parent: Option<usize>,
    heading: &DatetreeHeading<'_>,
    path: &Path,
    settings: &Settings,
) -> Result<usize, AgdError> {
    let document = parse_text(path, text, settings)?;
    let (region, level) = match parent {
        None => ((0, text.len()), 1),
        Some(start) => {
            let parent = headline_starting_at(&document, start)?;
            (
                narrowed_subtree(parent, text.len(), heading.newline),
                parent.level + 1,
            )
        }
    };
    let key = datetree_key(heading.key, heading.title).unwrap_or_default();
    let sibling = document
        .headlines()
        .iter()
        .filter(|headline| {
            headline.level == level
                && region.0 <= headline.line_span.start
                && headline.line_span.start < region.1
        })
        .find_map(|headline| {
            datetree_key(heading.key, &headline.item.title)
                .filter(|sibling| *sibling >= key)
                .map(|sibling| (headline.line_span.start, sibling == key))
        });
    if let Some((start, true)) = sibling {
        return Ok(start);
    }
    let point = sibling.map_or(region.1, |(start, _)| start);
    let deletion_start = region.0.max(
        text[..point]
            .trim_end_matches([' ', '\t', '\n', '\r'])
            .len(),
    );
    text.replace_range(deletion_start..point, "");
    let region_end = region.1 - (point - deletion_start);
    let blank = blank_before_heading(text, (region.0, region_end), deletion_start);
    let line = format!("{} {}{}", "*".repeat(level), heading.title, heading.newline);
    let insertion = format!(
        "{}{}{line}",
        if blank { heading.newline } else { "" },
        heading.newline
    );
    text.insert_str(deletion_start, &insertion);
    Ok(deletion_start + insertion.len() - line.len())
}

fn datetree_key(key: &Regex, title: &str) -> Option<String> {
    key.captures(title)
        .and_then(|captures| captures.get(1))
        .map(|matched| matched.as_str().to_string())
}

/// The region org-narrow-to-subtree leaves: the subtree without its final
/// newline unless it ends the file.
fn narrowed_subtree(headline: &HeadlineSpan, text_len: usize, newline: &str) -> (usize, usize) {
    let end = headline.subtree_span.end;
    let end = if end == text_len {
        end
    } else {
        end.saturating_sub(newline.len())
            .max(headline.line_span.start)
    };
    (headline.subtree_span.start, end)
}

/// Inserts the entry as org-capture-place-entry does with no
/// `:empty-lines`: blank lines before the insertion point are removed, one
/// is added when Org wants a blank line before a new heading here
/// (org--blank-before-heading-p) and the line before is not already blank,
/// and blank lines right after the entry are removed. Returns the new text
/// and where the entry starts.
fn insert_capture_entry(text: &str, offset: usize, entry: &str, newline: &str) -> (String, usize) {
    let mut before = text[..offset].to_string();
    let after = &text[offset..];
    if !before.is_empty() && !before.ends_with('\n') {
        before.push_str(newline);
    }
    let whole = format!("{before}{after}");
    let point = before.len();
    let blank = blank_before_heading(&whole, (0, whole.len()), point)
        && !previous_line_blank(&whole, 0, point);
    before.truncate(back_over_empty_lines(&before, point));
    if blank {
        before.push_str(newline);
    }
    let entry_start = before.len();
    let after = trim_leading_blank_lines(after);
    (format!("{before}{entry}{after}"), entry_start)
}

/// org-back-over-empty-lines: the start of the blank lines that end
/// `text[..pos]`.
fn back_over_empty_lines(text: &str, pos: usize) -> usize {
    let content_end = text[..pos].trim_end_matches([' ', '\t', '\n', '\r']).len();
    let next_line = text[content_end..]
        .find('\n')
        .map_or(text.len(), |index| content_end + index + 1);
    next_line.min(pos)
}

fn trim_leading_blank_lines(mut text: &str) -> &str {
    loop {
        let line_end = text.find('\n');
        match line_end {
            Some(index) if text[..index].trim_matches([' ', '\t', '\r']).is_empty() => {
                text = &text[index + 1..];
            }
            _ => return text,
        }
    }
}

/// org--blank-before-heading-p with org-blank-before-new-entry's `auto`,
/// within `region`: whether the heading at or before `point` (else the
/// next one) is preceded by a blank line. The region's first heading
/// defers to the heading after it.
fn blank_before_heading(text: &str, region: (usize, usize), point: usize) -> bool {
    let headings = heading_line_starts(text, region);
    let heading = headings
        .iter()
        .rev()
        .find(|start| **start <= point)
        .or_else(|| headings.iter().find(|start| **start > point));
    let Some(&heading) = heading else {
        return false;
    };
    if heading != region.0 {
        return previous_line_blank(text, region.0, heading);
    }
    headings
        .iter()
        .find(|start| **start > heading)
        .is_some_and(|next| previous_line_blank(text, region.0, *next))
}

/// Starts of the heading lines (`*` followed by a space or tab) that begin
/// inside `region`.
fn heading_line_starts(text: &str, region: (usize, usize)) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        if start < region.0 || start >= region.1 {
            continue;
        }
        let stars = line.bytes().take_while(|byte| *byte == b'*').count();
        if stars > 0 && matches!(line.as_bytes().get(stars), Some(b' ' | b'\t')) {
            starts.push(start);
        }
    }
    starts
}

/// org-previous-line-empty-p for the line starting at `line_start`.
fn previous_line_blank(text: &str, region_start: usize, line_start: usize) -> bool {
    if line_start <= region_start {
        return false;
    }
    let previous_end = line_start - 1;
    let previous_start = text[..previous_end]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    text[previous_start.max(region_start)..previous_end]
        .trim_matches([' ', '\t', '\r'])
        .is_empty()
}

/// A body given without a `%i` in the template becomes the entry's body,
/// after the template's text and any properties drawer, instead of being
/// dropped.
fn append_body_without_escape(
    rendered: String,
    template: &str,
    body: &str,
    newline: &str,
) -> String {
    let body = body.trim_end_matches(['\n', '\r']);
    if body.is_empty() || template_inserts_body(template) {
        return rendered;
    }
    let mut entry = rendered.trim_end_matches(['\n', '\r']).to_string();
    entry.push_str(newline);
    entry.push_str(body);
    entry
}

/// Whether the template has a `%i` escape (`%%i` is a literal `%i`).
fn template_inserts_body(template: &str) -> bool {
    let mut characters = template.chars();
    while let Some(character) = characters.next() {
        if character == '%' && characters.next() == Some('i') {
            return true;
        }
    }
    false
}

/// The entry's headline with its tags aligned to `tags_column`, as an edit
/// writes a headline.
fn align_first_line_tags(entry: &str, column: i64) -> String {
    let line_end = entry.find(['\r', '\n']).unwrap_or(entry.len());
    let (headline, rest) = entry.split_at(line_end);
    let Some((head, tags)) = crate::document::split_trailing_tags(headline) else {
        return entry.to_string();
    };
    format!(
        "{}{rest}",
        align_tags(format!("{head} {tags}"), true, column)
    )
}

/// The rendered template as an entry at `level`: every heading in it moved
/// by the same number of levels (org-paste-subtree), a template without a
/// leading heading made one (`* `), ending in a newline.
fn capture_entry(rendered: &str, level: usize, newline: &str) -> Result<String, AgdError> {
    let rendered = if heading_line_starts(rendered, (0, 1)).is_empty() {
        format!("* {rendered}")
    } else {
        rendered.to_string()
    };
    let first_level = rendered.bytes().take_while(|byte| *byte == b'*').count();
    let mut entry = relevel_subtree(&rendered, level, first_level)?;
    if !entry.ends_with('\n') {
        entry.push_str(newline);
    }
    Ok(entry)
}

fn parse_text(path: &Path, text: &str, settings: &Settings) -> Result<IndexedDocument, AgdError> {
    IndexedDocument::parse(path.to_path_buf(), text.as_bytes().to_vec(), settings)
}

fn headline_starting_at(
    document: &IndexedDocument,
    start: usize,
) -> Result<&HeadlineSpan, AgdError> {
    document
        .headlines()
        .iter()
        .find(|headline| headline.line_span.start == start)
        .ok_or_else(|| AgdError::new("parse_error", "capture target heading moved while placing"))
}

/// One patch turning `before` into `after`: the differing middle, widened
/// to whole lines so the journal reads like the diff.
fn changed_lines_patch(before: &str, after: &str) -> Patch {
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

fn capture_id_error(template_key: &str, id: &str, error: AgdError) -> AgdError {
    if error.code() == "duplicate_id" {
        return error;
    }
    AgdError::new(
        "capture_target_not_found",
        format!("capture template '{template_key}' targets ID '{id}', which no agenda heading has"),
    )
}

// With agent_inbox set, an agent capture always lands in that template
// without planning, so it reaches the day only through triage.
fn apply_agent_capture_gate(
    template: &CaptureTemplate,
    draft: &CaptureDraft,
    settings: &Settings,
    context: &WriteContext,
) -> (CaptureTemplate, CaptureDraft) {
    let inbox_key = match settings.agent_inbox.as_deref() {
        Some(key) if context.is_agent() => key,
        _ => return (template.clone(), draft.clone()),
    };
    let inbox = settings
        .capture_template(inbox_key)
        .cloned()
        .unwrap_or_else(|| template.clone());
    let mut draft = draft.clone();
    draft.scheduled = None;
    draft.deadline = None;
    (inbox, draft)
}

fn parse_updated_item(
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
        .ok_or_else(|| AgdError::new("parse_error", "write produced no target headline"))
}

/// Writes every image's post-image in order; when one cannot be written,
/// the ones before it get their pre-image back and the error is returned.
fn replace_all_or_restore(images: &[JournalImage<'_>]) -> Result<(), AgdError> {
    for (position, image) in images.iter().enumerate() {
        if let Err(error) = atomic_replace(image.path, image.post_image.as_bytes()) {
            for written in images[..position].iter().rev() {
                let _ = atomic_replace(written.path, written.pre_image.as_bytes());
            }
            return Err(error);
        }
    }
    Ok(())
}

/// The title and line of the heading a journal entry changed, as `current`
/// reads now: the heading with its ID, else the one around its first patch.
fn touched_heading(
    path: &Path,
    current: &str,
    record: &JournalRecord,
    settings: &Settings,
) -> Option<(String, usize)> {
    let document = parse_text(path, current, settings).ok()?;
    let by_id = record
        .item_ref
        .as_deref()
        .filter(|item_ref| !item_ref.contains("#sha256:"))
        .and_then(|id| {
            document
                .headlines()
                .iter()
                .find(|headline| headline.item.id.as_deref() == Some(id))
        });
    let at = record
        .patches
        .first()
        .map_or(0, |patch| patch.start)
        .min(current.len());
    let headline = by_id.or_else(|| {
        document
            .headlines()
            .iter()
            .rev()
            .find(|headline| headline.line_span.start <= at)
    })?;
    Some((headline.item.title.clone(), headline.item.line))
}

/// A batch's targets by file: (position of the edit, its heading).
type BatchTargets<'a> = BTreeMap<PathBuf, Vec<(usize, &'a HeadlineSpan)>>;

/// Resolves every edit's target and checks them against each other and
/// their files, grouped by file: (edit position, heading) pairs. A refusal
/// names the target.
fn resolve_batch_targets<'a>(
    index: &'a WorkspaceIndex,
    edits: &[ItemEdit],
) -> Result<BatchTargets<'a>, AgdError> {
    let mut by_file = BatchTargets::new();
    let mut new_ids: BTreeSet<&str> = BTreeSet::new();
    for (position, edit) in edits.iter().enumerate() {
        let (document, headline) = index
            .resolve(&edit.item_ref)
            .map_err(|error| error.with_context(&edit.item_ref))?;
        let label = |error: AgdError| error.with_context(&headline.item.title);
        validate_intent_for_file(&edit.intent, document).map_err(label)?;
        validate_id_change(&edit.intent, headline, index).map_err(label)?;
        let new_id = edit
            .intent
            .properties
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("ID"))
            .and_then(|(_, value)| value.as_deref());
        if let Some(id) = new_id
            && headline.item.id.is_none()
            && !new_ids.insert(id)
        {
            return Err(label(AgdError::new(
                ErrorCode::DuplicateId,
                format!("ID '{id}' is given to two of the targets"),
            )));
        }
        let targets = by_file.entry(document.path().to_path_buf()).or_default();
        if targets
            .iter()
            .any(|(_, other)| other.line_span.start == headline.line_span.start)
        {
            return Err(label(AgdError::new(
                "invalid_edit",
                "the same item is listed twice",
            )));
        }
        targets.push((position, headline));
    }
    Ok(by_file)
}

/// Where `offset` in the pre-image lands after `patches`: shifted by every
/// patch that ends at or before it.
fn post_image_offset(patches: &[Patch], offset: usize) -> usize {
    patches
        .iter()
        .filter(|patch| patch.span.end <= offset)
        .fold(offset, |shifted, patch| {
            shifted + patch.replacement.len() - (patch.span.end - patch.span.start)
        })
}

/// A successful mutation result.
fn mutation(item: Item, diff: String, journal_id: &str) -> Mutation {
    Mutation {
        schema: MUTATION_SCHEMA.to_string(),
        ok: true,
        item,
        diff,
        journal_id: journal_id.to_string(),
        repeated: None,
    }
}

/// An org file's text, or empty when it does not exist yet.
fn read_org_file(path: &Path) -> Result<String, AgdError> {
    match fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|error| {
            AgdError::with_source(
                "non_utf8",
                format!("org file is not UTF-8: {}", path.display()),
                error,
            )
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(AgdError::from(error)),
    }
}

fn refuse_emacs_lock(path: &Path) -> Result<(), AgdError> {
    let parent = path
        .parent()
        .ok_or_else(|| AgdError::new("invalid_path", "target has no parent"))?;
    let name = path
        .file_name()
        .ok_or_else(|| AgdError::new("invalid_path", "target has no file name"))?;
    let lock = parent.join(format!(".#{}", name.to_string_lossy()));
    if fs::symlink_metadata(&lock).is_ok() {
        let owner = fs::read_link(&lock)
            .map(|target| target.to_string_lossy().into_owned())
            .or_else(|_| fs::read_to_string(&lock))
            .unwrap_or_else(|_| "ownership unavailable".to_string());
        return Err(AgdError::new(
            "editor_dirty",
            format!(
                "Emacs lock {} exists ({owner}); save or close the buffer",
                lock.display()
            ),
        ));
    }
    Ok(())
}

/// Replaces `path` through a same-directory temporary file and a rename.
/// A replaced file keeps its mode; a new one gets the default mode (0666
/// less the umask), as an editor would create it.
fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<(), AgdError> {
    let parent = path
        .parent()
        .ok_or_else(|| AgdError::new("invalid_path", "target has no parent"))?;
    let permissions = fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());
    let temporary = parent.join(format!(".agd-{}.tmp", uuid::Uuid::new_v4()));
    let written = write_synced_new_file(&temporary, bytes, permissions)
        .and_then(|()| fs::rename(&temporary, path).map_err(AgdError::from));
    if let Err(error) = written {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    sync_parent_directory(parent)
}

/// Creates `path` (which must not exist) with the default mode, or with
/// `permissions` when given, and syncs its bytes to disk.
fn write_synced_new_file(
    path: &Path,
    bytes: &[u8],
    permissions: Option<fs::Permissions>,
) -> Result<(), AgdError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(AgdError::from)?;
    file.write_all(bytes).map_err(AgdError::from)?;
    if let Some(permissions) = permissions {
        file.set_permissions(permissions).map_err(AgdError::from)?;
    }
    file.sync_all().map_err(AgdError::from)
}

#[cfg(not(target_os = "windows"))]
fn sync_parent_directory(parent: &Path) -> Result<(), AgdError> {
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(AgdError::from)
}

#[cfg(target_os = "windows")]
fn sync_parent_directory(_: &Path) -> Result<(), AgdError> {
    // std::fs cannot open directory handles on Windows without platform-specific flags.
    Ok(())
}

fn after_headline_line(source: &str, content_end: usize) -> usize {
    if source.as_bytes().get(content_end) == Some(&b'\r')
        && source.as_bytes().get(content_end + 1) == Some(&b'\n')
    {
        content_end + 2
    } else if source.as_bytes().get(content_end) == Some(&b'\n') {
        content_end + 1
    } else {
        content_end
    }
}

fn line_ending(source: &str) -> &'static str {
    if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

fn leading_indent(source: &str, offset: usize) -> &str {
    let end = source[offset..]
        .find(['\r', '\n'])
        .map_or(source.len(), |relative| offset + relative);
    let line = &source[offset..end];
    let bytes = line
        .bytes()
        .take_while(|byte| matches!(byte, b' ' | b'\t'))
        .count();
    &line[..bytes]
}

fn content_hash(source: &str) -> String {
    sha256_hex(source.as_bytes())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Creates `directory` and its parents, readable only by the user.
fn create_private_directory(directory: &Path) -> Result<(), AgdError> {
    fs::create_dir_all(directory).map_err(AgdError::from)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
            .map_err(AgdError::from)?;
    }
    Ok(())
}

fn unified_diff(path: &Path, before: &str, after: &str) -> String {
    TextDiff::from_lines(before, after)
        .unified_diff()
        .header(&path.display().to_string(), &path.display().to_string())
        .to_string()
}

fn journal_id() -> String {
    format!(
        "{}-{}",
        Local::now().format("%Y%m%dT%H%M%S%.9f"),
        std::process::id()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(root: &Path) -> Settings {
        Settings {
            roots: vec![root.to_string_lossy().into_owned()],
            agenda_files: vec!["**/*.org".to_string()],
            ..Settings::default()
        }
    }

    fn context() -> WriteContext {
        WriteContext {
            actor: "test".to_string(),
            project: "fixture".to_string(),
            now: None,
        }
    }

    #[test]
    fn capture_journal_records_the_new_item_ref() {
        let temp = tempfile::tempdir().unwrap();
        let mut settings = settings(temp.path());
        settings.captures[0].file =
            Some(temp.path().join("inbox.org").to_string_lossy().into_owned());
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl")).unwrap();
        let draft = CaptureDraft {
            title: "New".to_string(),
            ..CaptureDraft::default()
        };
        let mutation = engine
            .capture("t", &draft, "", &BTreeMap::new(), false, &context())
            .unwrap();
        let record = engine
            .journal()
            .unwrap()
            .into_iter()
            .find(|record| record.id == mutation.journal_id)
            .unwrap();
        assert_eq!(
            record.item_ref.as_deref(),
            Some(mutation.item.item_ref.as_str())
        );
        assert_eq!(
            Path::new(&record.file),
            fs::canonicalize(temp.path().join("inbox.org")).unwrap()
        );
    }

    /// Every path under `directory`, relative and sorted.
    fn tree(directory: &Path) -> Vec<String> {
        let mut paths: Vec<String> = ignore::WalkBuilder::new(directory)
            .hidden(false)
            .build()
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let relative = entry.path().strip_prefix(directory).ok()?;
                (!relative.as_os_str().is_empty()).then(|| relative.display().to_string())
            })
            .collect();
        paths.sort();
        paths
    }

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

    #[test]
    fn write_locks_live_in_the_data_directory_not_beside_org_files() {
        let org = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let mut settings = settings(org.path());
        settings.captures[0].file = Some("inbox.org".to_string());
        let engine = WriteEngine::new(settings, data.path().join("journal.jsonl")).unwrap();
        let draft = CaptureDraft {
            title: "Written".to_string(),
            ..CaptureDraft::default()
        };
        engine
            .capture("t", &draft, "", &BTreeMap::new(), false, &context())
            .unwrap();
        assert_eq!(tree(org.path()), ["inbox.org"]);
        let data_paths = tree(data.path());
        assert_eq!(data_paths.len(), 3, "{data_paths:?}");
        assert_eq!(data_paths[0], "journal.jsonl");
        assert_eq!(data_paths[1], "locks");
        assert!(data_paths[2].starts_with("locks/") && data_paths[2].ends_with(".lock"));
    }

    #[test]
    fn a_refused_capture_creates_no_directory() {
        let org = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let mut settings = settings(org.path());
        settings.captures[0].file = Some("new/inbox.org".to_string());
        settings.captures[0].template = "* TODO %?\n%^{Missing}".to_string();
        let engine = WriteEngine::new(settings, data.path().join("journal.jsonl")).unwrap();
        let draft = CaptureDraft {
            title: "Refused".to_string(),
            ..CaptureDraft::default()
        };
        let error = engine
            .capture("t", &draft, "", &BTreeMap::new(), false, &context())
            .unwrap_err();
        assert_eq!(error.code(), "missing_prompt");
        assert!(tree(org.path()).is_empty());
        assert!(tree(data.path()).is_empty());
    }

    /// Appends a pending journal record for `file`, as a write interrupted
    /// between journaling and committing leaves it.
    fn append_interrupted_write(journal: &Path, file: &Path, pre: &str, post: &str) -> String {
        let record = JournalRecord {
            schema: "agd.journal/v1".to_string(),
            id: "20261016T090000.000000000-1".to_string(),
            status: JournalStatus::Pending,
            timestamp: "2026-10-16T09:00:00+00:00".to_string(),
            actor: "test".to_string(),
            project: "fixture".to_string(),
            file: fs::canonicalize(file)
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            item_ref: None,
            transaction: None,
            pre_hash: content_hash(pre),
            post_hash: content_hash(post),
            patches: Vec::new(),
            diff: String::new(),
        };
        let mut line = serde_json::to_string(&record).unwrap();
        line.push('\n');
        let mut existing = fs::read_to_string(journal).unwrap_or_default();
        existing.push_str(&line);
        fs::write(journal, existing).unwrap();
        record.id
    }

    #[test]
    fn an_unresolvable_interrupted_write_blocks_only_its_file_until_resolved() {
        let temp = tempfile::tempdir().unwrap();
        let journal = temp.path().join("journal.jsonl");
        let conflicted = temp.path().join("conflicted.org");
        let other = temp.path().join("other.org");
        let conflicted_bytes = "* TODO Edited by hand since\n";
        fs::write(&conflicted, conflicted_bytes).unwrap();
        fs::write(&other, "* TODO Other\n").unwrap();
        let id =
            append_interrupted_write(&journal, &conflicted, "* TODO Before\n", "* DONE Before\n");

        let settings = settings(temp.path());
        let engine = WriteEngine::new(settings.clone(), journal.clone()).unwrap();
        assert_eq!(engine.journal().unwrap().len(), 1);
        let conflicts = engine.journal_conflicts().unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].id, id);

        let refs: BTreeMap<String, String> = WorkspaceIndex::load(settings)
            .unwrap()
            .items()
            .map(|item| (item.title.clone(), item.item_ref.clone()))
            .collect();
        let done = EditIntent {
            state: Change::Set("DONE".to_string()),
            ..EditIntent::default()
        };
        engine
            .edit(&refs["Other"], &done, false, &context())
            .unwrap();
        let error = engine
            .edit(&refs["Edited by hand since"], &done, false, &context())
            .unwrap_err();
        assert_eq!(error.code(), "journal_conflict");
        assert!(
            error.to_string().contains("agd setup check --resolve"),
            "{error}"
        );
        assert_eq!(fs::read_to_string(&conflicted).unwrap(), conflicted_bytes);

        let resolved = engine.resolve_conflicts(&context()).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].status, JournalStatus::Resolved);
        assert_eq!(fs::read_to_string(&conflicted).unwrap(), conflicted_bytes);
        assert!(engine.journal_conflicts().unwrap().is_empty());
        engine
            .edit(&refs["Edited by hand since"], &done, false, &context())
            .unwrap();
    }

    #[test]
    fn an_interrupted_write_that_reached_the_file_settles_as_committed() {
        let temp = tempfile::tempdir().unwrap();
        let journal = temp.path().join("journal.jsonl");
        let path = temp.path().join("tasks.org");
        fs::write(&path, "* DONE Landed\n").unwrap();
        let id = append_interrupted_write(&journal, &path, "* TODO Landed\n", "* DONE Landed\n");
        let settings = settings(temp.path());
        let item_ref = WorkspaceIndex::load(settings.clone())
            .unwrap()
            .items()
            .next()
            .unwrap()
            .item_ref
            .clone();
        let engine = WriteEngine::new(settings, journal).unwrap();
        assert!(engine.journal_conflicts().unwrap().is_empty());
        let status = |engine: &WriteEngine| {
            engine
                .journal()
                .unwrap()
                .into_iter()
                .find(|record| record.id == id)
                .unwrap()
                .status
        };
        assert_eq!(status(&engine), JournalStatus::Pending);
        let todo = EditIntent {
            state: Change::Set("TODO".to_string()),
            ..EditIntent::default()
        };
        engine.edit(&item_ref, &todo, false, &context()).unwrap();
        assert_eq!(status(&engine), JournalStatus::Committed);
    }

    /// Captures `* TODO Entry` on 2026-10-16 into `input` (None: no file)
    /// through a template with this target, returning the file or the error
    /// code.
    fn capture_into(
        input: Option<&str>,
        heading: Option<&str>,
        outline: Option<&[&str]>,
        datetree: bool,
    ) -> Result<String, String> {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("input.org");
        if let Some(input) = input {
            fs::write(&path, input).unwrap();
        }
        let mut settings = settings(temp.path());
        settings.captures = vec![CaptureTemplate {
            key: "t".to_string(),
            file: Some("input.org".to_string()),
            heading: heading.map(str::to_string),
            outline_path: outline.map(|path| path.iter().map(|title| title.to_string()).collect()),
            datetree,
            template: "* TODO Entry".to_string(),
            ..CaptureTemplate::default()
        }];
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl")).unwrap();
        let context = WriteContext {
            now: Some(
                NaiveDate::from_ymd_opt(2026, 10, 16)
                    .unwrap()
                    .and_hms_opt(14, 22, 0)
                    .unwrap(),
            ),
            ..context()
        };
        let draft = CaptureDraft {
            title: "Entry".to_string(),
            ..CaptureDraft::default()
        };
        let preview = engine.capture("t", &draft, "", &BTreeMap::new(), true, &context);
        let written = engine.capture("t", &draft, "", &BTreeMap::new(), false, &context);
        match (preview, written) {
            (Ok(preview), Ok(written)) => {
                assert_eq!(preview.diff, written.diff);
                assert_eq!(written.item.title, "Entry");
                Ok(fs::read_to_string(&path).unwrap())
            }
            (Err(preview), Err(written)) => {
                assert_eq!(preview.code(), written.code());
                assert_eq!(fs::read_to_string(&path).ok().as_deref(), input);
                Err(written.code().to_string())
            }
            (preview, written) => panic!("dry run and write disagree: {preview:?} {written:?}"),
        }
    }

    // Expected bytes are Emacs 31.1 / Org 9.8.7 org-capture results for the
    // same file, target and day (template "* TODO Entry", :immediate-finish).
    #[test]
    fn headline_and_outline_targets_place_entries_as_org_capture() {
        let ok = |text: &str| Ok(text.to_string());
        let inbox = Some("Inbox");
        assert_eq!(
            capture_into(None, inbox, None, false),
            ok("* Inbox\n** TODO Entry\n")
        );
        assert_eq!(
            capture_into(Some(""), inbox, None, false),
            ok("* Inbox\n** TODO Entry\n")
        );
        assert_eq!(
            capture_into(Some("* Other\nbody"), inbox, None, false),
            ok("* Other\nbody\n* Inbox\n** TODO Entry\n")
        );
        assert_eq!(
            capture_into(Some("* A\n\n"), inbox, None, false),
            ok("* A\n\n* Inbox\n\n** TODO Entry\n")
        );
        assert_eq!(
            capture_into(Some("* Inbox\n** a\n\n* Next\n"), inbox, None, false),
            ok("* Inbox\n** a\n** TODO Entry\n* Next\n")
        );
        assert_eq!(
            capture_into(Some("* A\n\n\n"), None, None, false),
            ok("* A\n* TODO Entry\n")
        );
        let olp: &[&str] = &["Projects", "Inbox"];
        assert_eq!(
            capture_into(
                Some("* Projects\n** Inbox\n*** a\n\n* Other\n"),
                None,
                Some(olp),
                false
            ),
            ok("* Projects\n** Inbox\n*** a\n*** TODO Entry\n* Other\n")
        );
        assert_eq!(
            capture_into(Some("* Projects\n"), None, Some(olp), false),
            Err("capture_target_not_found".to_string())
        );
    }

    #[test]
    fn datetree_targets_create_year_month_and_day_as_org_capture() {
        let ok = |text: &str| Ok(text.to_string());
        let tree = "* 2026\n** 2026-10 October\n*** 2026-10-16 Friday\n**** TODO Entry\n";
        assert_eq!(
            capture_into(None, None, None, true),
            ok(&format!("\n{tree}"))
        );
        assert_eq!(
            capture_into(Some(""), None, None, true),
            ok(&format!("\n{tree}"))
        );
        assert_eq!(
            capture_into(Some("* Inbox\n** TODO a\n"), None, None, true),
            ok(&format!("* Inbox\n** TODO a\n{tree}"))
        );
        assert_eq!(
            capture_into(
                Some("#+TITLE: J\n\n* Notes\n\nbody\n\n\n"),
                None,
                None,
                true
            ),
            ok(&format!("#+TITLE: J\n\n* Notes\n\nbody\n\n{tree}"))
        );
        assert_eq!(
            capture_into(
                Some(
                    "* 2026\n** 2026-10 October\n*** 2026-10-14 Wednesday\n**** a\n*** 2026-10-20 Tuesday\n**** b\n"
                ),
                None,
                None,
                true
            ),
            ok(
                "* 2026\n** 2026-10 October\n*** 2026-10-14 Wednesday\n**** a\n*** 2026-10-16 Friday\n**** TODO Entry\n*** 2026-10-20 Tuesday\n**** b\n"
            )
        );
        assert_eq!(
            capture_into(
                Some("* 2025\n** 2025-12 December\n* 2027\n"),
                None,
                None,
                true
            ),
            ok(
                "* 2025\n** 2025-12 December\n* 2026\n** 2026-10 October\n*** 2026-10-16 Friday\n**** TODO Entry\n* 2027\n"
            )
        );
        assert_eq!(
            capture_into(
                Some(
                    "* 2026\n\n** 2026-09 September\n\n*** 2026-09-01 Tuesday\n\n** 2026-11 November\n\n"
                ),
                None,
                None,
                true
            ),
            ok(
                "* 2026\n\n** 2026-09 September\n\n*** 2026-09-01 Tuesday\n\n** 2026-10 October\n*** 2026-10-16 Friday\n**** TODO Entry\n** 2026-11 November\n\n"
            )
        );
        assert_eq!(
            capture_into(
                Some(
                    "* 2026\n** 2026-10 October\n*** 2026-10-16 Friday\n**** a\n\n*** 2026-10-17 Saturday\n"
                ),
                None,
                None,
                true
            ),
            ok(
                "* 2026\n** 2026-10 October\n*** 2026-10-16 Friday\n**** a\n**** TODO Entry\n*** 2026-10-17 Saturday\n"
            )
        );
        assert_eq!(
            capture_into(
                Some("* Inbox\n\n* 2025\n\n** 2025-01 January\n"),
                None,
                None,
                true
            ),
            ok(&format!(
                "* Inbox\n\n* 2025\n\n** 2025-01 January\n\n{tree}"
            ))
        );
        let journal: &[&str] = &["Journal"];
        assert_eq!(
            capture_into(
                Some("* Inbox\n* Journal\n** 2025\n* After\n"),
                None,
                Some(journal),
                true
            ),
            ok(
                "* Inbox\n* Journal\n** 2025\n** 2026\n*** 2026-10 October\n**** 2026-10-16 Friday\n***** TODO Entry\n* After\n"
            )
        );
        assert_eq!(
            capture_into(
                Some("* Journal\n** 2026\n*** 2026-09 September\n* After\n"),
                None,
                Some(journal),
                true
            ),
            ok(
                "* Journal\n** 2026\n*** 2026-09 September\n*** 2026-10 October\n**** 2026-10-16 Friday\n***** TODO Entry\n* After\n"
            )
        );
        assert_eq!(
            capture_into(Some("* Inbox\n"), None, Some(journal), true),
            Err("capture_target_not_found".to_string())
        );
    }

    #[test]
    fn an_id_target_captures_under_that_heading_in_any_agenda_file() {
        let temp = tempfile::tempdir().unwrap();
        let projects = temp.path().join("projects.org");
        fs::write(
            &projects,
            "* Launch\n:PROPERTIES:\n:ID: launch-id\n:END:\n** TODO Existing\n* Other\n",
        )
        .unwrap();
        let mut settings = settings(temp.path());
        settings.captures = vec![
            CaptureTemplate {
                key: "l".to_string(),
                file: None,
                heading: None,
                id: Some("launch-id".to_string()),
                ..CaptureTemplate::default()
            },
            CaptureTemplate {
                key: "m".to_string(),
                file: None,
                heading: None,
                id: Some("missing-id".to_string()),
                ..CaptureTemplate::default()
            },
        ];
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl")).unwrap();
        let draft = CaptureDraft {
            title: "Under launch".to_string(),
            ..CaptureDraft::default()
        };
        let mutation = engine
            .capture("l", &draft, "", &BTreeMap::new(), false, &context())
            .unwrap();
        assert_eq!(mutation.item.olpath, ["Launch", "Under launch"]);
        let written = fs::read_to_string(&projects).unwrap();
        assert!(
            written.starts_with("* Launch\n:PROPERTIES:\n:ID: launch-id\n:END:\n** TODO Existing\n** TODO Under launch\n"),
            "{written}"
        );
        assert!(written.ends_with("* Other\n"));
        let error = engine
            .capture("m", &draft, "", &BTreeMap::new(), false, &context())
            .unwrap_err();
        assert_eq!(error.code(), "capture_target_not_found");
    }

    #[cfg(unix)]
    #[test]
    fn new_files_get_the_default_mode_and_rewrites_keep_theirs() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let probe = temp.path().join("probe.txt");
        fs::File::create(&probe).unwrap();
        let default_mode = fs::metadata(&probe).unwrap().permissions().mode() & 0o777;
        let mut settings = settings(temp.path());
        settings.captures[0].file = Some("inbox.org".to_string());
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl")).unwrap();
        let draft = CaptureDraft {
            title: "First".to_string(),
            ..CaptureDraft::default()
        };
        engine
            .capture("t", &draft, "", &BTreeMap::new(), false, &context())
            .unwrap();
        let inbox = temp.path().join("inbox.org");
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&inbox), default_mode);
        fs::set_permissions(&inbox, fs::Permissions::from_mode(0o640)).unwrap();
        engine
            .capture("t", &draft, "", &BTreeMap::new(), false, &context())
            .unwrap();
        assert_eq!(mode(&inbox), 0o640);
        let leftovers: Vec<String> = tree(temp.path())
            .into_iter()
            .filter(|path| path.starts_with(".agd-"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn a_body_is_appended_when_the_template_has_no_escape_for_it() {
        let temp = tempfile::tempdir().unwrap();
        let mut settings = settings(temp.path());
        settings.captures = vec![
            CaptureTemplate {
                key: "t".to_string(),
                file: Some("inbox.org".to_string()),
                heading: None,
                template: "* TODO %?\n:PROPERTIES:\n:SOURCE: agent\n:END:".to_string(),
                ..CaptureTemplate::default()
            },
            CaptureTemplate {
                key: "i".to_string(),
                file: Some("inbox.org".to_string()),
                heading: None,
                template: "* TODO %?\nContext: %i\nLiteral 100%%i".to_string(),
                ..CaptureTemplate::default()
            },
        ];
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl")).unwrap();
        let draft = CaptureDraft {
            title: "Follow up".to_string(),
            ..CaptureDraft::default()
        };
        engine
            .capture(
                "t",
                &draft,
                "Line one\nLine two\n",
                &BTreeMap::new(),
                false,
                &context(),
            )
            .unwrap();
        engine
            .capture("i", &draft, "inline", &BTreeMap::new(), false, &context())
            .unwrap();
        assert_eq!(
            fs::read_to_string(temp.path().join("inbox.org")).unwrap(),
            "* TODO Follow up\n:PROPERTIES:\n:SOURCE: agent\n:END:\nLine one\nLine two\n* TODO Follow up\nContext: inline\nLiteral 100%i\n"
        );
    }

    #[test]
    fn captured_tags_align_to_the_tags_column_like_edits() {
        let temp = tempfile::tempdir().unwrap();
        let mut settings = settings(temp.path());
        settings.tags_column = -30;
        settings.captures[0].file = Some("inbox.org".to_string());
        settings.captures[0].heading = None;
        settings.captures[0].template = "* TODO %?".to_string();
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl")).unwrap();
        let draft = CaptureDraft {
            title: "Renew".to_string(),
            tags: vec!["admin".to_string()],
            ..CaptureDraft::default()
        };
        engine
            .capture("t", &draft, "", &BTreeMap::new(), false, &context())
            .unwrap();
        let written = fs::read_to_string(temp.path().join("inbox.org")).unwrap();
        assert_eq!(
            written,
            format!("* TODO Renew{}:admin:\n", " ".repeat(30 - 12 - 7))
        );
    }

    #[test]
    fn capture_refuses_fields_that_would_write_other_org_syntax() {
        let temp = tempfile::tempdir().unwrap();
        let inbox = temp.path().join("inbox.org");
        let original = "#+PRIORITIES: A E C\n* Inbox\n";
        fs::write(&inbox, original).unwrap();
        let mut settings = settings(temp.path());
        settings.captures[0].file = Some("inbox.org".to_string());
        settings.captures[0].template = "* TODO %?\n%^{Where}".to_string();
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl")).unwrap();
        let answer = BTreeMap::from([("Where".to_string(), "desk".to_string())]);
        let capture = |draft: CaptureDraft, body: &str, prompts: &BTreeMap<String, String>| {
            engine
                .capture("t", &draft, body, prompts, false, &context())
                .map(|_| ())
                .map_err(|error| (error.code().to_string(), error.to_string()))
        };
        let titled = |title: &str| CaptureDraft {
            title: title.to_string(),
            ..CaptureDraft::default()
        };
        let refusals = [
            (
                titled("Innocent\n* DONE Injected\n:PROPERTIES:\n:ID: abc-123\n:END:"),
                "",
                answer.clone(),
                "invalid_title",
            ),
            (titled("DONE sneaky"), "", answer.clone(), "invalid_title"),
            (
                titled("[#A] Cookie first"),
                "",
                answer.clone(),
                "invalid_title",
            ),
            (titled("Tagged :a:b:"), "", answer.clone(), "invalid_title"),
            (
                CaptureDraft {
                    tags: vec!["bad tag".to_string()],
                    ..titled("Task")
                },
                "",
                answer.clone(),
                "invalid_tag",
            ),
            (
                CaptureDraft {
                    priority: Some("F".to_string()),
                    ..titled("Task")
                },
                "",
                answer.clone(),
                "invalid_priority",
            ),
            (
                CaptureDraft {
                    properties: BTreeMap::from([("X\n#+TODO".to_string(), "y".to_string())]),
                    ..titled("Task")
                },
                "",
                answer.clone(),
                "invalid_property",
            ),
            (
                CaptureDraft {
                    properties: BTreeMap::from([("SOURCE".to_string(), "a\nb".to_string())]),
                    ..titled("Task")
                },
                "",
                answer.clone(),
                "invalid_property",
            ),
            (
                titled("Task"),
                "fine\n* DONE Injected",
                answer.clone(),
                "invalid_body",
            ),
            (
                titled("Task"),
                "",
                BTreeMap::from([("Where".to_string(), "desk\n* DONE x".to_string())]),
                "invalid_prompt",
            ),
        ];
        for (draft, body, prompts, code) in refusals {
            let (actual, message) = capture(draft.clone(), body, &prompts).unwrap_err();
            assert_eq!(actual, code, "{draft:?} {body:?}: {message}");
        }
        let (_, message) = capture(
            CaptureDraft {
                priority: Some("F".to_string()),
                ..titled("Task")
            },
            "",
            &answer,
        )
        .unwrap_err();
        assert!(message.contains("A–E"), "{message}");
        let (_, message) = capture(titled("Task"), "fine\n* DONE Injected", &answer).unwrap_err();
        assert!(message.contains("line 2"), "{message}");
        assert_eq!(fs::read_to_string(&inbox).unwrap(), original);
        assert!(engine.journal().unwrap().is_empty());

        // The file's own range admits E.
        capture(
            CaptureDraft {
                priority: Some("E".to_string()),
                ..titled("Low but fine")
            },
            "",
            &answer,
        )
        .unwrap();
    }

    #[test]
    fn edit_refuses_titles_tags_priorities_and_properties_that_would_change_the_syntax() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tasks.org");
        let original = "* TODO Task\n";
        fs::write(&path, original).unwrap();
        let settings = settings(temp.path());
        let item_ref = WorkspaceIndex::load(settings.clone())
            .unwrap()
            .items()
            .next()
            .unwrap()
            .item_ref
            .clone();
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl")).unwrap();
        let refusals = [
            (
                EditIntent {
                    title: Change::Set("DONE sneaky :x:".to_string()),
                    ..EditIntent::default()
                },
                "invalid_title",
            ),
            (
                EditIntent {
                    title: Change::Set("NEXT step".to_string()),
                    ..EditIntent::default()
                },
                "invalid_title",
            ),
            (
                EditIntent {
                    priority: Change::Set("D".to_string()),
                    ..EditIntent::default()
                },
                "invalid_priority",
            ),
            (
                EditIntent {
                    tags_add: BTreeSet::from(["a:b".to_string()]),
                    ..EditIntent::default()
                },
                "invalid_tag",
            ),
            (
                EditIntent {
                    properties: BTreeMap::from([("A B".to_string(), Some("x".to_string()))]),
                    ..EditIntent::default()
                },
                "invalid_property",
            ),
        ];
        for (intent, code) in refusals {
            for dry_run in [true, false] {
                let error = engine
                    .edit(&item_ref, &intent, dry_run, &context())
                    .unwrap_err();
                assert_eq!(error.code(), code, "{intent:?}");
            }
        }
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        assert!(engine.journal().unwrap().is_empty());
    }

    #[test]
    fn ids_must_be_unique_and_never_change_once_set() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tasks.org");
        let original = "* TODO Known\n:PROPERTIES:\n:ID: known-id\n:END:\n* TODO Fresh\n";
        fs::write(&path, original).unwrap();
        std::fs::create_dir(temp.path().join("archive")).unwrap();
        fs::write(
            temp.path().join("archive/old.org"),
            "* Old\n:PROPERTIES:\n:ID: archived-id\n:END:\n",
        )
        .unwrap();
        let mut settings = settings(temp.path());
        settings.agenda_files = vec!["tasks.org".to_string()];
        settings.captures[0].file = Some("tasks.org".to_string());
        settings.captures[0].heading = None;
        let fresh = WorkspaceIndex::load(settings.clone())
            .unwrap()
            .items()
            .find(|item| item.title == "Fresh")
            .unwrap()
            .item_ref
            .clone();
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl")).unwrap();
        let set_id = |value: Option<&str>| EditIntent {
            properties: BTreeMap::from([("ID".to_string(), value.map(str::to_string))]),
            ..EditIntent::default()
        };
        let code = |result: Result<Mutation, AgdError>| result.unwrap_err().code().to_string();

        assert_eq!(
            code(engine.edit("known-id", &set_id(Some("other")), false, &context())),
            "id_immutable"
        );
        assert_eq!(
            code(engine.edit("known-id", &set_id(None), false, &context())),
            "id_immutable"
        );
        let message = engine
            .edit("known-id", &set_id(None), false, &context())
            .unwrap_err()
            .to_string();
        assert!(message.contains("identity"), "{message}");
        for used in ["known-id", "archived-id"] {
            assert_eq!(
                code(engine.edit(&fresh, &set_id(Some(used)), false, &context())),
                "duplicate_id"
            );
            let draft = CaptureDraft {
                title: "Copy".to_string(),
                properties: BTreeMap::from([("ID".to_string(), used.to_string())]),
                ..CaptureDraft::default()
            };
            assert_eq!(
                code(engine.capture("t", &draft, "", &BTreeMap::new(), false, &context())),
                "duplicate_id"
            );
        }
        assert_eq!(fs::read_to_string(&path).unwrap(), original);

        let added = engine
            .edit(&fresh, &set_id(Some("fresh-id")), false, &context())
            .unwrap();
        assert_eq!(added.item.id.as_deref(), Some("fresh-id"));
        let draft = CaptureDraft {
            title: "New with ID".to_string(),
            properties: BTreeMap::from([("ID".to_string(), "new-id".to_string())]),
            ..CaptureDraft::default()
        };
        let captured = engine
            .capture("t", &draft, "", &BTreeMap::new(), false, &context())
            .unwrap();
        assert_eq!(captured.item.item_ref, "new-id");
    }

    fn refs_by_title(settings: &Settings) -> BTreeMap<String, String> {
        WorkspaceIndex::load(settings.clone())
            .unwrap()
            .items()
            .map(|item| (item.title.clone(), item.item_ref.clone()))
            .collect()
    }

    fn scheduled(date: &str) -> EditIntent {
        EditIntent {
            scheduled: Change::Set(PlanningTimestamp {
                date: date.to_string(),
                time: None,
                end_time: None,
                repeater: None,
                warning: None,
                raw: None,
            }),
            ..EditIntent::default()
        }
    }

    #[test]
    fn a_batch_writes_every_file_or_none_and_one_undo_reverts_it() {
        let temp = tempfile::tempdir().unwrap();
        let work = temp.path().join("work.org");
        let home = temp.path().join("home.org");
        let work_bytes = "* TODO Pay rent\n* TODO Call bank\nbody\n* TODO Untouched\n";
        let home_bytes = "#+TODO: OPEN | CLOSED\n* OPEN Water plants\n";
        fs::write(&work, work_bytes).unwrap();
        fs::write(&home, home_bytes).unwrap();
        let settings = settings(temp.path());
        let refs = refs_by_title(&settings);
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl")).unwrap();
        let edit = |title: &str, intent: EditIntent| ItemEdit {
            item_ref: refs[title].clone(),
            intent,
        };

        // DONE is not a keyword of home.org: nothing is written anywhere.
        let done = EditIntent {
            state: Change::Set("DONE".to_string()),
            ..EditIntent::default()
        };
        let error = engine
            .edit_batch(
                &[edit("Pay rent", done.clone()), edit("Water plants", done)],
                false,
                &context(),
            )
            .unwrap_err();
        assert_eq!(error.code(), "invalid_todo");
        assert!(error.to_string().starts_with("Water plants: "), "{error}");
        assert_eq!(fs::read_to_string(&work).unwrap(), work_bytes);
        assert_eq!(fs::read_to_string(&home).unwrap(), home_bytes);
        assert!(engine.journal().unwrap().is_empty());

        // An Emacs lock on one file refuses the whole batch too.
        fs::write(temp.path().join(".#home.org"), "user@host.1").unwrap();
        let batch = [
            edit("Pay rent", scheduled("2026-10-19")),
            edit("Call bank", scheduled("2026-10-20")),
            edit("Water plants", scheduled("2026-10-21")),
        ];
        assert_eq!(
            engine
                .edit_batch(&batch, false, &context())
                .unwrap_err()
                .code(),
            "editor_dirty"
        );
        assert_eq!(fs::read_to_string(&work).unwrap(), work_bytes);
        fs::remove_file(temp.path().join(".#home.org")).unwrap();

        let preview = engine.edit_batch(&batch, true, &context()).unwrap();
        assert_eq!(preview.transaction, "dry-run");
        assert_eq!(fs::read_to_string(&work).unwrap(), work_bytes);
        let written = engine.edit_batch(&batch, false, &context()).unwrap();
        assert_eq!(
            fs::read_to_string(&work).unwrap(),
            "* TODO Pay rent\nSCHEDULED: <2026-10-19 Mon>\n* TODO Call bank\nSCHEDULED: <2026-10-20 Tue>\nbody\n* TODO Untouched\n"
        );
        let lines: Vec<(String, usize)> = written
            .mutations
            .iter()
            .map(|mutation| (mutation.item.title.clone(), mutation.item.line))
            .collect();
        assert_eq!(
            lines,
            [
                ("Pay rent".to_string(), 1),
                ("Call bank".to_string(), 3),
                ("Water plants".to_string(), 2)
            ]
        );
        assert_eq!(engine.journal().unwrap().len(), 2);

        let undone = engine
            .undo(&written.mutations[0].journal_id, &context())
            .unwrap();
        assert_eq!(undone.journal_id, written.transaction);
        assert_eq!(fs::read_to_string(&work).unwrap(), work_bytes);
        assert_eq!(fs::read_to_string(&home).unwrap(), home_bytes);
    }

    #[test]
    fn undo_takes_an_entry_or_transaction_id_and_refuses_what_is_not_committed() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("a.org"), "* TODO One\n").unwrap();
        fs::write(temp.path().join("b.org"), "* TODO Two\n").unwrap();
        let settings = settings(temp.path());
        let refs = refs_by_title(&settings);
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl")).unwrap();
        let batch = engine
            .edit_batch(
                &[
                    ItemEdit {
                        item_ref: refs["One"].clone(),
                        intent: scheduled("2026-10-19"),
                    },
                    ItemEdit {
                        item_ref: refs["Two"].clone(),
                        intent: scheduled("2026-10-19"),
                    },
                ],
                false,
                &context(),
            )
            .unwrap();
        let undone = engine.undo(&batch.transaction, &context()).unwrap();
        assert_eq!(undone.file.split(',').count(), 2);
        assert_eq!(
            fs::read_to_string(temp.path().join("a.org")).unwrap(),
            "* TODO One\n"
        );
        let again = engine.undo(&batch.transaction, &context()).unwrap_err();
        assert_eq!(again.code(), "nothing_to_undo");
        assert_eq!(
            engine.undo("no-such-entry", &context()).unwrap_err().code(),
            "journal_not_found"
        );
    }

    #[test]
    fn an_undo_conflict_names_the_heading_its_author_and_what_to_do() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("work.org");
        fs::write(
            &path,
            "* TODO Rotate staging certs\n:PROPERTIES:\n:ID: certs\n:END:\n",
        )
        .unwrap();
        let engine =
            WriteEngine::new(settings(temp.path()), temp.path().join("journal.jsonl")).unwrap();
        let agent = WriteContext {
            actor: "mcp:codex".to_string(),
            ..context()
        };
        let mutation = engine
            .edit("certs", &scheduled("2026-10-19"), false, &agent)
            .unwrap();
        let edited = fs::read_to_string(&path)
            .unwrap()
            .replace("<2026-10-19 Mon>", "<2026-10-22 Thu>");
        fs::write(&path, &edited).unwrap();
        let error = engine.undo(&mutation.journal_id, &context()).unwrap_err();
        assert_eq!(error.code(), "undo_conflict");
        assert_eq!(
            error.to_string(),
            "can't undo: Rotate staging certs changed after codex edited it (work.org:1); edit it by hand"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), edited);
    }

    #[test]
    fn inbox_gate_redirects_only_agent_captures_and_drops_their_planning() {
        let temp = tempfile::tempdir().unwrap();
        let mut settings = settings(temp.path());
        settings.agent_inbox = Some("t".to_string());
        settings.captures.push(CaptureTemplate {
            key: "w".to_string(),
            file: Some("work.org".to_string()),
            heading: None,
            template: "* TODO %?".to_string(),
            ..CaptureTemplate::default()
        });
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl")).unwrap();
        let draft = CaptureDraft {
            title: "Planned".to_string(),
            scheduled: Some(PlanningTimestamp {
                date: "2026-10-15".to_string(),
                time: None,
                end_time: None,
                repeater: None,
                warning: None,
                raw: None,
            }),
            deadline: None,
            tags: Vec::new(),
            priority: None,
            properties: BTreeMap::new(),
        };
        let agent = WriteContext {
            actor: "mcp:claude-code".to_string(),
            project: "fixture".to_string(),
            now: None,
        };
        let agent_capture = engine
            .capture("w", &draft, "", &BTreeMap::new(), true, &agent)
            .unwrap();
        assert!(agent_capture.item.file.ends_with("inbox.org"));
        assert!(!agent_capture.diff.contains("SCHEDULED"));

        let human_capture = engine
            .capture("w", &draft, "", &BTreeMap::new(), true, &context())
            .unwrap();
        assert!(human_capture.item.file.ends_with("work.org"));
        assert!(human_capture.diff.contains("SCHEDULED: <2026-10-15 Thu>"));
    }

    #[test]
    fn add_on_write_inserts_an_id_only_when_missing() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tasks.org");
        fs::write(
            &path,
            "* TODO New\n* TODO Known\n:PROPERTIES:\n:ID: kept\n:END:\n",
        )
        .unwrap();
        let mut settings = settings(temp.path());
        settings.ids = IdPolicy::AddOnWrite;
        let refs: Vec<String> = WorkspaceIndex::load(settings.clone())
            .unwrap()
            .items()
            .map(|item| item.item_ref.clone())
            .collect();
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl")).unwrap();
        let done = EditIntent {
            state: Change::Set("DONE".to_string()),
            ..EditIntent::default()
        };
        let added = engine.edit(&refs[0], &done, false, &context()).unwrap();
        let id = added.item.id.clone().unwrap();
        assert_eq!(id.len(), 36);
        assert_eq!(added.item.item_ref, id);
        engine.edit("kept", &done, false, &context()).unwrap();
        let written = fs::read_to_string(&path).unwrap();
        assert_eq!(written.matches(":ID:").count(), 2);
        assert!(
            written.contains(&format!(":ID:       {id}"))
                || written.contains(&format!(":ID: {id}"))
        );
    }

    #[test]
    fn undo_survives_unrelated_edits_and_refuses_overlapping_ones() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tasks.org");
        fs::write(&path, "* TODO First\n* TODO Second\nbody\n").unwrap();
        let settings = settings(temp.path());
        let refs: Vec<String> = WorkspaceIndex::load(settings.clone())
            .unwrap()
            .items()
            .map(|item| item.item_ref.clone())
            .collect();
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl")).unwrap();
        let done = EditIntent {
            state: Change::Set("DONE".to_string()),
            ..EditIntent::default()
        };

        let second = engine.edit(&refs[1], &done, false, &context()).unwrap();
        let edited = fs::read_to_string(&path).unwrap();
        fs::write(
            &path,
            format!("#+TITLE: Added by an editor\n{edited}* TODO Third\n"),
        )
        .unwrap();
        engine.undo(&second.journal_id, &context()).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "#+TITLE: Added by an editor\n* TODO First\n* TODO Second\nbody\n* TODO Third\n"
        );

        let first = engine.edit(&refs[0], &done, false, &context()).unwrap();
        let edited = fs::read_to_string(&path).unwrap();
        fs::write(
            &path,
            edited.replace("* DONE First", "* DONE First, renamed"),
        )
        .unwrap();
        let error = engine.undo(&first.journal_id, &context()).unwrap_err();
        assert_eq!(error.code(), "undo_conflict");
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains("First, renamed")
        );
    }

    #[test]
    fn patch_and_inverse_preserve_every_untouched_byte() {
        let source = "α before\r\n* TODO Keep spacing   :x:\r\nbody\r\n";
        let start = source.find("TODO").unwrap();
        let (after, stored) = apply_patches(
            source,
            vec![Patch {
                span: ByteSpan {
                    start,
                    end: start + 4,
                },
                replacement: "DONE".to_string(),
            }],
        )
        .unwrap();
        assert_eq!(after, "α before\r\n* DONE Keep spacing   :x:\r\nbody\r\n");
        assert_eq!(
            apply_patches(&after, inverse_patches(&stored)).unwrap().0,
            source
        );
    }

    #[test]
    fn edit_revalidates_writes_journal_and_undo_keeps_later_unrelated_text() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tasks.org");
        let original = "#+TODO: TODO | DONE\r\n* TODO Exact  spacing :work:\r\nbody\r\n";
        fs::write(&path, original).unwrap();
        let settings = settings(temp.path());
        let item_ref = WorkspaceIndex::load(settings.clone())
            .unwrap()
            .items()
            .next()
            .unwrap()
            .item_ref
            .clone();
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl")).unwrap();
        let mutation = engine
            .edit(
                &item_ref,
                &EditIntent {
                    scheduled: Change::Set(PlanningTimestamp {
                        date: "2026-10-15".to_string(),
                        time: None,
                        end_time: None,
                        repeater: None,
                        warning: None,
                        raw: None,
                    }),
                    ..EditIntent::default()
                },
                false,
                &context(),
            )
            .unwrap();
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.starts_with("#+TODO: TODO | DONE\r\n* TODO Exact  spacing :work:\r\n"));
        assert!(written.contains("SCHEDULED: <2026-10-15 Thu>\r\nbody\r\n"));
        engine.undo(&mutation.journal_id, &context()).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), original);

        let mutation = engine
            .edit(
                &item_ref,
                &EditIntent {
                    scheduled: Change::Set(PlanningTimestamp {
                        date: "2026-10-16".to_string(),
                        time: None,
                        end_time: None,
                        repeater: None,
                        warning: None,
                        raw: None,
                    }),
                    ..EditIntent::default()
                },
                false,
                &context(),
            )
            .unwrap();
        fs::write(
            &path,
            format!("{}external\r\n", fs::read_to_string(&path).unwrap()),
        )
        .unwrap();
        engine.undo(&mutation.journal_id, &context()).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            format!("{original}external\r\n")
        );
    }

    #[test]
    fn refuses_editor_locks() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tasks.org");
        fs::write(&path, "* TODO Task\n").unwrap();
        let settings = settings(temp.path());
        let item_ref = WorkspaceIndex::load(settings.clone())
            .unwrap()
            .items()
            .next()
            .unwrap()
            .item_ref
            .clone();
        let engine = WriteEngine::new(settings.clone(), temp.path().join("journal.jsonl")).unwrap();
        fs::write(temp.path().join(".#tasks.org"), "user@host.123").unwrap();
        let error = engine
            .edit(
                &item_ref,
                &EditIntent {
                    state: Change::Set("DONE".to_string()),
                    ..EditIntent::default()
                },
                false,
                &context(),
            )
            .unwrap_err();
        assert_eq!(error.code(), "editor_dirty");
    }

    #[test]
    fn capture_preview_matches_committed_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let settings = settings(temp.path());
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl")).unwrap();
        let draft = CaptureDraft {
            title: "Captured".to_string(),
            scheduled: None,
            deadline: None,
            tags: Vec::new(),
            priority: None,
            properties: BTreeMap::new(),
        };
        let preview = engine
            .capture("t", &draft, "", &BTreeMap::new(), true, &context())
            .unwrap();
        let committed = engine
            .capture("t", &draft, "", &BTreeMap::new(), false, &context())
            .unwrap();
        assert_eq!(preview.diff, committed.diff);
        assert!(
            fs::read_to_string(temp.path().join("inbox.org"))
                .unwrap()
                .contains("* TODO Captured\n")
        );
    }

    #[test]
    fn completing_repeater_advances_date_and_records_last_repeat() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("repeat.org");
        fs::write(
            &path,
            "#+TODO: TODO | DONE\n* TODO Water plants\nSCHEDULED: <2020-01-01 Wed .+1d>\n",
        )
        .unwrap();
        let settings = settings(temp.path());
        let item_ref = WorkspaceIndex::load(settings.clone())
            .unwrap()
            .items()
            .next()
            .unwrap()
            .item_ref
            .clone();
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl")).unwrap();
        let repeated = engine
            .edit(
                &item_ref,
                &EditIntent {
                    state: Change::Set("DONE".to_string()),
                    ..EditIntent::default()
                },
                false,
                &context(),
            )
            .unwrap();
        let written = fs::read_to_string(path).unwrap();
        let tomorrow = (Local::now().date_naive() + Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        assert_eq!(
            repeated.repeated,
            Some(Repeat {
                done_state: "DONE".to_string(),
                next_date: tomorrow.clone(),
            })
        );
        assert!(written.contains("* TODO Water plants\n"));
        assert!(written.contains(&format!("SCHEDULED: <{tomorrow}")));
        assert!(written.contains(":LAST_REPEAT: ["));
    }

    #[test]
    fn indented_planning_edit_and_undo_are_byte_exact() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("agenda.org");
        let original = concat!(
            "* Work Tasks\n",
            "** TODO [#A] Complete quarterly report                                :work:\n",
            "   SCHEDULED: <2026-10-14 Wed> DEADLINE: <2026-10-19 Mon>\n",
            "   :PROPERTIES:\n",
            "   :ID: work-task-001\n",
            "   :END:\n",
            "   Need to finish Q3 report for the board meeting.\n\n",
            "** TODO [#B] Review pull requests :work:\n",
        );
        fs::write(&path, original).unwrap();
        let settings = settings(temp.path());
        let engine = WriteEngine::new(settings, temp.path().join("journal-real.jsonl")).unwrap();
        let mutation = engine
            .edit(
                "work-task-001",
                &EditIntent {
                    scheduled: Change::Set(PlanningTimestamp {
                        date: "2026-10-30".to_string(),
                        time: None,
                        end_time: None,
                        repeater: None,
                        warning: None,
                        raw: None,
                    }),
                    ..EditIntent::default()
                },
                false,
                &context(),
            )
            .unwrap();
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains("   SCHEDULED: <2026-10-30 Fri> DEADLINE:"));
        assert!(written.contains("   Need to finish Q3 report for the board meeting."));
        engine.undo(&mutation.journal_id, &context()).unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), original);
    }

    #[test]
    fn cross_file_refile_and_transaction_undo_are_exact() {
        let temp = tempfile::tempdir().unwrap();
        let source_path = temp.path().join("source.org");
        let target_path = temp.path().join("target.org");
        let source = "* Source\n** TODO Move me\n:PROPERTIES:\n:ID: move-id\n:END:\nbody\n* Keep\n";
        let target = "* Destination\n:PROPERTIES:\n:ID: destination-id\n:END:\n";
        fs::write(&source_path, source).unwrap();
        fs::write(&target_path, target).unwrap();
        let settings = settings(temp.path());
        let engine = WriteEngine::new(settings, temp.path().join("journal-refile.jsonl")).unwrap();
        let result = engine
            .refile("move-id", "destination-id", false, &context())
            .unwrap();
        assert!(
            !fs::read_to_string(&source_path)
                .unwrap()
                .contains("Move me")
        );
        assert!(
            fs::read_to_string(&target_path)
                .unwrap()
                .contains("** TODO Move me\n")
        );
        engine.undo(&result.journal_ids[0], &context()).unwrap();
        assert_eq!(fs::read_to_string(source_path).unwrap(), source);
        assert_eq!(fs::read_to_string(target_path).unwrap(), target);
    }
}
