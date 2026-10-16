//! Which paths agd may touch. [`RootSet`] confines reads and writes to the
//! configured roots and lists the org and agenda files under them
//! ([`OrgFiles`]); [`config_dir`] and [`data_dir`] are agd's own directories.
//!
//! `..` segments are refused, a symlink is followed only while it stays inside
//! a root and is never written through, and an entry discovery cannot read is
//! skipped with a [`SkipReason`] instead of failing the walk.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;

use crate::{AgdError, ErrorCode};

/// agd's configuration directory: `$XDG_CONFIG_HOME/agd`, else
/// `~/.config/agd`, on every Unix including macOS; the platform
/// configuration directory on Windows.
pub fn config_dir() -> Result<PathBuf, AgdError> {
    base_dir("XDG_CONFIG_HOME", ".config", dirs::config_dir).map(|base| base.join("agd"))
}

/// agd's data directory, which holds the journal: `$XDG_DATA_HOME/agd`, else
/// `~/.local/share/agd`, on every Unix including macOS; the platform local
/// data directory on Windows.
pub fn data_dir() -> Result<PathBuf, AgdError> {
    base_dir("XDG_DATA_HOME", ".local/share", dirs::data_local_dir).map(|base| base.join("agd"))
}

fn base_dir(
    variable: &str,
    home_relative: &str,
    windows: fn() -> Option<PathBuf>,
) -> Result<PathBuf, AgdError> {
    if cfg!(windows) {
        return windows().ok_or_else(|| {
            AgdError::new(
                ErrorCode::InvalidConfig,
                format!("cannot determine the Windows directory for {variable}"),
            )
        });
    }
    // The XDG Base Directory spec says a relative value is invalid and must be ignored.
    if let Some(value) = std::env::var_os(variable) {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            return Ok(path);
        }
    }
    let home = dirs::home_dir().ok_or_else(|| {
        AgdError::new(
            ErrorCode::InvalidConfig,
            format!("cannot determine the home directory for ~/{home_relative}; set {variable}"),
        )
    })?;
    Ok(home.join(home_relative))
}

/// Org files discovered under the roots, plus the paths discovery left out.
#[derive(Clone, Debug, Default)]
pub struct OrgFiles {
    /// Canonical paths, sorted.
    pub files: Vec<PathBuf>,
    /// Path as discovered -> why it was left out.
    pub skipped: BTreeMap<PathBuf, SkipReason>,
}

/// Why discovery left a path out instead of failing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SkipReason {
    /// A symlinked file or directory that resolves outside every root.
    OutsideRoots,
    /// An entry the walk could not read, such as a dangling symlink.
    Unreadable(String),
}

impl SkipReason {
    /// The `agd setup check` problem code for this reason.
    pub fn code(&self) -> &'static str {
        match self {
            Self::OutsideRoots => ErrorCode::InvalidPath.as_str(),
            Self::Unreadable(_) => ErrorCode::WalkFailed.as_str(),
        }
    }

    /// The reason alone, for a report that names the path separately.
    pub fn message(&self) -> String {
        match self {
            Self::OutsideRoots => "symlink resolves outside the roots; skipped".to_string(),
            Self::Unreadable(error) => format!("cannot read; skipped: {error}"),
        }
    }

    /// One line naming the path and the reason, for warnings.
    pub fn describe(&self, path: &Path) -> String {
        format!("{}: {}", path.display(), self.message())
    }
}

#[derive(Clone, Debug)]
pub struct RootSet {
    roots: Vec<PathBuf>,
    agenda: Vec<String>,
}

impl RootSet {
    /// `roots` are the only directories agd may read or write; `agenda`
    /// holds the agenda file patterns (files, directories or globs).
    pub fn new(roots: &[String], agenda: &[String]) -> Result<Self, AgdError> {
        let configured_roots = roots;
        let mut roots = Vec::with_capacity(configured_roots.len());
        for configured in configured_roots {
            let expanded = shellexpand::tilde(configured).into_owned();
            let canonical = PathBuf::from(&expanded).canonicalize().map_err(|error| {
                AgdError::with_source(
                    ErrorCode::InvalidRoot,
                    format!("cannot resolve root '{configured}': {error}"),
                    error,
                )
            })?;
            if !canonical.is_dir() {
                return Err(AgdError::new(
                    ErrorCode::InvalidRoot,
                    format!("root '{configured}' is not a directory"),
                ));
            }
            roots.push(canonical);
        }
        roots.sort();
        roots.dedup();
        if roots.is_empty() {
            return Err(AgdError::new(
                ErrorCode::InvalidRoot,
                "roots must contain at least one directory",
            ));
        }
        Ok(Self {
            roots,
            agenda: agenda.to_vec(),
        })
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.roots.iter().any(|root| path.starts_with(root))
    }

