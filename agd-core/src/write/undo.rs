//! `WriteEngine::undo`: reverting a journal entry, or every file of its
//! transaction, by inverting its stored patches.
//!
//! A change is reverted only where the file still holds exactly what it wrote,
//! found again by its surrounding text when the file changed elsewhere;
//! otherwise nothing is written, and the `undo_conflict` error names the
//! heading, who changed it, and what to do.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Local;
use serde::{Deserialize, Serialize};

use crate::{AgdError, ByteSpan, ErrorCode, Settings, UNDO_SCHEMA};

use super::engine::{JournalImage, PreparedFile, WriteContext, WriteEngine, parse_text};
use super::file::replace_all_or_restore;
use super::journal::{JournalRecord, JournalStatus, content_hash};
use super::patch::{Patch, StoredPatch, apply_patches};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UndoResult {
    pub schema: String,
    pub ok: bool,
    pub journal_id: String,
    pub file: String,
    pub diff: String,
}

impl WriteEngine {
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
                ErrorCode::InvalidJournalState,
                format!(
                    "journal entry {} is {}, not a committed change; nothing to undo",
                    record.id,
                    format!("{:?}", record.status).to_lowercase()
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
            schema: UNDO_SCHEMA.to_string(),
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
                ErrorCode::IoFailed,
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
                ErrorCode::InvalidJournal,
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
        let author = record.actor_name();
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
            ErrorCode::UndoConflict,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::write::test_support::{
        context, first_ref, refs_by_title, scheduled, settings, state_change,
    };
    use crate::{ItemEdit, WorkspaceIndex};

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
        assert_eq!(again.code().as_str(), "invalid_journal_state");
        assert_eq!(
            engine
                .undo("no-such-entry", &context())
                .unwrap_err()
                .code()
                .as_str(),
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
        assert_eq!(error.code().as_str(), "undo_conflict");
        assert_eq!(
            error.to_string(),
            "can't undo: Rotate staging certs changed after codex edited it (work.org:1); edit it by hand"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), edited);
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
        let done = state_change("DONE");

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
        assert_eq!(error.code().as_str(), "undo_conflict");
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
        let item_ref = first_ref(&settings);
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl")).unwrap();
        let mutation = engine
            .edit(&item_ref, &scheduled("2026-10-15"), false, &context())
            .unwrap();
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.starts_with("#+TODO: TODO | DONE\r\n* TODO Exact  spacing :work:\r\n"));
        assert!(written.contains("SCHEDULED: <2026-10-15 Thu>\r\nbody\r\n"));
        engine.undo(&mutation.journal_id, &context()).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), original);

        let mutation = engine
            .edit(&item_ref, &scheduled("2026-10-16"), false, &context())
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
}
