use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{Duration, Local, Months, NaiveDate};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use similar::TextDiff;

use crate::{
    AgdError, ByteSpan, CaptureDraft, CaptureTemplate, HeadlineSpan, IndexedDocument, Item,
    MUTATION_SCHEMA, Mutation, PlanningTimestamp, RootSet, Settings, WorkspaceIndex,
    format_active_timestamp, render_capture_template,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Patch {
    pub span: ByteSpan,
    pub replacement: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StoredPatch {
    pub start: usize,
    pub end: usize,
    pub before: String,
    pub after: String,
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

#[derive(Clone, Debug)]
pub struct WriteContext {
    pub actor: String,
    pub project: String,
}

impl WriteContext {
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

struct JournalImage<'a> {
    path: &'a Path,
    item_ref: Option<&'a str>,
    pre_image: &'a str,
    post_image: &'a str,
    patches: &'a [StoredPatch],
    diff: &'a str,
}

impl WriteEngine {
    pub fn new(settings: Settings, journal_path: PathBuf) -> Result<Self, AgdError> {
        let roots = RootSet::new(&settings.files)?;
        let engine = Self {
            settings,
            roots,
            journal_path,
        };
        engine.recover_pending()?;
        Ok(engine)
    }

    pub fn with_default_journal(settings: Settings) -> Result<Self, AgdError> {
        let data = dirs::data_local_dir().ok_or_else(|| {
            AgdError::new("config", "cannot determine the platform data directory")
        })?;
        Self::new(settings, data.join("agd").join("journal.jsonl"))
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn edit(
        &self,
        item_ref: &str,
        intent: &EditIntent,
        dry_run: bool,
        context: &WriteContext,
    ) -> Result<Mutation, AgdError> {
        validate_intent(intent)?;
        let initial = WorkspaceIndex::load(self.settings.clone())?;
        let (initial_document, _) = initial.resolve(item_ref)?;
        let path = initial_document.path().to_path_buf();
        self.roots.resolve_write(&path, false)?;
        let _lock = self.lock_target(&path)?;
        refuse_emacs_lock(&path)?;

        let fresh = WorkspaceIndex::load(self.settings.clone())?;
        let (document, headline) = fresh.resolve(item_ref)?;
        if document.path() != path {
            return Err(AgdError::new(
                "stale_ref",
                "item resolved to a different file while acquiring the write lock",
            ));
        }
        let patches = build_edit_patches(document, headline, intent, &self.settings)?;
        let (post_image, stored) = apply_patches(document.source(), patches)?;
        if post_image == document.source() {
            return Err(AgdError::new("no_change", "edit does not change the item"));
        }
        let diff = unified_diff(&path, document.source(), &post_image);
        let updated = parse_updated_item(&path, &post_image, headline.item.line, &self.settings)?;
        if dry_run {
            return Ok(Mutation {
                schema: MUTATION_SCHEMA.to_string(),
                ok: true,
                item: updated,
                diff,
                journal_id: "dry-run".to_string(),
            });
        }

        let record = self.commit_image(
            JournalImage {
                path: &path,
                item_ref: Some(item_ref),
                pre_image: document.source(),
                post_image: &post_image,
                patches: &stored,
                diff: &diff,
            },
            context,
        )?;
        Ok(Mutation {
            schema: MUTATION_SCHEMA.to_string(),
            ok: true,
            item: updated,
            diff,
            journal_id: record.id,
        })
    }

    pub fn capture(
        &self,
        template_key: &str,
        draft: &CaptureDraft,
        body: &str,
        prompts: &BTreeMap<String, String>,
        dry_run: bool,
        context: &WriteContext,
    ) -> Result<Mutation, AgdError> {
        let template = self.settings.capture.get(template_key).ok_or_else(|| {
            AgdError::new(
                "unknown_template",
                format!("capture template '{template_key}' is not configured"),
            )
        })?;
        let (template, draft) = apply_agent_capture_gate(template, draft, &self.settings, context);
        let draft = &draft;
        let file = template.file.as_deref().ok_or_else(|| {
            AgdError::new(
                "unsupported_target",
                "capture by ID requires an existing target and is not valid for a new file",
            )
        })?;
        let path = self.roots.resolve_write(Path::new(file), true)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(AgdError::from)?;
        }
        self.roots.resolve_write(&path, true)?;
        let _lock = self.lock_target(&path)?;
        refuse_emacs_lock(&path)?;
        let pre_image = match fs::read(&path) {
            Ok(bytes) => String::from_utf8(bytes).map_err(|error| {
                AgdError::with_source(
                    "non_utf8",
                    format!("org file is not UTF-8: {}", path.display()),
                    error,
                )
            })?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(AgdError::from(error)),
        };
        let now = Local::now().naive_local();
        let rendered = render_capture_template(&template, draft, body, prompts, now)?;
        let (offset, level) = capture_target(&path, &pre_image, &template, &self.settings)?;
        let rendered = set_capture_level(&rendered, level);
        let newline = line_ending(&pre_image);
        let mut insertion = String::new();
        if offset > 0 && !pre_image[..offset].ends_with('\n') {
            insertion.push_str(newline);
        }
        insertion.push_str(&rendered);
        if !insertion.ends_with('\n') {
            insertion.push_str(newline);
        }
        let patches = vec![Patch {
            span: ByteSpan {
                start: offset,
                end: offset,
            },
            replacement: insertion,
        }];
        let (post_image, stored) = apply_patches(&pre_image, patches)?;
        let diff = unified_diff(&path, &pre_image, &post_image);
        let line = pre_image[..offset]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1;
        let updated = parse_updated_item(&path, &post_image, line, &self.settings)?;
        if dry_run {
            return Ok(Mutation {
                schema: MUTATION_SCHEMA.to_string(),
                ok: true,
                item: updated,
                diff,
                journal_id: "dry-run".to_string(),
            });
        }
        let record = self.commit_image(
            JournalImage {
                path: &path,
                item_ref: None,
                pre_image: &pre_image,
                post_image: &post_image,
                patches: &stored,
                diff: &diff,
            },
            context,
        )?;
        Ok(Mutation {
            schema: MUTATION_SCHEMA.to_string(),
            ok: true,
            item: updated,
            diff,
            journal_id: record.id,
        })
    }

    pub fn refile(
        &self,
        item_ref: &str,
        target_ref: &str,
        dry_run: bool,
        context: &WriteContext,
    ) -> Result<RefileResult, AgdError> {
        let initial = WorkspaceIndex::load(self.settings.clone())?;
        let (source_document, _) = initial.resolve(item_ref)?;
        let (target_document, _) = initial.resolve(target_ref)?;
        let source_path = source_document.path().to_path_buf();
        let target_path = target_document.path().to_path_buf();
        self.roots.resolve_write(&source_path, false)?;
        self.roots.resolve_write(&target_path, false)?;

        let mut paths = vec![source_path.clone(), target_path.clone()];
        paths.sort();
        paths.dedup();
        let mut locks = Vec::with_capacity(paths.len());
        for path in &paths {
            locks.push(self.lock_target(path)?);
            refuse_emacs_lock(path)?;
        }

        let fresh = WorkspaceIndex::load(self.settings.clone())?;
        let (source_document, source_headline) = fresh.resolve(item_ref)?;
        let (target_document, target_headline) = fresh.resolve(target_ref)?;
        if source_document.path() == target_document.path()
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

        if source_document.path() == target_document.path() {
            let path = source_document.path();
            let (post_image, stored) = apply_patches(
                source_document.source(),
                vec![
                    Patch {
                        span: source_headline.subtree_span,
                        replacement: String::new(),
                    },
                    Patch {
                        span: ByteSpan {
                            start: target_headline.subtree_span.end,
                            end: target_headline.subtree_span.end,
                        },
                        replacement: moved,
                    },
                ],
            )?;
            let diff = unified_diff(path, source_document.source(), &post_image);
            let item = find_refiled_item(
                path,
                &post_image,
                &source_headline.item,
                &target_headline.item,
                &self.settings,
            )?;
            if dry_run {
                return Ok(RefileResult {
                    schema: "agd.refile/v1".to_string(),
                    ok: true,
                    item,
                    diffs: vec![diff],
                    journal_ids: vec!["dry-run".to_string()],
                });
            }
            let record = self.commit_image(
                JournalImage {
                    path,
                    item_ref: Some(item_ref),
                    pre_image: source_document.source(),
                    post_image: &post_image,
                    patches: &stored,
                    diff: &diff,
                },
                context,
            )?;
            return Ok(RefileResult {
                schema: "agd.refile/v1".to_string(),
                ok: true,
                item,
                diffs: vec![diff],
                journal_ids: vec![record.id],
            });
        }

        let (source_post, source_patches) = apply_patches(
            source_document.source(),
            vec![Patch {
                span: source_headline.subtree_span,
                replacement: String::new(),
            }],
        )?;
        let (target_post, target_patches) = apply_patches(
            target_document.source(),
            vec![Patch {
                span: ByteSpan {
                    start: target_headline.subtree_span.end,
                    end: target_headline.subtree_span.end,
                },
                replacement: moved,
            }],
        )?;
        let source_diff = unified_diff(
            source_document.path(),
            source_document.source(),
            &source_post,
        );
        let target_diff = unified_diff(
            target_document.path(),
            target_document.source(),
            &target_post,
        );
        let item = find_refiled_item(
            target_document.path(),
            &target_post,
            &source_headline.item,
            &target_headline.item,
            &self.settings,
        )?;
        if dry_run {
            return Ok(RefileResult {
                schema: "agd.refile/v1".to_string(),
                ok: true,
                item,
                diffs: vec![source_diff, target_diff],
                journal_ids: vec!["dry-run".to_string()],
            });
        }

        let transaction = journal_id();
        let mut source_record = pending_record(
            format!("{transaction}:source"),
            Some(transaction.clone()),
            &JournalImage {
                path: source_document.path(),
                item_ref: Some(item_ref),
                pre_image: source_document.source(),
                post_image: &source_post,
                patches: &source_patches,
                diff: &source_diff,
            },
            context,
        );
        let mut target_record = pending_record(
            format!("{transaction}:target"),
            Some(transaction),
            &JournalImage {
                path: target_document.path(),
                item_ref: Some(target_ref),
                pre_image: target_document.source(),
                post_image: &target_post,
                patches: &target_patches,
                diff: &target_diff,
            },
            context,
        );
        self.append_record(&source_record)?;
        self.append_record(&target_record)?;
        atomic_replace(target_document.path(), target_post.as_bytes())?;
        if let Err(error) = atomic_replace(source_document.path(), source_post.as_bytes()) {
            let _ = atomic_replace(target_document.path(), target_document.source().as_bytes());
            return Err(error);
        }
        source_record.status = JournalStatus::Committed;
        source_record.timestamp = Local::now().to_rfc3339();
        target_record.status = JournalStatus::Committed;
        target_record.timestamp = source_record.timestamp.clone();
        self.append_record(&source_record)?;
        self.append_record(&target_record)?;
        Ok(RefileResult {
            schema: "agd.refile/v1".to_string(),
            ok: true,
            item,
            diffs: vec![source_diff, target_diff],
            journal_ids: vec![source_record.id, target_record.id],
        })
    }
    pub fn undo(
        &self,
        journal_id: Option<&str>,
        context: &WriteContext,
    ) -> Result<UndoResult, AgdError> {
        self.recover_pending()?;
        let records = self.latest_records()?;
        let selected = match journal_id {
            Some(id) => records.get(id),
            None => records
                .values()
                .filter(|record| {
                    matches!(
                        record.status,
                        JournalStatus::Committed | JournalStatus::Reviewed
                    )
                })
                .max_by_key(|record| record.timestamp.as_str()),
        }
        .ok_or_else(|| AgdError::new("nothing_to_undo", "no committed journal entry found"))?;
        let mut group: Vec<JournalRecord> = if let Some(transaction) = &selected.transaction {
            records
                .values()
                .filter(|record| {
                    record.transaction.as_ref() == Some(transaction)
                        && matches!(
                            record.status,
                            JournalStatus::Committed | JournalStatus::Reviewed
                        )
                })
                .cloned()
                .collect()
        } else {
            vec![selected.clone()]
        };
        group.sort_by(|left, right| left.file.cmp(&right.file));

        let mut locks = Vec::with_capacity(group.len());
        for record in &group {
            let path = PathBuf::from(&record.file);
            self.roots.resolve_write(&path, false)?;
            locks.push(self.lock_target(&path)?);
            refuse_emacs_lock(&path)?;
        }

        let mut prepared: Vec<(PathBuf, String, String, String)> = Vec::with_capacity(group.len());
        for record in &group {
            let path = PathBuf::from(&record.file);
            let current = fs::read_to_string(&path).map_err(|error| {
                AgdError::with_source(
                    "io",
                    format!("cannot read {}: {error}", path.display()),
                    error,
                )
            })?;
            if content_hash(&current) != record.post_hash {
                return Err(AgdError::new(
                    "undo_conflict",
                    format!(
                        "{} changed after journal entry {}",
                        path.display(),
                        record.id
                    ),
                ));
            }
            let (pre_image, _) = apply_patches(&current, inverse_patches(&record.patches))?;
            if content_hash(&pre_image) != record.pre_hash {
                return Err(AgdError::new(
                    "journal_corrupt",
                    format!(
                        "inverse patch for {} does not reproduce its pre-image",
                        record.id
                    ),
                ));
            }
            let diff = unified_diff(&path, &current, &pre_image);
            prepared.push((path, current, pre_image, diff));
        }

        let mut written: Vec<usize> = Vec::new();
        for (index, (path, _, pre_image, _)) in prepared.iter().enumerate() {
            if let Err(error) = atomic_replace(path, pre_image.as_bytes()) {
                for index in written.into_iter().rev() {
                    let (written_path, original, _, _) = &prepared[index];
                    let _ = atomic_replace(written_path, original.as_bytes());
                }
                return Err(error);
            }
            written.push(index);
        }
        for mut record in group.clone() {
            record.status = JournalStatus::Undone;
            record.timestamp = Local::now().to_rfc3339();
            record.actor = context.actor.clone();
            record.project = context.project.clone();
            self.append_record(&record)?;
        }
        Ok(UndoResult {
            schema: "agd.undo/v1".to_string(),
            ok: true,
            journal_id: selected
                .transaction
                .clone()
                .unwrap_or_else(|| selected.id.clone()),
            file: group
                .iter()
                .map(|record| record.file.as_str())
                .collect::<Vec<_>>()
                .join(","),
            diff: prepared
                .into_iter()
                .map(|(_, _, _, diff)| diff)
                .collect::<Vec<_>>()
                .join("\n"),
        })
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

    fn lock_target(&self, path: &Path) -> Result<File, AgdError> {
        let lock_path = sidecar_lock_path(path)?;
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

    fn append_record(&self, record: &JournalRecord) -> Result<(), AgdError> {
        if let Some(parent) = self.journal_path.parent() {
            fs::create_dir_all(parent).map_err(AgdError::from)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
                    .map_err(AgdError::from)?;
            }
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

    fn recover_pending(&self) -> Result<(), AgdError> {
        let mut conflicts = Vec::new();
        for mut record in self.latest_records()?.into_values().filter(|record| {
            matches!(
                record.status,
                JournalStatus::Pending | JournalStatus::Conflict
            )
        }) {
            let path = PathBuf::from(&record.file);
            let current = match fs::read_to_string(&path) {
                Ok(source) => content_hash(&source),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => content_hash(""),
                Err(error) => return Err(AgdError::from(error)),
            };
            let previous = record.status;
            record.status = if current == record.post_hash {
                JournalStatus::Committed
            } else if current == record.pre_hash {
                JournalStatus::Unapplied
            } else {
                JournalStatus::Conflict
            };
            if record.status != previous {
                record.timestamp = Local::now().to_rfc3339();
                self.append_record(&record)?;
            }
            if record.status == JournalStatus::Conflict {
                conflicts.push(record.id);
            }
        }
        if conflicts.is_empty() {
            Ok(())
        } else {
            Err(AgdError::new(
                "journal_recovery_conflict",
                format!(
                    "journal entries require explicit file resolution: {}",
                    conflicts.join(", ")
                ),
            ))
        }
    }
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
        });
        cursor = patch.span.end;
    }
    output.push_str(&source[cursor..]);
    Ok((output, stored))
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
) -> Result<Vec<Patch>, AgdError> {
    let source = document.source();
    let now = Local::now().naive_local();
    let mut patches = Vec::new();
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
    let mut scheduled = change_value(&headline.item.planning.scheduled, &intent.scheduled);
    let mut deadline = change_value(&headline.item.planning.deadline, &intent.deadline);
    let mut closed = headline.item.planning.closed.clone();
    let mut properties = intent.properties.clone();
    let mut note = intent.note.clone();

    let requested_done = state
        .as_ref()
        .is_some_and(|value| done_keywords(document).contains(value));
    let repeating = scheduled
        .as_ref()
        .is_some_and(|timestamp| timestamp.repeater.is_some())
        || deadline
            .as_ref()
            .is_some_and(|timestamp| timestamp.repeater.is_some());
    if requested_done && repeating {
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
        state = document.todo_active().first().cloned();
        properties.insert(
            "LAST_REPEAT".to_string(),
            Some(format!("[{}]", now.format("%Y-%m-%d %a %H:%M"))),
        );
        if settings.agenda.log_repeat != "nil" && settings.agenda.log_repeat != "false" {
            note = Some(note.unwrap_or_else(|| "State repeated".to_string()));
        }
        closed = None;
    } else if requested_done {
        if settings.agenda.log_done != "nil" && settings.agenda.log_done != "false" {
            closed = Some(PlanningTimestamp {
                date: now.format("%Y-%m-%d").to_string(),
                time: Some(now.format("%H:%M").to_string()),
                end_time: None,
                repeater: None,
                warning: None,
                raw: None,
            });
        }
    } else if !matches!(intent.state, Change::Keep) {
        closed = None;
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
        let line = render_headline(
            headline.level,
            state.as_deref(),
            priority.as_deref(),
            &title,
            &tags,
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

    if let Some(note) = note {
        if note.contains(['\n', '\r']) {
            return Err(AgdError::new(
                "invalid_note",
                "state notes must be one line",
            ));
        }
        let offset = headline.drawer_span.map_or_else(
            || {
                headline.planning_span.map_or(
                    after_headline_line(source, headline.line_span.end),
                    |span| span.end,
                )
            },
            |span| span.end,
        );
        let indent = headline
            .drawer_span
            .or(headline.planning_span)
            .map_or("", |span| leading_indent(source, span.start));
        let log = format!(
            "{indent}:LOGBOOK:{}{indent}- Note taken on [{}] :: {}{}{indent}:END:{}",
            line_ending(source),
            now.format("%Y-%m-%d %a %H:%M"),
            note,
            line_ending(source),
            line_ending(source)
        );
        patches.push(Patch {
            span: ByteSpan {
                start: offset,
                end: offset,
            },
            replacement: log,
        });
    }
    Ok(patches)
}

fn validate_intent(intent: &EditIntent) -> Result<(), AgdError> {
    if intent == &EditIntent::default() {
        return Err(AgdError::new("invalid_edit", "nothing to edit"));
    }
    if let Change::Set(title) = &intent.title
        && (title.trim().is_empty() || title.contains(['\n', '\r']))
    {
        return Err(AgdError::new(
            "invalid_title",
            "title must be one non-empty line",
        ));
    }
    if let Change::Set(priority) = &intent.priority
        && (priority.len() != 1 || !priority.bytes().all(|byte| byte.is_ascii_alphanumeric()))
    {
        return Err(AgdError::new(
            "invalid_priority",
            "priority must be one alphanumeric character",
        ));
    }
    for tag in intent.tags_add.iter().chain(&intent.tags_remove) {
        if tag.is_empty()
            || !tag.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'@' | b'#' | b'%')
            })
        {
            return Err(AgdError::new("invalid_tag", format!("invalid tag: {tag}")));
        }
    }
    for (key, value) in &intent.properties {
        if key.is_empty()
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err(AgdError::new(
                "invalid_property",
                format!("invalid property key: {key}"),
            ));
        }
        if value
            .as_ref()
            .is_some_and(|value| value.contains(['\n', '\r']))
        {
            return Err(AgdError::new(
                "invalid_property",
                format!("property {key} must be one line"),
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

fn capture_target(
    path: &Path,
    source: &str,
    template: &CaptureTemplate,
    settings: &Settings,
) -> Result<(usize, usize), AgdError> {
    if source.is_empty() {
        return Ok((0, 1));
    }
    let document =
        IndexedDocument::parse(path.to_path_buf(), source.as_bytes().to_vec(), settings)?;
    let target = if let Some(outline) = &template.outline_path {
        document
            .headlines()
            .iter()
            .find(|headline| &headline.item.olpath == outline)
    } else if let Some(heading) = &template.heading {
        document
            .headlines()
            .iter()
            .find(|headline| headline.item.title == *heading)
    } else if let Some(id) = &template.id {
        document
            .headlines()
            .iter()
            .find(|headline| headline.item.id.as_deref() == Some(id))
    } else {
        None
    };
    match target {
        Some(headline) => Ok((headline.subtree_span.end, headline.level + 1)),
        None if template.heading.is_none()
            && template.outline_path.is_none()
            && template.id.is_none() =>
        {
            Ok((source.len(), 1))
        }
        None => Err(AgdError::new(
            "capture_target_not_found",
            "configured capture target heading was not found",
        )),
    }
}

fn set_capture_level(rendered: &str, level: usize) -> String {
    let stars = rendered.bytes().take_while(|byte| *byte == b'*').count();
    if stars == 0 {
        return rendered.to_string();
    }
    format!("{}{}", "*".repeat(level), &rendered[stars..])
}

// With agents.capture_to = "inbox", an agent capture always lands in the
// inbox template without planning, so it reaches the day only through triage.
fn apply_agent_capture_gate(
    template: &CaptureTemplate,
    draft: &CaptureDraft,
    settings: &Settings,
    context: &WriteContext,
) -> (CaptureTemplate, CaptureDraft) {
    if !context.is_agent() || settings.agents.capture_to != "inbox" {
        return (template.clone(), draft.clone());
    }
    let inbox = settings
        .capture
        .get(&settings.agents.inbox_template)
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

fn sidecar_lock_path(path: &Path) -> Result<PathBuf, AgdError> {
    let parent = path
        .parent()
        .ok_or_else(|| AgdError::new("invalid_path", "target has no parent"))?;
    let name = path
        .file_name()
        .ok_or_else(|| AgdError::new("invalid_path", "target has no file name"))?;
    Ok(parent.join(format!(".{}.agd.lock", name.to_string_lossy())))
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

fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<(), AgdError> {
    let parent = path
        .parent()
        .ok_or_else(|| AgdError::new("invalid_path", "target has no parent"))?;
    let permissions = fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());
    let mut temporary = tempfile::Builder::new()
        .prefix(".agd-")
        .tempfile_in(parent)
        .map_err(AgdError::from)?;
    temporary.write_all(bytes).map_err(AgdError::from)?;
    if let Some(permissions) = permissions {
        temporary
            .as_file()
            .set_permissions(permissions)
            .map_err(AgdError::from)?;
    }
    temporary.as_file().sync_all().map_err(AgdError::from)?;
    temporary
        .persist(path)
        .map_err(|error| AgdError::from(error.error))?;
    sync_parent_directory(parent)
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
    Sha256::digest(source.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn unified_diff(path: &Path, before: &str, after: &str) -> String {
    TextDiff::from_lines(before, after)
        .unified_diff()
        .header(
            &format!("a/{}", path.display()),
            &format!("b/{}", path.display()),
        )
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
        let mut settings = Settings::default();
        settings.files.roots = vec![root.to_string_lossy().into_owned()];
        settings.files.agenda = vec!["**/*.org".to_string()];
        settings
    }

    fn context() -> WriteContext {
        WriteContext {
            actor: "test".to_string(),
            project: "fixture".to_string(),
        }
    }

    #[test]
    fn inbox_gate_redirects_only_agent_captures_and_drops_their_planning() {
        let temp = tempfile::tempdir().unwrap();
        let mut settings = settings(temp.path());
        settings.agents.capture_to = "inbox".to_string();
        settings.capture.insert(
            "w".to_string(),
            CaptureTemplate {
                file: Some("work.org".to_string()),
                heading: None,
                template: "* TODO %?".to_string(),
                ..CaptureTemplate::default()
            },
        );
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
        };
        let agent = WriteContext {
            actor: "mcp:claude-code".to_string(),
            project: "fixture".to_string(),
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
    fn edit_revalidates_writes_journal_and_undoes_only_unchanged_postimage() {
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
        engine.undo(Some(&mutation.journal_id), &context()).unwrap();
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
        assert_eq!(
            engine
                .undo(Some(&mutation.journal_id), &context())
                .unwrap_err()
                .code(),
            "undo_conflict"
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
        engine
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
        assert!(written.contains("* TODO Water plants\n"));
        assert!(written.contains(&format!("SCHEDULED: <{tomorrow}")));
        assert!(written.contains(":LAST_REPEAT: ["));
    }

    #[test]
    fn real_fixture_planning_edit_and_undo_are_byte_exact() {
        let temp = test_utils::fixtures::setup_test_org_files_with_dates().unwrap();
        let path = temp.path().join("agenda.org");
        let original = fs::read_to_string(&path).unwrap();
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
        engine.undo(Some(&mutation.journal_id), &context()).unwrap();
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
        engine
            .undo(Some(&result.journal_ids[0]), &context())
            .unwrap();
        assert_eq!(fs::read_to_string(source_path).unwrap(), source);
        assert_eq!(fs::read_to_string(target_path).unwrap(), target);
    }
}