    /// Whether discovery would list this canonical `.org` path as an agenda
    /// file: an exact absolute entry, or a pattern matching it relative to
    /// its root outside hidden directories (which the walk skips). Lets a
    /// file created after the index loaded join it.
    pub fn is_agenda_file(&self, canonical: &Path) -> Result<bool, AgdError> {
        if canonical
            .extension()
            .is_none_or(|extension| extension != "org")
        {
            return Ok(false);
        }
        let mut relative_patterns = Vec::new();
        for pattern in self.agenda_patterns() {
            let path = Path::new(&pattern);
            if !path.is_absolute() {
                relative_patterns.push(pattern);
            } else if path.canonicalize().is_ok_and(|exact| exact == canonical) {
                return Ok(true);
            }
        }
        if relative_patterns.is_empty() {
            return Ok(false);
        }
        let matcher = build_globset(&relative_patterns)?;
        Ok(self.roots.iter().any(|root| {
            canonical
                .strip_prefix(root)
                .is_ok_and(|relative| !has_hidden_component(relative) && matcher.is_match(relative))
        }))
    }

    pub fn resolve_read(&self, requested: &Path) -> Result<PathBuf, AgdError> {
        reject_parent_segments(requested)?;
        if requested.is_absolute() {
            return self.validate_existing_read(requested);
        }

        let mut matches = BTreeSet::new();
        for root in &self.roots {
            let candidate = root.join(requested);
            if candidate.exists() {
                matches.insert(self.validate_existing_read(&candidate)?);
            }
        }
        match matches.len() {
            0 => Err(AgdError::new(
                ErrorCode::FileNotFound,
                format!("file is not under a root: {}", requested.display()),
            )),
            1 => Ok(matches.into_iter().next().expect("one path")),
            _ => Err(AgdError::new(
                ErrorCode::PathConflict,
                format!(
                    "relative path matches more than one root: {}",
                    requested.display()
                ),
            )),
        }
    }

    pub fn resolve_write(&self, requested: &Path, create: bool) -> Result<PathBuf, AgdError> {
        reject_parent_segments(requested)?;
        let lexical = if requested.is_absolute() {
            requested.to_path_buf()
        } else if self.roots.len() == 1 {
            self.roots[0].join(requested)
        } else {
            let existing: Vec<PathBuf> = self
                .roots
                .iter()
                .map(|root| root.join(requested))
                .filter(|path| path.exists())
                .collect();
            match existing.as_slice() {
                [path] => path.clone(),
                [] if create => {
                    return Err(AgdError::new(
                        ErrorCode::PathConflict,
                        "capture into multiple roots requires an absolute target",
                    ));
                }
                [] => {
                    return Err(AgdError::new(
                        ErrorCode::FileNotFound,
                        format!("file is not under a root: {}", requested.display()),
                    ));
                }
                _ => {
                    return Err(AgdError::new(
                        ErrorCode::PathConflict,
                        format!(
                            "relative path matches more than one root: {}",
                            requested.display()
                        ),
                    ));
                }
            }
        };

        if let Ok(metadata) = std::fs::symlink_metadata(&lexical)
            && metadata.file_type().is_symlink()
        {
            return Err(AgdError::new(
                ErrorCode::InvalidPath,
                format!("writes to symlinks are refused: {}", lexical.display()),
            ));
        }

        let parent = lexical.parent().ok_or_else(|| {
            AgdError::new(
                ErrorCode::InvalidPath,
                format!("path has no parent: {}", lexical.display()),
            )
        })?;
        let canonical_parent = nearest_existing(parent)?.canonicalize().map_err(|error| {
            AgdError::with_source(
                ErrorCode::InvalidPath,
                format!("cannot resolve parent of {}: {error}", lexical.display()),
                error,
            )
        })?;
        if !self.contains(&canonical_parent) {
            return Err(AgdError::new(
                ErrorCode::InvalidPath,
                format!("path is outside the roots: {}", lexical.display()),
            ));
        }
        if lexical.exists() && !lexical.is_file() {
            return Err(AgdError::new(
                ErrorCode::InvalidPath,
                format!("write target is not a regular file: {}", lexical.display()),
            ));
        }
        Ok(lexical)
    }

