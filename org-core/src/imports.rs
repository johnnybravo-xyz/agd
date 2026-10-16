use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{AgdError, CaptureTemplate, SORTING_KEYS, Settings, ViewBlock, ViewSettings};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportStatus {
    Exact,
    Difference,
    Unsupported,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImportDiagnostic {
    pub status: ImportStatus,
    pub key: String,
    pub message: String,
}

/// One capture template as the editor reports it; disabled templates are
/// reported, not imported.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImportedCapture {
    pub key: String,
    pub desc: String,
    pub file: Option<String>,
    pub heading: Option<String>,
    pub outline_path: Option<Vec<String>>,
    pub id: Option<String>,
    #[serde(default)]
    pub datetree: bool,
    pub template: String,
    pub template_file: Option<String>,
    pub enabled: bool,
    #[serde(default)]
    pub diagnostics: Vec<String>,
}

/// One custom agenda command as the editor reports it. Enabled commands
/// become views; `diagnostics` name what was left out.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImportedCommand {
    pub key: String,
    pub description: String,
    pub enabled: bool,
    #[serde(default)]
    pub blocks: Vec<ViewBlock>,
    #[serde(default)]
    pub diagnostics: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedEditorConfig {
    pub editor: String,
    #[serde(default)]
    pub editor_version: String,
    /// org-directory when the config sets it; it becomes the root on a first
    /// import.
    #[serde(default)]
    pub org_directory: Option<String>,
    /// Agenda entries as written in the editor config, made absolute: files,
    /// directories or globs.
    pub agenda_files: Vec<String>,
    pub todo_sequences: Vec<Vec<String>>,
    pub tag_alist: Vec<String>,
    pub persistent_tags: Vec<String>,
    pub tags_column: i64,
    pub tag_inheritance_exclusions: Vec<String>,
    pub highest_priority: String,
    pub lowest_priority: String,
    pub default_priority: String,
    pub deadline_warning_days: i64,
    pub agenda_span: String,
    pub agenda_start_day: Option<String>,
    pub week_start: String,
    pub skip_scheduled_if_done: bool,
    pub skip_deadline_if_done: bool,
    pub skip_timestamp_if_done: bool,
    pub sorting_strategy: Vec<String>,
    pub log_done: String,
    pub log_repeat: String,
    pub log_reschedule: String,
    pub log_redeadline: String,
    pub log_into_drawer: bool,
    /// In the editor's order: as written for Emacs; by key for nvim-orgmode,
    /// whose Lua table keeps no order. config.toml keeps this order.
    #[serde(default)]
    pub capture: Vec<ImportedCapture>,
    #[serde(default)]
    pub commands: Vec<ImportedCommand>,
    #[serde(default)]
    pub diagnostics: Vec<ImportDiagnostic>,
    #[serde(default)]
    pub verification: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigEditor {
    Emacs,
    Neovim,
}

impl ConfigEditor {
    pub fn name(self) -> &'static str {
        match self {
            Self::Emacs => "Emacs",
            Self::Neovim => "Neovim",
        }
    }
}

/// The editor config files an import reads, in load order, and what was
/// passed over.
#[derive(Clone, Debug)]
pub struct ConfigSources {
    pub editor: ConfigEditor,
    pub files: Vec<PathBuf>,
    pub notes: Vec<String>,
}

const MAX_DEPTH: usize = 4;
const MAX_FILES: usize = 300;
const MAX_BYTES: u64 = 1 << 20;
/// Package managers' and editors' own state: never the user's settings.
const SKIPPED_DIRECTORIES: [&str; 12] = [
    ".git",
    ".local",
    ".cache",
    "elpa",
    "straight",
    "eln-cache",
    "var",
    "auto-save-list",
    "pack",
    "plugged",
    "lazy",
    "node_modules",
];
/// Files read first, in this order, as the editors load them.
const ENTRY_POINTS: [&str; 7] = [
    "early-init.el",
    "init.el",
    "config.el",
    ".emacs",
    ".spacemacs",
    "config.org",
    "init.lua",
];

