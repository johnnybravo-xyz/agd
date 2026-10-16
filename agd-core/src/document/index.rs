//! `WorkspaceIndex`: every agenda file under the roots, parsed, plus the
//! `:ID:`s of the other org files there; resolving an item ref to its heading;
//! and refreshing one changed path.
//!
//! A ref is the heading's `:ID:` when it is unique, else
//! `file:line#sha256:digest`. An ID that occurs more than once anywhere under
//! the roots is refused for writes (`id_conflict`). A non-UTF-8 file or a
//! symlink out of the roots is skipped with a reason instead of failing the
//! whole index.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::{AgdError, ErrorCode, Item, RootSet, Settings};

use super::headline::{HeadlineSpan, location_ref};
use super::org_document::OrgDocument;

#[derive(Clone, Debug)]
pub struct WorkspaceIndex {
    roots: RootSet,
    settings: Settings,
    documents: BTreeMap<PathBuf, OrgDocument>,
    ids: BTreeMap<String, Vec<(PathBuf, usize)>>,
    // `:ID:` values found in org files under the roots that are not agenda
    // files, so an ID shared with an archive or notes file is still ambiguous.
    outside_ids: BTreeMap<String, Vec<PathBuf>>,
    skipped: BTreeMap<PathBuf, String>,
}

fn scan_property_ids(path: &Path) -> Vec<String> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let key = line.get(..4)?;
            if !key.eq_ignore_ascii_case(":ID:") {
                return None;
            }
            let value = line[4..].trim();
            (!value.is_empty()).then(|| value.to_string())
        })
        .collect()
}

impl WorkspaceIndex {
    pub fn load(settings: Settings) -> Result<Self, AgdError> {
        let roots = RootSet::new(&settings.roots, &settings.agenda_files)?;
        let agenda_files = roots.agenda_files()?;
        let skipped = agenda_files
            .skipped
            .iter()
            .map(|(path, reason)| (path.clone(), reason.describe(path)))
            .collect();
        let mut index = Self {
            roots,
            settings,
            documents: BTreeMap::new(),
            ids: BTreeMap::new(),
            outside_ids: BTreeMap::new(),
            skipped,
        };
        for path in agenda_files.files {
            index.add_agenda_file(path)?;
        }
        for path in index.roots.all_org_files()?.files {
            if !index.documents.contains_key(&path) && !index.skipped.contains_key(&path) {
                index.add_outside_ids(&path);
            }
        }
        index.rebuild_ids();
        Ok(index)
    }

    /// Reads and parses one agenda file. A non-UTF-8 file is skipped with
    /// its reason instead of failing, so one unreadable file never hides the
    /// rest of the agenda.
    fn add_agenda_file(&mut self, path: PathBuf) -> Result<(), AgdError> {
        let bytes = std::fs::read(&path).map_err(|error| {
            AgdError::with_source(
                ErrorCode::IoFailed,
                format!("cannot read {}: {error}", path.display()),
                error,
            )
        })?;
        match OrgDocument::parse(path.clone(), bytes, &self.settings) {
            Ok(document) => {
                self.documents.insert(path, document);
            }
            Err(error) if error.code() == ErrorCode::InvalidUtf8 => {
                self.skipped.insert(path, error.to_string());
            }
            Err(error) => return Err(error),
        }
        Ok(())
    }

    /// Records the `:ID:`s of an org file under the roots that is not an
    /// agenda file.
    fn add_outside_ids(&mut self, path: &Path) {
        for id in scan_property_ids(path) {
            self.outside_ids
                .entry(id)
                .or_default()
                .push(path.to_path_buf());
        }
    }

    /// Agenda files left out of the index, with the reason: non-UTF-8
    /// content, or a symlink that resolves outside the roots.
    pub fn skipped_files(&self) -> impl Iterator<Item = (&Path, &str)> {
        self.skipped
            .iter()
            .map(|(path, reason)| (path.as_path(), reason.as_str()))
    }