    /// The agenda files: every `.org` file under the roots that matches an
    /// agenda pattern.
    pub fn agenda_files(&self) -> Result<OrgFiles, AgdError> {
        self.org_files_matching(&self.agenda_patterns())
    }

    /// Every `.org` file under the roots, agenda file or not.
    pub fn all_org_files(&self) -> Result<OrgFiles, AgdError> {
        self.org_files_matching(&[String::from("**/*.org")])
    }

    fn agenda_patterns(&self) -> Vec<String> {
        if self.agenda.is_empty() {
            vec!["**/*.org".to_string()]
        } else {
            self.agenda.clone()
        }
    }

    fn org_files_matching(&self, patterns: &[String]) -> Result<OrgFiles, AgdError> {
        let mut relative_patterns = Vec::new();
        let mut files = BTreeSet::new();
        let mut skipped = BTreeMap::new();
        for pattern in patterns {
            let path = Path::new(pattern);
            reject_parent_segments(path)?;
            if path.is_absolute() {
                if pattern.contains(['*', '?', '[', ']']) {
                    return Err(AgdError::new(
                        ErrorCode::InvalidAgendaPattern,
                        format!("absolute agenda entries must be exact files: {pattern}"),
                    ));
                }
                let resolved = self.validate_existing_read(path)?;
                if resolved
                    .extension()
                    .is_none_or(|extension| extension != "org")
                {
                    return Err(AgdError::new(
                        ErrorCode::InvalidAgendaFile,
                        format!("agenda file does not have an .org extension: {pattern}"),
                    ));
                }
                files.insert(resolved);
            } else {
                relative_patterns.push(pattern.clone());
            }
        }
        if relative_patterns.is_empty() {
            return Ok(OrgFiles {
                files: files.into_iter().collect(),
                skipped,
            });
        }

        let matcher = build_globset(&relative_patterns)?;
        for root in &self.roots {
            // Symlinked directories that resolve outside the roots; nothing
            // below them is read.
            let mut escaped_directories: Vec<PathBuf> = Vec::new();
            for entry in WalkBuilder::new(root).follow_links(true).build() {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) if is_emacs_lock(&error) => continue,
                    Err(error) => match walk_error_path(&error) {
                        Some(path) => {
                            skipped.insert(path, SkipReason::Unreadable(error.to_string()));
                            continue;
                        }
                        None => {
                            return Err(AgdError::new(
                                ErrorCode::WalkFailed,
                                format!("cannot walk {}: {error}", root.display()),
                            ));
                        }
                    },
                };
                let path = entry.path();
                if escaped_directories
                    .iter()
                    .any(|directory| path.starts_with(directory))
                {
                    continue;
                }
                let is_directory = entry.file_type().is_some_and(|kind| kind.is_dir());
                let is_org_file = entry.file_type().is_some_and(|kind| kind.is_file())
                    && path.extension().is_some_and(|extension| extension == "org");
                if entry.path_is_symlink()
                    && (is_directory || is_org_file)
                    && !self.symlink_target_inside(path)
                {
                    skipped.insert(path.to_path_buf(), SkipReason::OutsideRoots);
                    if is_directory {
                        escaped_directories.push(path.to_path_buf());
                    }
                    continue;
                }
                let relative = path.strip_prefix(root).unwrap_or(path);
                if is_org_file && matcher.is_match(relative) {
                    files.insert(self.validate_existing_read(path)?);
                }
            }
        }
        Ok(OrgFiles {
            files: files.into_iter().collect(),
            skipped,
        })
    }

    fn symlink_target_inside(&self, path: &Path) -> bool {
        path.canonicalize()
            .is_ok_and(|canonical| self.contains(&canonical))
    }

    /// Containment is checked before the file itself is resolved, so a path
    /// outside the roots is `invalid_path` whether or not it exists.
    fn validate_existing_read(&self, path: &Path) -> Result<PathBuf, AgdError> {
        if !self.contains(&resolve_existing_prefix(path)?) {
            return Err(AgdError::new(
                ErrorCode::InvalidPath,
                format!("resolved path is outside the roots: {}", path.display()),
            ));
        }
        let canonical = path.canonicalize().map_err(|error| {
            AgdError::with_source(
                ErrorCode::FileNotFound,
                format!("cannot resolve file {}: {error}", path.display()),
                error,
            )
        })?;
        if !self.contains(&canonical) {
            return Err(AgdError::new(
                ErrorCode::InvalidPath,
                format!("resolved path is outside the roots: {}", path.display()),
            ));
        }
        if !canonical.is_file() {
            return Err(AgdError::new(
                ErrorCode::InvalidPath,
                format!("path is not a regular file: {}", path.display()),
            ));
        }
        Ok(canonical)
    }
}