/// Finds the files to read under `paths`. A file is taken as given; a
/// directory is walked (respecting .gitignore, at most MAX_DEPTH levels,
/// MAX_FILES files and MAX_BYTES each, skipping SKIPPED_DIRECTORIES), keeping
/// Emacs Lisp files that mention `org-`, Lua files that mention `orgmode`
/// and Org files with emacs-lisp source blocks. Both editors' files under
/// one path is an error: import one editor at a time.
pub fn find_config_sources(paths: &[PathBuf]) -> Result<ConfigSources, AgdError> {
    let mut files = Vec::new();
    let mut notes = Vec::new();
    for path in paths {
        let path = PathBuf::from(shellexpand::tilde(&path.to_string_lossy()).into_owned());
        if path.is_file() {
            if editor_of(&path).is_none() {
                return Err(import_error(format!(
                    "{} is not an Emacs Lisp (.el, .org) or Lua (.lua) file",
                    path.display()
                )));
            }
            files.push(path);
        } else if path.is_dir() {
            walk_config_directory(&path, &mut files, &mut notes)?;
        } else {
            return Err(import_error(format!("{} does not exist", path.display())));
        }
    }
    let editors: Vec<ConfigEditor> = files.iter().filter_map(|file| editor_of(file)).collect();
    let Some(&editor) = editors.first() else {
        return Err(import_error(
            "found no org settings: no .el file mentioning org-, .lua file mentioning orgmode, or config.org",
        ));
    };
    if editors.iter().any(|other| *other != editor) {
        return Err(import_error(
            "found both Emacs and Neovim files; import one editor's config at a time",
        ));
    }
    files.sort_by_key(|file| {
        let name = file
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        let rank = ENTRY_POINTS
            .iter()
            .position(|entry| Some(*entry) == name.as_deref())
            .unwrap_or(ENTRY_POINTS.len());
        (rank, file.clone())
    });
    files.dedup();
    Ok(ConfigSources {
        editor,
        files,
        notes,
    })
}

fn walk_config_directory(
    root: &Path,
    files: &mut Vec<PathBuf>,
    notes: &mut Vec<String>,
) -> Result<(), AgdError> {
    let walker = ignore::WalkBuilder::new(root)
        .hidden(false)
        .max_depth(Some(MAX_DEPTH))
        .filter_entry(|entry| {
            !(entry.file_type().is_some_and(|kind| kind.is_dir())
                && SKIPPED_DIRECTORIES.contains(&entry.file_name().to_string_lossy().as_ref()))
        })
        .build();
    let mut seen = 0;
    for entry in walker {
        let entry = entry
            .map_err(|error| import_error(format!("cannot read {}: {error}", root.display())))?;
        let path = entry.path();
        let Some(editor) = editor_of(path) else {
            continue;
        };
        seen += 1;
        if seen > MAX_FILES {
            notes.push(format!(
                "stopped after {MAX_FILES} files; point at the config file instead"
            ));
            break;
        }
        if entry
            .metadata()
            .is_ok_and(|metadata| metadata.len() > MAX_BYTES)
        {
            notes.push(format!("{}: skipped, larger than 1 MB", path.display()));
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            notes.push(format!("{}: skipped, not UTF-8 text", path.display()));
            continue;
        };
        let relevant = match editor {
            ConfigEditor::Neovim => text.contains("orgmode"),
            ConfigEditor::Emacs if path.extension().is_some_and(|extension| extension == "org") => {
                text.contains("org-") && text.to_lowercase().contains("#+begin_src emacs-lisp")
            }
            ConfigEditor::Emacs => text.contains("org-"),
        };
        if relevant {
            files.push(path.to_path_buf());
        }
    }
    Ok(())
}

/// The editor a config file belongs to, by name: Emacs Lisp, literate Org
/// configs, `.emacs` and `.spacemacs`, or Lua.
fn editor_of(path: &Path) -> Option<ConfigEditor> {
    let name = path.file_name()?.to_string_lossy();
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("el") => Some(ConfigEditor::Emacs),
        Some("lua") => Some(ConfigEditor::Neovim),
        Some("org")
            if ["config.org", "init.org", "emacs.org", "README.org"].contains(&name.as_ref()) =>
        {
            Some(ConfigEditor::Emacs)
        }
        _ if name == ".emacs" || name == ".spacemacs" => Some(ConfigEditor::Emacs),
        _ => None,
    }
}

