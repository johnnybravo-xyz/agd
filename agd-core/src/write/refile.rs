use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{AgdError, ByteSpan, IndexedDocument, Item, Settings, WorkspaceIndex};

use super::engine::{DRY_RUN_JOURNAL_ID, PreparedFile, WriteContext, WriteEngine};
use super::file::refuse_emacs_lock;
use super::patch::{Patch, apply_patches, line_ending};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RefileResult {
    pub schema: String,
    pub ok: bool,
    pub item: Item,
    pub diffs: Vec<String>,
    pub journal_ids: Vec<String>,
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

impl WriteEngine {
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
}

pub(super) fn relevel_subtree(
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

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::write::test_support::{context, settings};

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
