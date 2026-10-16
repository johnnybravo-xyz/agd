//! The write journal, `journal.jsonl`: `JournalRecord`s appended before and
//! after each write, the latest record of each entry, review, and settling
//! interrupted writes from the file's bytes.
//!
//! A pending record is synced before its file changes, so every interruption
//! can be settled: a file holding the post-image was committed, one holding the
//! pre-image was never changed, and anything else is a conflict that blocks
//! writes to that file until `agd setup check --resolve`. The journal is the
//! user's alone (mode 0600 in a 0700 directory).

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::Local;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{AgdError, ErrorCode, JOURNAL_SCHEMA};

use super::engine::{JournalImage, WriteContext, WriteEngine};
use super::file::{
    atomic_replace, create_private_directory, read_org_file, replace_all_or_restore,
};
use super::patch::StoredPatch;

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

/// What a journaled write did, so every surface names a change the same
/// way without reading its diff. A review, undo or resolve keeps the
/// operation of the change it settles.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalOperation {
    Capture,
    /// A TODO state change.
    State,
    /// SCHEDULED set or removed.
    Schedule,
    /// DEADLINE set or removed.
    Deadline,
    Priority,
    Tags,
    /// Any other edit, or one that changes several of the above.
    Edit,
    Refile,
}

impl JournalOperation {
    /// The change in words, as `undid capture` or an Agents card says it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Capture => "capture",
            Self::State => "state change",
            Self::Schedule => "schedule",
            Self::Deadline => "deadline",
            Self::Priority => "priority",
            Self::Tags => "tags",
            Self::Edit => "edit",
            Self::Refile => "refile",
        }
    }
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
    /// What the write did; absent from records agd wrote before it
    /// journaled operations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<JournalOperation>,
    pub pre_hash: String,
    pub post_hash: String,
    pub patches: Vec<StoredPatch>,
    pub diff: String,
}

impl JournalRecord {
    /// The agent that made the change, `codex` for `mcp:codex`; None for a
    /// person's change from the CLI or TUI.
    pub fn agent(&self) -> Option<&str> {
        super::engine::agent_name(&self.actor)
    }

    /// Who made the change as agd names them: the agent (`codex`), or
    /// `you` for the CLI and TUI.
    pub fn actor_name(&self) -> &str {
        super::engine::actor_name(&self.actor)
    }

    /// What the change did in words (JournalOperation::label); `change`
    /// for a record without an operation.
    pub fn operation_label(&self) -> &'static str {
        self.operation.map_or("change", JournalOperation::label)
    }
}

impl WriteEngine {
    pub fn review(&self, journal_id: &str) -> Result<(), AgdError> {
        let records = self.latest_records()?;
        let mut record = records.get(journal_id).cloned().ok_or_else(|| {
            AgdError::new(
                ErrorCode::JournalNotFound,
                format!("journal entry not found: {journal_id}"),
            )
        })?;
        if record.status != JournalStatus::Committed {
            return Err(AgdError::new(
                ErrorCode::InvalidJournalState,
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
    pub(super) fn commit_transaction(
        &self,
        images: &[JournalImage<'_>],
        operation: JournalOperation,
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
                    operation,
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

    pub(super) fn commit_image(
        &self,
        image: JournalImage<'_>,
        operation: JournalOperation,
        context: &WriteContext,
    ) -> Result<JournalRecord, AgdError> {
        let mut record = pending_record(journal_id(), None, &image, operation, context);
        self.append_record(&record)?;
        atomic_replace(image.path, image.post_image.as_bytes())?;
        record.status = JournalStatus::Committed;
        record.timestamp = Local::now().to_rfc3339();
        self.append_record(&record)?;
        Ok(record)
    }

    pub(super) fn append_record(&self, record: &JournalRecord) -> Result<(), AgdError> {
        if let Some(parent) = self.journal_path.parent() {
            create_private_directory(parent)?;
        }
        let serialized = serde_json::to_vec(record).map_err(|error| {
            AgdError::new(
                ErrorCode::JournalFailed,
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

    pub(super) fn latest_records(&self) -> Result<BTreeMap<String, JournalRecord>, AgdError> {
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
                    ErrorCode::InvalidJournal,
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
    pub(super) fn recover_files(&self, paths: &[PathBuf]) -> Result<(), AgdError> {
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
    operation: JournalOperation,
    context: &WriteContext,
) -> JournalRecord {
    JournalRecord {
        schema: JOURNAL_SCHEMA.to_string(),
        id,
        status: JournalStatus::Pending,
        timestamp: Local::now().to_rfc3339(),
        actor: context.actor.clone(),
        project: context.project.clone(),
        file: image.path.to_string_lossy().into_owned(),
        item_ref: image.item_ref.map(str::to_string),
        transaction,
        operation: Some(operation),
        pre_hash: content_hash(image.pre_image),
        post_hash: content_hash(image.post_image),
        patches: image.patches.to_vec(),
        diff: image.diff.to_string(),
    }
}

pub(super) fn content_hash(source: &str) -> String {
    sha256_hex(source.as_bytes())
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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
    use crate::write::test_support::{context, settings};
    use crate::{Change, EditIntent, WorkspaceIndex};

    /// Appends a pending journal record for `file`, as a write interrupted
    /// between journaling and committing leaves it.
    fn append_interrupted_write(journal: &Path, file: &Path, pre: &str, post: &str) -> String {
        let record = JournalRecord {
            schema: JOURNAL_SCHEMA.to_string(),
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
            operation: Some(JournalOperation::State),
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
        assert_eq!(error.code().as_str(), "journal_conflict");
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
}
