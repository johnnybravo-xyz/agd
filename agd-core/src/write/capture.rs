//! `WriteEngine::capture`: a capture through a template, from the target file
//! (a configured file, or the file of an `id` target, inside the roots) to the
//! rendered entry inserted and journaled; and the agent inbox gate, which sends
//! an agent's capture to `agent_inbox` without dates.
//!
//! The capture is computed once before any directory is created and again
//! under the write lock, so a refused capture leaves nothing behind. Where the
//! entry goes is `placement`'s; the template text is `crate::capture_template`'s.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::NaiveDateTime;

use crate::{
    AgdError, CaptureDraft, CaptureTemplate, ErrorCode, Item, Mutation, Settings, WorkspaceIndex,
    render_capture_template,
};

use super::engine::{
    DRY_RUN_JOURNAL_ID, PreparedFile, WriteContext, WriteEngine, mutation, parse_text,
    parse_updated_item,
};
use super::file::{read_org_file, refuse_emacs_lock};
use super::org_edit::align_tags;
use super::patch::{apply_patches, changed_lines_patch, line_ending};
use super::placement::{heading_line_starts, insert_capture_entry, place_capture};
use super::refile::relevel_subtree;
use super::validation::{refuse_used_id, validate_capture_for_file, validate_capture_input};

/// What a capture renders, gathered so it can be prepared twice: once to
/// validate and preview, once under the write lock.
struct CaptureInput<'a> {
    template: &'a CaptureTemplate,
    draft: &'a CaptureDraft,
    body: &'a str,
    prompts: &'a BTreeMap<String, String>,
    now: NaiveDateTime,
}

struct PreparedCapture {
    file: PreparedFile,
    item: Item,
}

impl WriteEngine {
    /// Captures through a template. A dry run renders the same diff and
    /// touches nothing: no directory, lock, journal entry or file. A real
    /// capture is computed once before any directory is created, so a
    /// refused capture leaves nothing behind either.
    pub fn capture(
        &self,
        template_key: &str,
        draft: &CaptureDraft,
        body: &str,
        prompts: &BTreeMap<String, String>,
        dry_run: bool,
        context: &WriteContext,
    ) -> Result<Mutation, AgdError> {
        let template = self
            .settings
            .capture_template(template_key)
            .ok_or_else(|| {
                let keys: Vec<&str> = self
                    .settings
                    .captures
                    .iter()
                    .map(|t| t.key.as_str())
                    .collect();
                AgdError::new(
                    ErrorCode::UnknownTemplate,
                    format!(
                        "capture template '{template_key}' is not configured; templates: {}",
                        keys.join(", ")
                    ),
                )
            })?;
        let (template, draft) = apply_agent_capture_gate(template, draft, &self.settings, context);
        validate_capture_input(&draft, body, prompts)?;
        let input = CaptureInput {
            template: &template,
            draft: &draft,
            body,
            prompts,
            now: context.org_now(),
        };
        let path = self.capture_path(&template)?;
        let preview = self.prepare_capture(&path, &input)?;
        refuse_emacs_lock(&path)?;
        if dry_run {
            return Ok(mutation(
                preview.item,
                preview.file.diff,
                DRY_RUN_JOURNAL_ID,
            ));
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(AgdError::from)?;
        }
        self.roots.resolve_write(&path, true)?;
        let _locks = self.lock_paths(std::slice::from_ref(&path))?;
        let prepared = self.prepare_capture(&path, &input)?;
        let record = self.commit_image(
            prepared.file.journal_image(Some(&prepared.item.item_ref)),
            context,
        )?;
        Ok(mutation(prepared.item, prepared.file.diff, &record.id))
    }

    /// The canonical path a template captures into, confined to the roots,
    /// whether or not the file or its directories exist yet. An `id` target
    /// is the file of the one heading with that ID.
    fn capture_path(&self, template: &CaptureTemplate) -> Result<PathBuf, AgdError> {
        if let Some(id) = &template.id {
            let index = WorkspaceIndex::load(self.settings.clone())?;
            let (document, _) = index
                .resolve(id)
                .map_err(|error| capture_id_error(&template.key, id, error))?;
            return self.roots.resolve_write(document.path(), false);
        }
        let file = template.file.as_deref().ok_or_else(|| {
            AgdError::new(
                ErrorCode::UnsupportedTarget,
                format!(
                    "capture template '{}' names no file or id target",
                    template.key
                ),
            )
        })?;
        // `~` expands as in roots, before the path is confined to them.
        let file = shellexpand::tilde(file);
        let path = self.roots.resolve_write(Path::new(file.as_ref()), true)?;
        // Canonical like every indexed path, so the journal, refs and the
        // index agree (on macOS /var is /private/var).
        let path = crate::path::resolve_existing_prefix(&path)?;
        self.roots.resolve_write(&path, true)
    }