fn import_error(message: impl Into<String>) -> AgdError {
    AgdError::new("import", message)
}

/// The editor's settings applied over `base`, and what could not be.
#[derive(Clone, Debug)]
pub struct ImportResult {
    pub settings: Settings,
    pub diagnostics: Vec<ImportDiagnostic>,
}

/// Applies resolved editor settings over `base` (the current config). The
/// editor's values replace the keys it reports; agd-only keys (editor,
/// theme, agents, triage) keep their `base` values. On a first import
/// (`derive_roots`) the roots become org-directory, or the directories the
/// agenda entries live in; otherwise `base.roots` stay and entries outside
/// them are left out with a diagnostic.
pub fn apply_import(
    resolved: &ResolvedEditorConfig,
    base: &Settings,
    derive_roots: bool,
) -> Result<ImportResult, AgdError> {
    validate_resolved(resolved)?;
    let mut diagnostics = resolved.diagnostics.clone();
    let mut settings = base.clone();
    let entries: Vec<PathBuf> = resolved
        .agenda_files
        .iter()
        .map(|entry| PathBuf::from(shellexpand::tilde(entry).into_owned()))
        .collect();
    if derive_roots {
        let derived = derived_roots(resolved.org_directory.as_deref(), &entries);
        if !derived.is_empty() {
            settings.roots = derived;
        }
    }
    settings.agenda_files = Vec::new();
    for (entry, original) in entries.iter().zip(&resolved.agenda_files) {
        match agenda_pattern(entry, &settings.roots) {
            Some(pattern) => settings.agenda_files.push(pattern),
            None => diagnostics.push(ImportDiagnostic {
                status: ImportStatus::Unsupported,
                key: original.clone(),
                message: "agenda entry left out: outside the roots".to_string(),
            }),
        }
    }
    settings.agenda_files.dedup();

    // Each sequence as its #+TODO: line; (type …) keywords read the same.
    settings.todo_keywords = resolved
        .todo_sequences
        .iter()
        .map(|sequence| sequence.join(" "))
        .collect();
    settings.tag_alist = resolved.tag_alist.clone();
    settings.persistent_tags = resolved.persistent_tags.clone();
    settings.tags_column = resolved.tags_column;
    settings.agenda_span = resolved.agenda_span.clone();
    settings.agenda_start_day = resolved.agenda_start_day.clone();
    settings.week_start = resolved.week_start.clone();
    settings.deadline_warning_days = resolved.deadline_warning_days;
    settings.skip_scheduled_if_done = resolved.skip_scheduled_if_done;
    settings.skip_deadline_if_done = resolved.skip_deadline_if_done;
    settings.skip_timestamp_if_done = resolved.skip_timestamp_if_done;
    settings.sorting_strategy.clear();
    for key in &resolved.sorting_strategy {
        if SORTING_KEYS.contains(&key.as_str()) {
            settings.sorting_strategy.push(key.clone());
        } else {
            diagnostics.push(ImportDiagnostic {
                status: ImportStatus::Difference,
                key: "sorting_strategy".to_string(),
                message: format!("sorting key {key} is not supported and was left out"),
            });
        }
    }
    settings.highest_priority = resolved.highest_priority.clone();
    settings.lowest_priority = resolved.lowest_priority.clone();
    settings.default_priority = resolved.default_priority.clone();
    settings.tag_inheritance_exclusions = resolved.tag_inheritance_exclusions.clone();
    settings.log_done = resolved.log_done.clone();
    settings.log_repeat = resolved.log_repeat.clone();
    settings.log_reschedule = resolved.log_reschedule.clone();
    settings.log_redeadline = resolved.log_redeadline.clone();
    settings.log_into_drawer = resolved.log_into_drawer;

    settings.captures.clear();
    for capture in &resolved.capture {
        if !capture.enabled {
            diagnostics.push(ImportDiagnostic {
                status: ImportStatus::Unsupported,
                key: format!("capture {}", capture.key),
                message: capture.diagnostics.join("; "),
            });
            continue;
        }
        settings.captures.push(CaptureTemplate {
            key: capture.key.clone(),
            desc: capture.desc.clone(),
            file: capture.file.clone(),
            heading: capture.heading.clone(),
            outline_path: capture.outline_path.clone(),
            id: capture.id.clone(),
            datetree: capture.datetree,
            template: capture.template.clone(),
            template_file: capture.template_file.clone(),
        });
    }
    // The defaults stay only when the editor defines no templates at all.
    if resolved.capture.is_empty() {
        settings.captures = base.captures.clone();
    }

    settings.views.clear();
    for command in &resolved.commands {
        let key = format!("view {}", command.key);
        let unusable_match = command
            .blocks
            .iter()
            .find_map(|block| crate::agenda_engine::validate_view_block(block).err());
        if let Some(error) = &unusable_match {
            diagnostics.push(ImportDiagnostic {
                status: ImportStatus::Unsupported,
                key: key.clone(),
                message: error.to_string(),
            });
        }
        if command.enabled && unusable_match.is_none() {
            settings.views.push(ViewSettings {
                key: command.key.clone(),
                desc: command.description.clone(),
                blocks: command.blocks.clone(),
            });
        }
        let status = if command.enabled {
            ImportStatus::Difference
        } else {
            ImportStatus::Unsupported
        };
        for message in &command.diagnostics {
            diagnostics.push(ImportDiagnostic {
                status: status.clone(),
                key: key.clone(),
                message: message.clone(),
            });
        }
    }
    if resolved.commands.is_empty() {
        settings.views = base.views.clone();
    }

    Ok(ImportResult {
        settings,
        diagnostics,
    })
}

