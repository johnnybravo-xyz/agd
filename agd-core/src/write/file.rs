//! Files during a write: advisory locks in agd's data directory (never beside
//! the org files), refusing a file Emacs has locked, reading an org file, and
//! atomic replacement through a synced temporary file and a rename.
//!
//! Locks are taken in sorted order, so two writers never deadlock. A replaced
//! file keeps its mode and a new one gets the default mode. When one file of a
//! multi-file write cannot be written, the ones already written get their old
//! bytes back.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::{AgdError, ErrorCode};

use super::engine::{JournalImage, WriteEngine};
use crate::document::sha256_hex;

impl WriteEngine {
    /// Locks `paths` in sorted order (so two writers never deadlock),
    /// refuses Emacs-locked files, and settles interrupted writes to them.
    pub(super) fn lock_paths(&self, paths: &[PathBuf]) -> Result<Vec<File>, AgdError> {
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

    /// Takes the advisory write lock for `path`. Lock files live in agd's
    /// data directory (`locks/<sha256 of the path>.lock` beside the journal),
    /// never in the org directories, so synced folders stay clean.
    pub(super) fn lock_target(&self, path: &Path) -> Result<File, AgdError> {
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
                    ErrorCode::LockFailed,
                    format!("cannot open write lock {}: {error}", lock_path.display()),
                    error,
                )
            })?;
        lock.lock().map_err(|error| {
            AgdError::with_source(
                ErrorCode::LockFailed,
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
}

/// Writes every image's post-image in order; when one cannot be written,
/// the ones before it get their pre-image back and the error is returned.
pub(super) fn replace_all_or_restore(images: &[JournalImage<'_>]) -> Result<(), AgdError> {
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

/// An org file's text, or empty when it does not exist yet.
pub(super) fn read_org_file(path: &Path) -> Result<String, AgdError> {
    match fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|error| {
            AgdError::with_source(
                ErrorCode::InvalidUtf8,
                format!("org file is not UTF-8: {}", path.display()),
                error,
            )
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(AgdError::from(error)),
    }
}

pub(super) fn refuse_emacs_lock(path: &Path) -> Result<(), AgdError> {
    let parent = path
        .parent()
        .ok_or_else(|| AgdError::new(ErrorCode::InvalidPath, "target has no parent"))?;
    let name = path
        .file_name()
        .ok_or_else(|| AgdError::new(ErrorCode::InvalidPath, "target has no file name"))?;
    let lock = parent.join(format!(".#{}", name.to_string_lossy()));
    if fs::symlink_metadata(&lock).is_ok() {
        let owner = fs::read_link(&lock)
            .map(|target| target.to_string_lossy().into_owned())
            .or_else(|_| fs::read_to_string(&lock))
            .unwrap_or_else(|_| "ownership unavailable".to_string());
        return Err(AgdError::new(
            ErrorCode::EditorConflict,
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
pub(super) fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<(), AgdError> {
    let parent = path
        .parent()
        .ok_or_else(|| AgdError::new(ErrorCode::InvalidPath, "target has no parent"))?;
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

/// Creates `directory` and its parents, readable only by the user.
pub(super) fn create_private_directory(directory: &Path) -> Result<(), AgdError> {
    fs::create_dir_all(directory).map_err(AgdError::from)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
            .map_err(AgdError::from)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::CaptureDraft;
    use crate::write::test_support::{context, first_ref, settings, state_change, tree};

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
    fn refuses_editor_locks() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tasks.org");
        fs::write(&path, "* TODO Task\n").unwrap();
        let settings = settings(temp.path());
        let item_ref = first_ref(&settings);
        let engine = WriteEngine::new(settings.clone(), temp.path().join("journal.jsonl")).unwrap();
        fs::write(temp.path().join(".#tasks.org"), "user@host.123").unwrap();
        let error = engine
            .edit(&item_ref, &state_change("DONE"), false, &context())
            .unwrap_err();
        assert_eq!(error.code().as_str(), "editor_conflict");
    }
}