    /// The capture's insertion into `path` as it is on disk now.
    fn prepare_capture(
        &self,
        path: &Path,
        input: &CaptureInput<'_>,
    ) -> Result<PreparedCapture, AgdError> {
        let pre_image = read_org_file(path)?;
        validate_capture_for_file(input.draft, &parse_text(path, &pre_image, &self.settings)?)?;
        let rendered = render_capture_template(
            input.template,
            input.draft,
            input.body,
            input.prompts,
            input.now,
        )?;
        let newline = line_ending(&pre_image);
        let rendered =
            append_body_without_escape(rendered, &input.template.template, input.body, newline);
        let placement = place_capture(
            path,
            &pre_image,
            input.template,
            input.now.date(),
            &self.settings,
        )?;
        let entry = capture_entry(&rendered, placement.level, newline)?;
        let entry = align_first_line_tags(&entry, self.settings.tags_column);
        let (post_image, entry_start) =
            insert_capture_entry(&placement.text, placement.offset, &entry, newline);
        let patch = changed_lines_patch(&pre_image, &post_image);
        let (post_image, stored) = apply_patches(&pre_image, vec![patch])?;
        let line = post_image[..entry_start]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1;
        let item = parse_updated_item(path, &post_image, line, &self.settings)?;
        if let Some(id) = &item.id {
            refuse_used_id(id, &WorkspaceIndex::load(self.settings.clone())?)?;
        }
        Ok(PreparedCapture {
            file: PreparedFile::new(path.to_path_buf(), pre_image, post_image, stored),
            item,
        })
    }
}

/// A body given without a `%i` in the template becomes the entry's body,
/// after the template's text and any properties drawer, instead of being
/// dropped.
fn append_body_without_escape(
    rendered: String,
    template: &str,
    body: &str,
    newline: &str,
) -> String {
    let body = body.trim_end_matches(['\n', '\r']);
    if body.is_empty() || template_inserts_body(template) {
        return rendered;
    }
    let mut entry = rendered.trim_end_matches(['\n', '\r']).to_string();
    entry.push_str(newline);
    entry.push_str(body);
    entry
}

/// Whether the template has a `%i` escape (`%%i` is a literal `%i`).
fn template_inserts_body(template: &str) -> bool {
    let mut characters = template.chars();
    while let Some(character) = characters.next() {
        if character == '%' && characters.next() == Some('i') {
            return true;
        }
    }
    false
}

/// The entry's headline with its tags aligned to `tags_column`, as an edit
/// writes a headline.
fn align_first_line_tags(entry: &str, column: i64) -> String {
    let line_end = entry.find(['\r', '\n']).unwrap_or(entry.len());
    let (headline, rest) = entry.split_at(line_end);
    let Some((head, tags)) = crate::document::split_trailing_tags(headline) else {
        return entry.to_string();
    };
    format!(
        "{}{rest}",
        align_tags(format!("{head} {tags}"), true, column)
    )
}

/// The rendered template as an entry at `level`: every heading in it moved
/// by the same number of levels (org-paste-subtree), a template without a
/// leading heading made one (`* `), ending in a newline.
fn capture_entry(rendered: &str, level: usize, newline: &str) -> Result<String, AgdError> {
    let rendered = if heading_line_starts(rendered, (0, 1)).is_empty() {
        format!("* {rendered}")
    } else {
        rendered.to_string()
    };
    let first_level = rendered.bytes().take_while(|byte| *byte == b'*').count();
    let mut entry = relevel_subtree(&rendered, level, first_level)?;
    if !entry.ends_with('\n') {
        entry.push_str(newline);
    }
    Ok(entry)
}

fn capture_id_error(template_key: &str, id: &str, error: AgdError) -> AgdError {
    if error.code() == ErrorCode::DuplicateId.as_str() {
        return error;
    }
    AgdError::new(
        ErrorCode::CaptureTargetNotFound,
        format!("capture template '{template_key}' targets ID '{id}', which no agenda heading has"),
    )
}