/// The literal directory an agenda entry starts from: a directory itself, a
/// file's parent, or a glob's components before its first wildcard.
fn entry_base(entry: &Path) -> PathBuf {
    let mut base = PathBuf::new();
    for component in entry.components() {
        if component
            .as_os_str()
            .to_string_lossy()
            .contains(['*', '?', '['])
        {
            return base;
        }
        base.push(component);
    }
    if entry.is_dir() {
        base
    } else {
        base.parent().map(Path::to_path_buf).unwrap_or(base)
    }
}

/// org-directory if set, else each agenda entry's directory; nested
/// directories collapse into their ancestor. Home-relative paths are written
/// with `~`.
fn derived_roots(org_directory: Option<&str>, entries: &[PathBuf]) -> Vec<String> {
    let mut directories: Vec<PathBuf> = match org_directory {
        Some(directory) => vec![PathBuf::from(shellexpand::tilde(directory).into_owned())],
        None => entries.iter().map(|entry| entry_base(entry)).collect(),
    };
    directories.retain(|directory| directory.is_dir());
    directories.sort();
    directories.dedup();
    let mut roots: Vec<PathBuf> = Vec::new();
    for directory in directories {
        if !roots.iter().any(|root| directory.starts_with(root)) {
            roots.push(directory);
        }
    }
    let home = dirs::home_dir();
    roots
        .into_iter()
        .map(|root| {
            match home
                .as_deref()
                .and_then(|home| root.strip_prefix(home).ok())
            {
                Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
                Some(rest) => format!("~/{}", rest.to_string_lossy()),
                None => root.to_string_lossy().into_owned(),
            }
        })
        .collect()
}

/// An agenda entry as a pattern relative to the root that holds it: a file
/// stays a file, a directory becomes its `*.org` files (Emacs does not
/// recurse), and a glob keeps its wildcards. None when no root holds it.
fn agenda_pattern(entry: &Path, roots: &[String]) -> Option<String> {
    let base = entry_base(entry);
    let root = roots
        .iter()
        .map(|root| PathBuf::from(shellexpand::tilde(root).into_owned()))
        .filter(|root| base.starts_with(root))
        .max_by_key(|root| root.components().count())?;
    let relative = entry
        .strip_prefix(&root)
        .ok()?
        .to_string_lossy()
        .into_owned();
    if entry.is_dir() {
        Some(if relative.is_empty() {
            "*.org".to_string()
        } else {
            format!("{relative}/*.org")
        })
    } else {
        Some(relative)
    }
}