fn reject_parent_segments(path: &Path) -> Result<(), AgdError> {
    if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err(AgdError::new(
            ErrorCode::InvalidPath,
            format!("'..' is not allowed in paths: {}", path.display()),
        ));
    }
    Ok(())
}

/// `path` with its longest existing prefix canonicalized and the missing
/// rest appended as written; for an existing path, its canonical form.
pub(crate) fn resolve_existing_prefix(path: &Path) -> Result<PathBuf, AgdError> {
    let existing = nearest_existing(path)?;
    let canonical = existing.canonicalize().map_err(|error| {
        AgdError::with_source(
            ErrorCode::InvalidPath,
            format!("cannot resolve {}: {error}", existing.display()),
            error,
        )
    })?;
    let rest = path.strip_prefix(existing).unwrap_or(Path::new(""));
    Ok(if rest.as_os_str().is_empty() {
        canonical
    } else {
        canonical.join(rest)
    })
}

/// The entry a walk error is about, when it names one.
fn walk_error_path(error: &ignore::Error) -> Option<PathBuf> {
    match error {
        ignore::Error::WithPath { path, .. } => Some(path.clone()),
        ignore::Error::WithDepth { err, .. } | ignore::Error::WithLineNumber { err, .. } => {
            walk_error_path(err)
        }
        ignore::Error::Loop { child, .. } => Some(child.clone()),
        _ => None,
    }
}

fn nearest_existing(path: &Path) -> Result<&Path, AgdError> {
    let mut current = path;
    loop {
        if current.exists() {
            return Ok(current);
        }
        current = current.parent().ok_or_else(|| {
            AgdError::new(
                ErrorCode::InvalidPath,
                format!("no existing ancestor for {}", path.display()),
            )
        })?;
    }
}

/// A component starting with `.`, which the discovery walk never enters.
fn has_hidden_component(relative: &Path) -> bool {
    relative
        .components()
        .any(|component| component.as_os_str().to_string_lossy().starts_with('.'))
}

fn build_globset(patterns: &[String]) -> Result<GlobSet, AgdError> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        reject_parent_segments(Path::new(pattern))?;
        if Path::new(pattern).is_absolute() {
            return Err(AgdError::new(
                ErrorCode::InvalidAgendaPattern,
                format!("agenda pattern must be relative to a root: {pattern}"),
            ));
        }
        builder.add(Glob::new(pattern).map_err(|error| {
            AgdError::new(
                ErrorCode::InvalidAgendaPattern,
                format!("invalid agenda pattern '{pattern}': {error}"),
            )
        })?);
    }
    builder.build().map_err(|error| {
        AgdError::new(
            ErrorCode::InvalidAgendaPattern,
            format!("cannot build agenda patterns: {error}"),
        )
    })
}