    pub fn roots(&self) -> &RootSet {
        &self.roots
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Re-reads one changed path, as a file watcher reports it. An agenda
    /// file, including one created after the index loaded, is parsed again;
    /// another `.org` file under the roots only updates the IDs that make
    /// targets ambiguous; a removed file or directory leaves the index; a
    /// symlink resolving outside the roots is skipped. Other paths are
    /// ignored.
    pub fn refresh_path(&mut self, path: &Path) -> Result<(), AgdError> {
        if !path.exists() {
            let removed = crate::path::resolve_existing_prefix(path)?;
            self.forget_paths_under(&removed);
            self.rebuild_ids();
            return Ok(());
        }
        if path.extension().is_none_or(|extension| extension != "org") {
            return Ok(());
        }
        let path = match self.roots.resolve_read(path) {
            Ok(path) => path,
            Err(error) if error.code() == ErrorCode::InvalidPath => {
                let reason = crate::SkipReason::OutsideRoots.describe(path);
                self.skipped.insert(path.to_path_buf(), reason);
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        self.forget_paths_under(&path);
        if self.roots.is_agenda_file(&path)? {
            self.add_agenda_file(path)?;
        } else {
            self.add_outside_ids(&path);
        }
        self.rebuild_ids();
        Ok(())
    }

    /// Drops every document, skip and outside ID at or below `path`.
    fn forget_paths_under(&mut self, path: &Path) {
        self.documents.retain(|known, _| !known.starts_with(path));
        self.skipped.retain(|known, _| !known.starts_with(path));
        for locations in self.outside_ids.values_mut() {
            locations.retain(|location| !location.starts_with(path));
        }
        self.outside_ids
            .retain(|_, locations| !locations.is_empty());
    }

    /// Recounts `:ID:` values, then gives every item whose ID is not unique
    /// under the roots a file/line/digest ref, so two items never share a
    /// ref; resolving it still refuses the write as `id_conflict`.
    fn rebuild_ids(&mut self) {
        self.ids.clear();
        for (path, document) in &self.documents {
            for (index, headline) in document.headlines().iter().enumerate() {
                if let Some(id) = &headline.item.id {
                    self.ids
                        .entry(id.clone())
                        .or_default()
                        .push((path.clone(), index));
                }
            }
        }
        let duplicated: BTreeSet<String> =
            self.duplicate_ids().map(|(id, _)| id.to_string()).collect();
        for document in self.documents.values_mut() {
            for headline in &mut document.headlines {
                headline.item.item_ref = match &headline.item.id {
                    Some(id) if !duplicated.contains(id) => id.clone(),
                    _ => location_ref(
                        Path::new(&headline.item.file),
                        headline.item.line,
                        &headline.digest,
                    ),
                };
            }
        }
    }

    pub fn documents(&self) -> impl Iterator<Item = &OrgDocument> {
        self.documents.values()
    }

    pub fn items(&self) -> impl Iterator<Item = &Item> {
        self.documents
            .values()
            .flat_map(|document| document.headlines().iter().map(|headline| &headline.item))
    }

    pub fn duplicate_ids(&self) -> impl Iterator<Item = (&str, &[(PathBuf, usize)])> {
        self.ids
            .iter()
            .filter(|(id, _)| self.id_count(id) > 1)
            .map(|(id, locations)| (id.as_str(), locations.as_slice()))
    }

    /// Whether any heading under the roots, agenda file or not, has this
    /// `:ID:`.
    pub fn id_in_use(&self, id: &str) -> bool {
        self.ids.contains_key(id) || self.outside_ids.contains_key(id)
    }

    /// How many headings under the roots, agenda files or not, have this
    /// `:ID:`.
    fn id_count(&self, id: &str) -> usize {
        self.ids.get(id).map_or(0, Vec::len) + self.outside_ids.get(id).map_or(0, Vec::len)
    }

    fn refuse_duplicate_id(&self, id: Option<&str>) -> Result<(), AgdError> {
        let Some(id) = id else {
            return Ok(());
        };
        let count = self.id_count(id);
        if count > 1 {
            return Err(AgdError::new(
                ErrorCode::IdConflict,
                format!("ID '{id}' occurs {count} times under the configured roots"),
            ));
        }
        Ok(())
    }

    pub fn document(&self, path: &Path) -> Option<&OrgDocument> {
        self.documents.get(path)
    }

    pub fn resolve(&self, item_ref: &str) -> Result<(&OrgDocument, &HeadlineSpan), AgdError> {
        if !item_ref.contains("#sha256:") {
            self.refuse_duplicate_id(Some(item_ref))?;
            return match self.ids.get(item_ref).and_then(|matches| matches.first()) {
                Some((path, index)) => {
                    let document = self.documents.get(path).expect("indexed document");
                    Ok((document, &document.headlines()[*index]))
                }
                None if self.outside_ids.contains_key(item_ref) => Err(AgdError::new(
                    ErrorCode::RefConflict,
                    format!("ID '{item_ref}' is in a file outside the agenda files"),
                )),
                None => Err(AgdError::new(
                    ErrorCode::RefConflict,
                    format!("no item has ID '{item_ref}'"),
                )),
            };
        }

        let (location, digest) = item_ref.rsplit_once("#sha256:").ok_or_else(|| {
            AgdError::new(
                ErrorCode::InvalidRef,
                format!("invalid item ref: {item_ref}"),
            )
        })?;
        if digest.len() != 32 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(AgdError::new(
                ErrorCode::InvalidRef,
                format!("invalid SHA-256 digest in item ref: {item_ref}"),
            ));
        }
        let (file, line) = location.rsplit_once(':').ok_or_else(|| {
            AgdError::new(
                ErrorCode::InvalidRef,
                format!("item ref has no line hint: {item_ref}"),
            )
        })?;
        let line = line.parse::<usize>().map_err(|_| {
            AgdError::new(
                ErrorCode::InvalidRef,
                format!("invalid line hint in item ref: {item_ref}"),
            )
        })?;
        let requested = Path::new(file);
        self.roots.resolve_read(requested)?;
        if let Some(reason) = self.skipped.get(requested) {
            return Err(AgdError::new(ErrorCode::InvalidUtf8, reason.clone()));
        }
        let document = self
            .documents
            .get(requested)
            .or_else(|| {
                requested
                    .canonicalize()
                    .ok()
                    .and_then(|canonical| self.documents.get(&canonical))
            })
            .ok_or_else(|| {
                AgdError::new(
                    ErrorCode::RefConflict,
                    format!(
                        "referenced file is not an agenda file: {}",
                        requested.display()
                    ),
                )
            })?;
        if let Some(headline) = document.headline_at_line(line)
            && headline.digest == digest
        {
            self.refuse_duplicate_id(headline.item.id.as_deref())?;
            return Ok((document, headline));
        }
        let matches: Vec<&HeadlineSpan> = document
            .headlines()
            .iter()
            .filter(|headline| headline.digest == digest)
            .collect();
        match matches.as_slice() {
            [headline] => {
                self.refuse_duplicate_id(headline.item.id.as_deref())?;
                Ok((document, headline))
            }
            [] => {
                let mut error = AgdError::new(
                    ErrorCode::RefConflict,
                    format!("headline bytes no longer match ref: {item_ref}"),
                );
                if let Some(current) = document.headline_at_line(line) {
                    error = error.with_current(current.item.clone());
                }
                Err(error)
            }
            _ => Err(AgdError::new(
                ErrorCode::RefConflict,
                format!("headline digest is ambiguous in {}", requested.display()),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::test_support::settings;

    #[test]
    fn digest_ref_relocates_uniquely_and_reports_stale_current() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tasks.org");
        std::fs::write(&path, "* TODO One\n* TODO Two\n").unwrap();
        let index = WorkspaceIndex::load(settings(temp.path())).unwrap();
        let item_ref = index.items().nth(1).unwrap().item_ref.clone();
        assert_eq!(index.resolve(&item_ref).unwrap().1.item.title, "Two");

        std::fs::write(&path, "intro\n* TODO One\n* TODO Two\n").unwrap();
        let relocated = WorkspaceIndex::load(settings(temp.path())).unwrap();
        assert_eq!(relocated.resolve(&item_ref).unwrap().1.item.line, 3);

        std::fs::write(&path, "intro\n* TODO One\n* TODO Changed\n").unwrap();
        let stale = WorkspaceIndex::load(settings(temp.path())).unwrap();
        let error = stale.resolve(&item_ref).unwrap_err();
        assert_eq!(error.code().as_str(), "ref_conflict");
    }

    #[test]
    fn non_utf8_agenda_file_is_skipped_for_reads_and_refused_for_writes() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("good.org"), "* TODO Readable\n").unwrap();
        let bad = temp.path().join("bad.org");
        std::fs::write(&bad, b"* TODO Latin-1 \xe9\n").unwrap();
        let index = WorkspaceIndex::load(settings(temp.path())).unwrap();
        assert_eq!(index.items().count(), 1);
        let skipped: Vec<_> = index.skipped_files().collect();
        assert_eq!(skipped.len(), 1);
        assert!(skipped[0].0.ends_with("bad.org"));
        let canonical = bad.canonicalize().unwrap();
        let bad_ref = format!(
            "{}:1#sha256:00000000000000000000000000000000",
            canonical.display()
        );
        assert_eq!(
            index.resolve(&bad_ref).unwrap_err().code().as_str(),
            "invalid_utf8"
        );
    }

    #[test]
    fn ids_in_non_agenda_files_make_targets_ambiguous() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("tasks.org"),
            "* TODO One\n:PROPERTIES:\n:ID: shared\n:END:\n",
        )
        .unwrap();
        std::fs::create_dir(temp.path().join("archive")).unwrap();
        std::fs::write(
            temp.path().join("archive/notes.org"),
            "* Notes\n:PROPERTIES:\n:id:   shared\n:END:\n",
        )
        .unwrap();
        let mut settings = settings(temp.path());
        settings.agenda_files = vec!["tasks.org".to_string()];
        let index = WorkspaceIndex::load(settings).unwrap();
        assert_eq!(
            index.resolve("shared").unwrap_err().code().as_str(),
            "id_conflict"
        );
        let item_ref = index.items().next().unwrap().item_ref.clone();
        assert_eq!(
            index.resolve(&item_ref).unwrap_err().code().as_str(),
            "id_conflict"
        );
        assert_eq!(index.duplicate_ids().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_leaving_the_roots_are_skipped_not_fatal() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("tasks.org"), "* TODO Kept\n").unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.org"), "* TODO Secret\n").unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("secret.org"),
            temp.path().join("escape.org"),
        )
        .unwrap();
        let index = WorkspaceIndex::load(settings(temp.path())).unwrap();
        let titles: Vec<&str> = index.items().map(|item| item.title.as_str()).collect();
        assert_eq!(titles, ["Kept"]);
        let skipped: Vec<_> = index.skipped_files().collect();
        assert_eq!(skipped.len(), 1);
        assert!(skipped[0].0.ends_with("escape.org"));
        assert!(
            skipped[0].1.contains("outside the roots"),
            "{}",
            skipped[0].1
        );
    }