// With agent_inbox set, an agent capture always lands in that template
// without planning, so it reaches the day only through triage.
fn apply_agent_capture_gate(
    template: &CaptureTemplate,
    draft: &CaptureDraft,
    settings: &Settings,
    context: &WriteContext,
) -> (CaptureTemplate, CaptureDraft) {
    let inbox_key = match settings.agent_inbox.as_deref() {
        Some(key) if context.is_agent() => key,
        _ => return (template.clone(), draft.clone()),
    };
    let inbox = settings
        .capture_template(inbox_key)
        .cloned()
        .unwrap_or_else(|| template.clone());
    let mut draft = draft.clone();
    draft.scheduled = None;
    draft.deadline = None;
    (inbox, draft)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::write::test_support::{context, settings, tree};
    use crate::{PlanningTimestamp, WriteEngine};

    #[test]
    fn capture_journal_records_the_new_item_ref() {
        let temp = tempfile::tempdir().unwrap();
        let mut settings = settings(temp.path());
        settings.captures[0].file =
            Some(temp.path().join("inbox.org").to_string_lossy().into_owned());
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl")).unwrap();
        let draft = CaptureDraft {
            title: "New".to_string(),
            ..CaptureDraft::default()
        };
        let mutation = engine
            .capture("t", &draft, "", &BTreeMap::new(), false, &context())
            .unwrap();
        let record = engine
            .journal()
            .unwrap()
            .into_iter()
            .find(|record| record.id == mutation.journal_id)
            .unwrap();
        assert_eq!(
            record.item_ref.as_deref(),
            Some(mutation.item.item_ref.as_str())
        );
        assert_eq!(
            Path::new(&record.file),
            fs::canonicalize(temp.path().join("inbox.org")).unwrap()
        );
    }

    #[test]
    fn a_refused_capture_creates_no_directory() {
        let org = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let mut settings = settings(org.path());
        settings.captures[0].file = Some("new/inbox.org".to_string());
        settings.captures[0].template = "* TODO %?\n%^{Missing}".to_string();
        let engine = WriteEngine::new(settings, data.path().join("journal.jsonl")).unwrap();
        let draft = CaptureDraft {
            title: "Refused".to_string(),
            ..CaptureDraft::default()
        };
        let error = engine
            .capture("t", &draft, "", &BTreeMap::new(), false, &context())
            .unwrap_err();
        assert_eq!(error.code(), "missing_prompt");
        assert!(tree(org.path()).is_empty());
        assert!(tree(data.path()).is_empty());
    }

    #[test]
    fn a_body_is_appended_when_the_template_has_no_escape_for_it() {
        let temp = tempfile::tempdir().unwrap();
        let mut settings = settings(temp.path());
        settings.captures = vec![
            CaptureTemplate {
                key: "t".to_string(),
                file: Some("inbox.org".to_string()),
                heading: None,
                template: "* TODO %?\n:PROPERTIES:\n:SOURCE: agent\n:END:".to_string(),
                ..CaptureTemplate::default()
            },
            CaptureTemplate {
                key: "i".to_string(),
                file: Some("inbox.org".to_string()),
                heading: None,
                template: "* TODO %?\nContext: %i\nLiteral 100%%i".to_string(),
                ..CaptureTemplate::default()
            },
        ];
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl")).unwrap();
        let draft = CaptureDraft {
            title: "Follow up".to_string(),
            ..CaptureDraft::default()
        };
        engine
            .capture(
                "t",
                &draft,
                "Line one\nLine two\n",
                &BTreeMap::new(),
                false,
                &context(),
            )
            .unwrap();
        engine
            .capture("i", &draft, "inline", &BTreeMap::new(), false, &context())
            .unwrap();
        assert_eq!(
            fs::read_to_string(temp.path().join("inbox.org")).unwrap(),
            "* TODO Follow up\n:PROPERTIES:\n:SOURCE: agent\n:END:\nLine one\nLine two\n* TODO Follow up\nContext: inline\nLiteral 100%i\n"
        );
    }

    #[test]
    fn captured_tags_align_to_the_tags_column_like_edits() {
        let temp = tempfile::tempdir().unwrap();
        let mut settings = settings(temp.path());
        settings.tags_column = -30;
        settings.captures[0].file = Some("inbox.org".to_string());
        settings.captures[0].heading = None;
        settings.captures[0].template = "* TODO %?".to_string();
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl")).unwrap();
        let draft = CaptureDraft {
            title: "Renew".to_string(),
            tags: vec!["admin".to_string()],
            ..CaptureDraft::default()
        };
        engine
            .capture("t", &draft, "", &BTreeMap::new(), false, &context())
            .unwrap();
        let written = fs::read_to_string(temp.path().join("inbox.org")).unwrap();
        assert_eq!(
            written,
            format!("* TODO Renew{}:admin:\n", " ".repeat(30 - 12 - 7))
        );
    }

    #[test]
    fn inbox_gate_redirects_only_agent_captures_and_drops_their_planning() {
        let temp = tempfile::tempdir().unwrap();
        let mut settings = settings(temp.path());
        settings.agent_inbox = Some("t".to_string());
        settings.captures.push(CaptureTemplate {
            key: "w".to_string(),
            file: Some("work.org".to_string()),
            heading: None,
            template: "* TODO %?".to_string(),
            ..CaptureTemplate::default()
        });
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
            properties: BTreeMap::new(),
        };
        let agent = WriteContext {
            actor: "mcp:claude-code".to_string(),
            project: "fixture".to_string(),
            now: None,
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
            properties: BTreeMap::new(),
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
}
