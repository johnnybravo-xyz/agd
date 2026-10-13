use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;

use crate::{AgendumError, FilesSettings};

#[derive(Clone, Debug)]
pub struct RootSet {
    roots: Vec<PathBuf>,
    agenda: Vec<String>,
}

impl RootSet {
    pub fn new(files: &FilesSettings) -> Result<Self, AgendumError> {
        let mut roots = Vec::with_capacity(files.roots.len());
        for configured in &files.roots {
            let expanded = shellexpand::tilde(configured).into_owned();
            let canonical = PathBuf::from(&expanded).canonicalize().map_err(|error| {
                AgendumError::with_source(
                    "invalid_root",
                    format!("cannot resolve approved root '{configured}': {error}"),
                    error,
                )
            })?;
            if !canonical.is_dir() {
                return Err(AgendumError::new(
                    "invalid_root",
                    format!("approved root is not a directory: {configured}"),
                ));
            }
            roots.push(canonical);
        }
        roots.sort();
        roots.dedup();
        if roots.is_empty() {
            return Err(AgendumError::new(
                "invalid_root",
                "at least one approved files.root is required",
            ));
        }
        Ok(Self {
            roots,
            agenda: files.agenda.clone(),
        })
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.roots.iter().any(|root| path.starts_with(root))
    }

    pub fn resolve_read(&self, requested: &Path) -> Result<PathBuf, AgendumError> {
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
            0 => Err(AgendumError::new(
                "not_found",
                format!(
                    "file is not present under an approved root: {}",
                    requested.display()
                ),
            )),
            1 => Ok(matches.into_iter().next().expect("one path")),
            _ => Err(AgendumError::new(
                "ambiguous_path",
                format!(
                    "relative path matches multiple approved roots: {}",
                    requested.display()
                ),
            )),
        }
    }

    pub fn resolve_write(&self, requested: &Path, create: bool) -> Result<PathBuf, AgendumError> {
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
                    return Err(AgendumError::new(
                        "ambiguous_path",
                        "capture into multiple roots requires an absolute target",
                    ));
                }
                [] => {
                    return Err(AgendumError::new(
                        "not_found",
                        format!(
                            "file is not present under an approved root: {}",
                            requested.display()
                        ),
                    ));
                }
                _ => {
                    return Err(AgendumError::new(
                        "ambiguous_path",
                        format!(
                            "relative path matches multiple approved roots: {}",
                            requested.display()
                        ),
                    ));
                }
            }
        };

        if let Ok(metadata) = std::fs::symlink_metadata(&lexical)
            && metadata.file_type().is_symlink()
        {
            return Err(AgendumError::new(
                "symlink_write_refused",
                format!("writes to symlinks are refused: {}", lexical.display()),
            ));
        }

        let parent = lexical.parent().ok_or_else(|| {
            AgendumError::new(
                "invalid_path",
                format!("path has no parent: {}", lexical.display()),
            )
        })?;
        let canonical_parent = nearest_existing(parent)?.canonicalize().map_err(|error| {
            AgendumError::with_source(
                "invalid_path",
                format!("cannot resolve parent of {}: {error}", lexical.display()),
                error,
            )
        })?;
        if !self.contains(&canonical_parent) {
            return Err(AgendumError::new(
                "outside_roots",
                format!("path escapes approved roots: {}", lexical.display()),
            ));
        }
        if lexical.exists() && !lexical.is_file() {
            return Err(AgendumError::new(
                "invalid_path",
                format!("write target is not a regular file: {}", lexical.display()),
            ));
        }
        Ok(lexical)
    }

    pub fn agenda_files(&self) -> Result<Vec<PathBuf>, AgendumError> {
        let patterns = if self.agenda.is_empty() {
            vec!["**/*.org".to_string()]
        } else {
            self.agenda.clone()
        };
        self.org_files_matching(&patterns)
    }

    pub fn all_org_files(&self) -> Result<Vec<PathBuf>, AgendumError> {
        self.org_files_matching(&[String::from("**/*.org")])
    }

    fn org_files_matching(&self, patterns: &[String]) -> Result<Vec<PathBuf>, AgendumError> {
        let mut relative_patterns = Vec::new();
        let mut files = BTreeSet::new();
        for pattern in patterns {
            let path = Path::new(pattern);
            reject_parent_segments(path)?;
            if path.is_absolute() {
                if pattern.contains(['*', '?', '[', ']']) {
                    return Err(AgendumError::new(
                        "invalid_agenda_pattern",
                        format!("absolute agenda entries must be exact files: {pattern}"),
                    ));
                }
                let resolved = self.validate_existing_read(path)?;
                if resolved
                    .extension()
                    .is_none_or(|extension| extension != "org")
                {
                    return Err(AgendumError::new(
                        "invalid_agenda_file",
                        format!("agenda file does not have an .org extension: {pattern}"),
                    ));
                }
                files.insert(resolved);
            } else {
                relative_patterns.push(pattern.clone());
            }
        }
        if relative_patterns.is_empty() {
            return Ok(files.into_iter().collect());
        }

        let matcher = build_globset(&relative_patterns)?;
        for root in &self.roots {
            for entry in WalkBuilder::new(root).follow_links(true).build() {
                let entry = entry.map_err(|error| {
                    AgendumError::new(
                        "walk_error",
                        format!("cannot walk {}: {error}", root.display()),
                    )
                })?;
                if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                    continue;
                }
                let path = entry.path();
                let relative = path.strip_prefix(root).unwrap_or(path);
                if path.extension().is_some_and(|extension| extension == "org")
                    && matcher.is_match(relative)
                {
                    files.insert(self.validate_existing_read(path)?);
                }
            }
        }
        Ok(files.into_iter().collect())
    }

    fn validate_existing_read(&self, path: &Path) -> Result<PathBuf, AgendumError> {
        let canonical = path.canonicalize().map_err(|error| {
            AgendumError::with_source(
                "not_found",
                format!("cannot resolve file {}: {error}", path.display()),
                error,
            )
        })?;
        if !self.contains(&canonical) {
            return Err(AgendumError::new(
                "outside_roots",
                format!("resolved path escapes approved roots: {}", path.display()),
            ));
        }
        if !canonical.is_file() {
            return Err(AgendumError::new(
                "invalid_path",
                format!("path is not a regular file: {}", path.display()),
            ));
        }
        Ok(canonical)
    }
}