    #[test]
    fn files_created_after_loading_join_the_index_by_agenda_pattern() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("archive")).unwrap();
        let mut settings = settings(temp.path());
        settings.agenda_files = vec!["inbox.org".to_string()];
        let mut index = WorkspaceIndex::load(settings).unwrap();
        assert_eq!(index.items().count(), 0);

        let inbox = temp.path().join("inbox.org");
        std::fs::write(&inbox, "* TODO First task\n").unwrap();
        index.refresh_path(&inbox).unwrap();
        let titles: Vec<&str> = index.items().map(|item| item.title.as_str()).collect();
        assert_eq!(titles, ["First task"]);

        // Not an agenda file: its ID still makes the agenda's copy ambiguous.
        std::fs::write(
            &inbox,
            "* TODO First task\n:PROPERTIES:\n:ID: shared\n:END:\n",
        )
        .unwrap();
        index.refresh_path(&inbox).unwrap();
        let notes = temp.path().join("archive/notes.org");
        std::fs::write(&notes, "* Notes\n:PROPERTIES:\n:ID: shared\n:END:\n").unwrap();
        index.refresh_path(&notes).unwrap();
        assert_eq!(index.items().count(), 1);
        assert_eq!(
            index.resolve("shared").unwrap_err().code().as_str(),
            "id_conflict"
        );

        std::fs::remove_file(&notes).unwrap();
        index.refresh_path(&notes).unwrap();
        assert_eq!(index.resolve("shared").unwrap().1.item.title, "First task");
        std::fs::remove_file(&inbox).unwrap();
        index.refresh_path(&inbox).unwrap();
        assert_eq!(index.items().count(), 0);
    }

    #[test]
    fn refs_to_missing_files_outside_the_roots_are_outside_roots() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("tasks.org"), "* TODO One\n").unwrap();
        let index = WorkspaceIndex::load(settings(temp.path())).unwrap();
        let outside = tempfile::tempdir().unwrap();
        let error = index
            .resolve(&format!(
                "{}:1#sha256:00000000000000000000000000000000",
                outside.path().join("no/such/file").display()
            ))
            .unwrap_err();
        assert_eq!(error.code().as_str(), "invalid_path");
    }

    #[test]
    fn items_sharing_an_id_get_distinct_location_refs_that_refuse_writes() {
        let temp = tempfile::tempdir().unwrap();
        let tasks = temp.path().join("tasks.org");
        std::fs::write(
            &tasks,
            "* TODO One\n:PROPERTIES:\n:ID: dup-id-1\n:END:\n* TODO Two\n:PROPERTIES:\n:ID: dup-id-1\n:END:\n",
        )
        .unwrap();
        let mut index = WorkspaceIndex::load(settings(temp.path())).unwrap();
        let refs: Vec<String> = index.items().map(|item| item.item_ref.clone()).collect();
        assert_eq!(refs.len(), 2);
        assert_ne!(refs[0], refs[1]);
        // Neither the location refs nor the ID itself selects one of them.
        for item_ref in refs.iter().map(String::as_str).chain(["dup-id-1"]) {
            assert_eq!(
                index.resolve(item_ref).unwrap_err().code().as_str(),
                "id_conflict"
            );
        }
        assert!(refs.iter().all(|item_ref| item_ref.contains("#sha256:")));

        // Once the ID is unique again, the item's ref is its ID.
        std::fs::write(
            &tasks,
            "* TODO One\n:PROPERTIES:\n:ID: dup-id-1\n:END:\n* TODO Two\n",
        )
        .unwrap();
        index.refresh_path(&tasks).unwrap();
        assert_eq!(index.items().next().unwrap().item_ref, "dup-id-1");
    }
}
