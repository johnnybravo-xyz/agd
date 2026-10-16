use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use chrono::NaiveDateTime;

use crate::{
    AgdError, ErrorCode, HeadlineSpan, IndexedDocument, Item, Mutation, PlanningTimestamp, Repeat,
    WorkspaceIndex,
};

use super::engine::{
    DRY_RUN_JOURNAL_ID, JournalImage, PreparedFile, WriteContext, WriteEngine, mutation,
    parse_text, parse_updated_item,
};
use super::file::refuse_emacs_lock;
use super::journal::JournalRecord;
use super::org_edit::build_edit_patches;
use super::patch::{Patch, apply_patches};
use super::validation::{validate_id_change, validate_intent, validate_intent_for_file};

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

impl WriteEngine {
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

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::write::test_support::{context, refs_by_title, scheduled, settings};

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
}