fn reject_parent_segments(path: &Path) -> Result<(), AgendumError> {
    if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err(AgendumError::new(
            "path_traversal",
            format!("'..' is not allowed in paths: {}", path.display()),
        ));
    }
    Ok(())
}

fn nearest_existing(path: &Path) -> Result<&Path, AgendumError> {
    let mut current = path;
    loop {
        if current.exists() {
            return Ok(current);
        }
        current = current.parent().ok_or_else(|| {
            AgendumError::new(
                "invalid_path",
                format!("no existing ancestor for {}", path.display()),
            )
        })?;
    }
}

fn build_globset(patterns: &[String]) -> Result<GlobSet, AgendumError> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        reject_parent_segments(Path::new(pattern))?;
        if Path::new(pattern).is_absolute() {
            return Err(AgendumError::new(
                "invalid_agenda_pattern",
                format!("agenda pattern must be relative to an approved root: {pattern}"),
            ));
        }
        builder.add(Glob::new(pattern).map_err(|error| {
            AgendumError::new(
                "invalid_agenda_pattern",
                format!("invalid agenda pattern '{pattern}': {error}"),
            )
        })?);
    }
    builder.build().map_err(|error| {
        AgendumError::new(
            "invalid_agenda_pattern",
            format!("cannot build agenda patterns: {error}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots(temp: &tempfile::TempDir) -> RootSet {
        RootSet::new(&FilesSettings {
            roots: vec![temp.path().to_string_lossy().into_owned()],
            agenda: Vec::new(),
        })
        .unwrap()
    }

    #[test]
    fn rejects_parent_traversal() {
        let temp = tempfile::tempdir().unwrap();
        let error = roots(&temp)
            .resolve_read(Path::new("../secret.org"))
            .unwrap_err();
        assert_eq!(error.code(), "path_traversal");
    }

    #[test]
    fn accepts_exact_absolute_agenda_files_inside_roots() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("tasks.org");
        std::fs::write(&file, "* TODO Safe\n").unwrap();
        let roots = RootSet::new(&FilesSettings {
            roots: vec![temp.path().to_string_lossy().into_owned()],
            agenda: vec![file.to_string_lossy().into_owned()],
        })
        .unwrap();
        assert_eq!(roots.agenda_files().unwrap(), vec![file]);
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
        assert_eq!(roots.resolve_read(Path::new("link.org")).unwrap(), target);
        let error = roots
            .resolve_write(Path::new("link.org"), false)
            .unwrap_err();
        assert_eq!(error.code(), "symlink_write_refused");

        let outside = tempfile::tempdir().unwrap();
        let outside_file = outside.path().join("outside.org");
        std::fs::write(&outside_file, "* TODO Secret\n").unwrap();
        symlink(&outside_file, temp.path().join("escape.org")).unwrap();
        let error = roots.resolve_read(Path::new("escape.org")).unwrap_err();
        assert_eq!(error.code(), "outside_roots");
    }
}