/// Emacs marks a buffer with unsaved changes by a dangling `.#name`
/// symlink; following it fails, but it is never an agenda file.
fn is_emacs_lock(error: &ignore::Error) -> bool {
    match error {
        ignore::Error::WithPath { path, err } => {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(".#"))
                || is_emacs_lock(err)
        }
        ignore::Error::WithDepth { err, .. } | ignore::Error::WithLineNumber { err, .. } => {
            is_emacs_lock(err)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots(temp: &tempfile::TempDir) -> RootSet {
        RootSet::new(&[temp.path().to_string_lossy().into_owned()], &[]).unwrap()
    }

    #[test]
    fn rejects_parent_traversal() {
        let temp = tempfile::tempdir().unwrap();
        let error = roots(&temp)
            .resolve_read(Path::new("../secret.org"))
            .unwrap_err();
        assert_eq!(error.code().as_str(), "invalid_path");
    }

    #[test]
    fn accepts_exact_absolute_agenda_files_inside_roots() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("tasks.org");
        std::fs::write(&file, "* TODO Safe\n").unwrap();
        let roots = RootSet::new(
            &[temp.path().to_string_lossy().into_owned()],
            &[file.to_string_lossy().into_owned()],
        )
        .unwrap();
        assert_eq!(
            roots.agenda_files().unwrap().files,
            vec![std::fs::canonicalize(file).unwrap()]
        );
    }

    #[cfg(unix)]
    #[test]
    fn emacs_lock_symlinks_do_not_break_discovery() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("tasks.org");
        std::fs::write(&file, "* TODO Edited in Emacs\n").unwrap();
        symlink("user@host.1234:1700000000", temp.path().join(".#tasks.org")).unwrap();
        assert_eq!(
            roots(&temp).agenda_files().unwrap().files,
            vec![std::fs::canonicalize(file).unwrap()]
        );
    }

    #[cfg(unix)]
    #[test]
    fn follows_only_root_contained_read_symlinks_and_refuses_symlink_writes() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target.org");
        std::fs::write(&target, "* TODO Safe\n").unwrap();
        symlink(&target, temp.path().join("link.org")).unwrap();
        let roots = roots(&temp);
        assert_eq!(
            roots.resolve_read(Path::new("link.org")).unwrap(),
            std::fs::canonicalize(target).unwrap()
        );
        let error = roots
            .resolve_write(Path::new("link.org"), false)
            .unwrap_err();
        assert_eq!(error.code().as_str(), "invalid_path");

        let outside = tempfile::tempdir().unwrap();
        let outside_file = outside.path().join("outside.org");
        std::fs::write(&outside_file, "* TODO Secret\n").unwrap();
        symlink(&outside_file, temp.path().join("escape.org")).unwrap();
        let error = roots.resolve_read(Path::new("escape.org")).unwrap_err();
        assert_eq!(error.code().as_str(), "invalid_path");
    }

    #[cfg(unix)]
    #[test]
    fn discovery_skips_and_reports_symlinks_that_leave_the_roots() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let kept = temp.path().join("tasks.org");
        std::fs::write(&kept, "* TODO Kept\n").unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.org"), "* TODO Secret\n").unwrap();
        symlink(
            outside.path().join("secret.org"),
            temp.path().join("escape.org"),
        )
        .unwrap();
        symlink(outside.path(), temp.path().join("elsewhere")).unwrap();
        symlink(
            outside.path().join("missing.org"),
            temp.path().join("dangling.org"),
        )
        .unwrap();

        let root = std::fs::canonicalize(temp.path()).unwrap();
        for found in [
            roots(&temp).agenda_files().unwrap(),
            roots(&temp).all_org_files().unwrap(),
        ] {
            assert_eq!(found.files, vec![std::fs::canonicalize(&kept).unwrap()]);
            let skipped: Vec<(&Path, &str)> = found
                .skipped
                .iter()
                .map(|(path, reason)| (path.strip_prefix(&root).unwrap(), reason.code()))
                .collect();
            assert_eq!(
                skipped,
                [
                    (Path::new("dangling.org"), "walk_failed"),
                    (Path::new("elsewhere"), "invalid_path"),
                    (Path::new("escape.org"), "invalid_path"),
                ]
            );
        }
    }

    #[test]
    fn outside_paths_are_outside_roots_whether_or_not_they_exist() {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("real.org"), "* TODO Secret\n").unwrap();
        let roots = roots(&temp);
        for path in [
            outside.path().join("real.org"),
            outside.path().join("missing.org"),
            PathBuf::from("/no/such/file.org"),
        ] {
            let error = roots.resolve_read(&path).unwrap_err();
            assert_eq!(error.code().as_str(), "invalid_path", "{}", path.display());
        }
        let error = roots
            .resolve_read(&temp.path().join("missing.org"))
            .unwrap_err();
        assert_eq!(error.code().as_str(), "file_not_found");
    }
}