fn validate_resolved(resolved: &ResolvedEditorConfig) -> Result<(), AgdError> {
    if resolved.todo_sequences.is_empty()
        || resolved
            .todo_sequences
            .iter()
            .any(|sequence| sequence.is_empty())
    {
        return Err(AgdError::new(
            "unsupported_import_shape",
            "editor returned no usable TODO sequence",
        ));
    }
    for (name, value) in [
        ("highest priority", &resolved.highest_priority),
        ("lowest priority", &resolved.lowest_priority),
        ("default priority", &resolved.default_priority),
    ] {
        if value.chars().count() != 1 {
            return Err(AgdError::new(
                "unsupported_import_shape",
                format!("{name} must be one character"),
            ));
        }
    }
    if resolved.week_start != "monday" && resolved.week_start != "sunday" {
        return Err(AgdError::new(
            "unsupported_import_shape",
            format!("unsupported week start: {}", resolved.week_start),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn discovery_reads_entry_points_first_and_only_org_mentions() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::write(root.join("config.el"), "(setq org-log-done 'time)\n").unwrap();
        fs::write(root.join("init.el"), "(setq org-directory \"~/org\")\n").unwrap();
        fs::write(root.join("keys.el"), "(map! :n \"x\" #'ignore)\n").unwrap();
        fs::create_dir_all(root.join(".local/straight")).unwrap();
        fs::write(root.join(".local/straight/org.el"), "(setq org-x 1)\n").unwrap();
        let sources = find_config_sources(&[root.to_path_buf()]).unwrap();
        assert_eq!(sources.editor, ConfigEditor::Emacs);
        assert_eq!(
            sources.files,
            vec![root.join("init.el"), root.join("config.el")]
        );

        fs::write(root.join("init.lua"), "require('orgmode').setup({})\n").unwrap();
        let error = find_config_sources(&[root.to_path_buf()]).unwrap_err();
        assert!(
            error.to_string().contains("both Emacs and Neovim"),
            "{error}"
        );
    }

    #[test]
    fn first_import_derives_roots_and_relative_patterns() {
        let temp = tempfile::tempdir().unwrap();
        let org = temp.path().join("org");
        fs::create_dir_all(org.join("projects")).unwrap();
        let entries = [
            org.join("projects"),
            org.join("**/*.org"),
            org.join("inbox.org"),
        ];
        let roots = derived_roots(None, &entries);
        assert_eq!(roots, vec![org.to_string_lossy().into_owned()]);
        let patterns: Vec<Option<String>> = entries
            .iter()
            .map(|entry| agenda_pattern(entry, &roots))
            .collect();
        assert_eq!(
            patterns,
            vec![
                Some("projects/*.org".to_string()),
                Some("**/*.org".to_string()),
                Some("inbox.org".to_string())
            ]
        );
        assert_eq!(
            agenda_pattern(&temp.path().join("elsewhere.org"), &roots),
            None
        );
    }

    #[test]
    fn reimport_keeps_roots_and_agd_only_keys_and_drops_outside_entries() {
        let temp = tempfile::tempdir().unwrap();
        let base = Settings {
            roots: vec![temp.path().to_string_lossy().into_owned()],
            theme: "dark".to_string(),
            ..Settings::default()
        };
        let resolved: ResolvedEditorConfig = serde_json::from_value(serde_json::json!({
            "editor": "emacs",
            "org_directory": "/elsewhere",
            "agenda_files": [temp.path().join("inbox.org"), "/elsewhere/private.org"],
            "todo_sequences": [["TODO", "|", "DONE"]],
            "tag_alist": [], "persistent_tags": [], "tags_column": -77,
            "tag_inheritance_exclusions": [],
            "highest_priority": "A", "lowest_priority": "C", "default_priority": "B",
            "deadline_warning_days": 14, "agenda_span": "week", "agenda_start_day": null,
            "week_start": "monday", "skip_scheduled_if_done": false,
            "skip_deadline_if_done": false, "skip_timestamp_if_done": false,
            "sorting_strategy": ["time-up"], "log_done": "time", "log_repeat": "time",
            "log_reschedule": "nil", "log_redeadline": "nil", "log_into_drawer": false
        }))
        .unwrap();
        let result = apply_import(&resolved, &base, false).unwrap();
        assert_eq!(result.settings.roots, base.roots);
        assert_eq!(result.settings.theme, "dark");
        assert_eq!(result.settings.agenda_files, vec!["inbox.org"]);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.key == "/elsewhere/private.org")
        );
        assert_eq!(result.settings.captures, base.captures);
    }
}
