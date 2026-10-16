use std::collections::BTreeMap;

use crate::{AgdError, CaptureDraft, ErrorCode, HeadlineSpan, IndexedDocument, WorkspaceIndex};

use super::edit::{Change, EditIntent};

/// The edit's fields that need no file: a title, tags and properties
/// that write back as exactly one heading with those fields.
pub(super) fn validate_intent(intent: &EditIntent) -> Result<(), AgdError> {
    if intent == &EditIntent::default() {
        return Err(AgdError::new(ErrorCode::InvalidEdit, "nothing to edit"));
    }
    if let Change::Set(title) = &intent.title {
        validate_title_text(title)?;
    }
    for tag in intent.tags_add.iter().chain(&intent.tags_remove) {
        validate_tag(tag)?;
    }
    for (key, value) in &intent.properties {
        validate_property(key, value.as_deref())?;
    }
    Ok(())
}

/// An `ID` is identity: an existing one may not change or go, and a new
/// one must not be in use anywhere agd indexes.
pub(super) fn validate_id_change(
    intent: &EditIntent,
    headline: &HeadlineSpan,
    index: &WorkspaceIndex,
) -> Result<(), AgdError> {
    let Some((_, requested)) = intent
        .properties
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("ID"))
    else {
        return Ok(());
    };
    match (&headline.item.id, requested) {
        (Some(current), Some(requested)) if current == requested => Ok(()),
        (Some(current), _) => Err(AgdError::new(
            ErrorCode::IdImmutable,
            format!(
                "IDs are identity: links and refs point at ID '{current}', so it cannot be changed or removed"
            ),
        )),
        (None, Some(requested)) => refuse_used_id(requested, index),
        (None, None) => Ok(()),
    }
}

/// Refuses an ID some heading under the roots already has.
pub(super) fn refuse_used_id(id: &str, index: &WorkspaceIndex) -> Result<(), AgdError> {
    if index.id_in_use(id) {
        return Err(AgdError::new(
            ErrorCode::DuplicateId,
            format!("ID '{id}' is already used under the roots; IDs must be unique"),
        ));
    }
    Ok(())
}

/// The edit's fields checked against the item's file: a title must not
/// start with one of its TODO keywords, a priority must be in its range.
pub(super) fn validate_intent_for_file(
    intent: &EditIntent,
    document: &IndexedDocument,
) -> Result<(), AgdError> {
    if let Change::Set(title) = &intent.title {
        validate_title_keyword(title, document)?;
    }
    if let Change::Set(priority) = &intent.priority {
        validate_priority(priority, document)?;
    }
    Ok(())
}

/// A capture's fields that need no file, checked before anything is read.
pub(super) fn validate_capture_input(
    draft: &CaptureDraft,
    body: &str,
    prompts: &BTreeMap<String, String>,
) -> Result<(), AgdError> {
    validate_title_text(&draft.title)?;
    for tag in &draft.tags {
        validate_tag(tag)?;
    }
    for (key, value) in &draft.properties {
        validate_property(key, Some(value))?;
    }
    for (name, answer) in prompts {
        if answer.contains(['\n', '\r']) {
            return Err(AgdError::new(
                ErrorCode::InvalidPrompt,
                format!("the answer to prompt '{name}' must be one line"),
            ));
        }
    }
    validate_body(body)
}

/// A capture's fields checked against the target file (or, for a new
/// file, the configuration).
pub(super) fn validate_capture_for_file(
    draft: &CaptureDraft,
    document: &IndexedDocument,
) -> Result<(), AgdError> {
    validate_title_keyword(&draft.title, document)?;
    if let Some(priority) = &draft.priority {
        validate_priority(priority, document)?;
    }
    Ok(())
}

/// One non-empty line that the parser reads back as the whole title: no
/// leading priority cookie and no trailing tag group.
fn validate_title_text(title: &str) -> Result<(), AgdError> {
    if title.trim().is_empty() || title.contains(['\n', '\r']) {
        return Err(AgdError::new(
            ErrorCode::InvalidTitle,
            "title must be one non-empty line",
        ));
    }
    if let Some(cookie) = crate::document::leading_priority_cookie(title.trim_start()) {
        return Err(AgdError::new(
            ErrorCode::InvalidTitle,
            format!("title starts with the priority cookie {cookie}; set the priority instead"),
        ));
    }
    if let Some((_, tags)) = crate::document::split_trailing_tags(title) {
        return Err(AgdError::new(
            ErrorCode::InvalidTitle,
            format!("title ends with the tag group {tags}; give tags separately"),
        ));
    }
    Ok(())
}

/// A title starting with one of the file's TODO keywords would be read
/// back as that state.
fn validate_title_keyword(title: &str, document: &IndexedDocument) -> Result<(), AgdError> {
    let first = title.split_whitespace().next().unwrap_or_default();
    if document.todo_keyword(first).is_some() {
        return Err(AgdError::new(
            ErrorCode::InvalidTitle,
            format!("title starts with the TODO keyword {first}; set the state instead"),
        ));
    }
    Ok(())
}

/// Org's tag characters: letters, digits, `_`, `@`, `#` and `%`.
fn validate_tag(tag: &str) -> Result<(), AgdError> {
    if tag.is_empty()
        || !tag
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '@' | '#' | '%'))
    {
        return Err(AgdError::new(
            ErrorCode::InvalidTag,
            format!("invalid tag '{tag}': use letters, digits, _, @, # or %"),
        ));
    }
    Ok(())
}

/// One cookie character within the file's `#+PRIORITIES` range (else the
/// configured one).
fn validate_priority(priority: &str, document: &IndexedDocument) -> Result<(), AgdError> {
    let range = document.priorities();
    if !range.contains(priority) {
        return Err(AgdError::new(
            ErrorCode::InvalidPriority,
            format!(
                "priority '{priority}' is outside the range {} of {}",
                range.describe(),
                document.path().display()
            ),
        ));
    }
    Ok(())
}

/// A property key of `[A-Za-z0-9_-]` and a one-line value.
fn validate_property(key: &str, value: Option<&str>) -> Result<(), AgdError> {
    if key.is_empty()
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(AgdError::new(
            ErrorCode::InvalidProperty,
            format!("invalid property key '{key}': use letters, digits, _ or -"),
        ));
    }
    if value.is_some_and(|value| value.contains(['\n', '\r'])) {
        return Err(AgdError::new(
            ErrorCode::InvalidProperty,
            format!("property {key} must be one line"),
        ));
    }
    Ok(())
}

/// A body line that would start a heading (`*` then a space) would split
/// the entry in two.
fn validate_body(body: &str) -> Result<(), AgdError> {
    for (index, line) in body.lines().enumerate() {
        let stars = line.bytes().take_while(|byte| *byte == b'*').count();
        if stars > 0 && line[stars..].starts_with(char::is_whitespace) {
            return Err(AgdError::new(
                ErrorCode::InvalidBody,
                format!("body line {} would start a heading: {line}", index + 1),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;

    use super::*;
    use crate::write::test_support::{context, settings};
    use crate::{Mutation, WriteEngine};

    #[test]
    fn capture_refuses_fields_that_would_write_other_org_syntax() {
        let temp = tempfile::tempdir().unwrap();
        let inbox = temp.path().join("inbox.org");
        let original = "#+PRIORITIES: A E C\n* Inbox\n";
        fs::write(&inbox, original).unwrap();
        let mut settings = settings(temp.path());
        settings.captures[0].file = Some("inbox.org".to_string());
        settings.captures[0].template = "* TODO %?\n%^{Where}".to_string();
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl")).unwrap();
        let answer = BTreeMap::from([("Where".to_string(), "desk".to_string())]);
        let capture = |draft: CaptureDraft, body: &str, prompts: &BTreeMap<String, String>| {
            engine
                .capture("t", &draft, body, prompts, false, &context())
                .map(|_| ())
                .map_err(|error| (error.code().to_string(), error.to_string()))
        };
        let titled = |title: &str| CaptureDraft {
            title: title.to_string(),
            ..CaptureDraft::default()
        };
        let refusals = [
            (
                titled("Innocent\n* DONE Injected\n:PROPERTIES:\n:ID: abc-123\n:END:"),
                "",
                answer.clone(),
                "invalid_title",
            ),
            (titled("DONE sneaky"), "", answer.clone(), "invalid_title"),
            (
                titled("[#A] Cookie first"),
                "",
                answer.clone(),
                "invalid_title",
            ),
            (titled("Tagged :a:b:"), "", answer.clone(), "invalid_title"),
            (
                CaptureDraft {
                    tags: vec!["bad tag".to_string()],
                    ..titled("Task")
                },
                "",
                answer.clone(),
                "invalid_tag",
            ),
            (
                CaptureDraft {
                    priority: Some("F".to_string()),
                    ..titled("Task")
                },
                "",
                answer.clone(),
                "invalid_priority",
            ),
            (
                CaptureDraft {
                    properties: BTreeMap::from([("X\n#+TODO".to_string(), "y".to_string())]),
                    ..titled("Task")
                },
                "",
                answer.clone(),
                "invalid_property",
            ),
            (
                CaptureDraft {
                    properties: BTreeMap::from([("SOURCE".to_string(), "a\nb".to_string())]),
                    ..titled("Task")
                },
                "",
                answer.clone(),
                "invalid_property",
            ),
            (
                titled("Task"),
                "fine\n* DONE Injected",
                answer.clone(),
                "invalid_body",
            ),
            (
                titled("Task"),
                "",
                BTreeMap::from([("Where".to_string(), "desk\n* DONE x".to_string())]),
                "invalid_prompt",
            ),
        ];
        for (draft, body, prompts, code) in refusals {
            let (actual, message) = capture(draft.clone(), body, &prompts).unwrap_err();
            assert_eq!(actual, code, "{draft:?} {body:?}: {message}");
        }
        let (_, message) = capture(
            CaptureDraft {
                priority: Some("F".to_string()),
                ..titled("Task")
            },
            "",
            &answer,
        )
        .unwrap_err();
        assert!(message.contains("A–E"), "{message}");
        let (_, message) = capture(titled("Task"), "fine\n* DONE Injected", &answer).unwrap_err();
        assert!(message.contains("line 2"), "{message}");
        assert_eq!(fs::read_to_string(&inbox).unwrap(), original);
        assert!(engine.journal().unwrap().is_empty());

        // The file's own range admits E.
        capture(
            CaptureDraft {
                priority: Some("E".to_string()),
                ..titled("Low but fine")
            },
            "",
            &answer,
        )
        .unwrap();
    }

    #[test]
    fn edit_refuses_titles_tags_priorities_and_properties_that_would_change_the_syntax() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tasks.org");
        let original = "* TODO Task\n";
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
        let refusals = [
            (
                EditIntent {
                    title: Change::Set("DONE sneaky :x:".to_string()),
                    ..EditIntent::default()
                },
                "invalid_title",
            ),
            (
                EditIntent {
                    title: Change::Set("NEXT step".to_string()),
                    ..EditIntent::default()
                },
                "invalid_title",
            ),
            (
                EditIntent {
                    priority: Change::Set("D".to_string()),
                    ..EditIntent::default()
                },
                "invalid_priority",
            ),
            (
                EditIntent {
                    tags_add: BTreeSet::from(["a:b".to_string()]),
                    ..EditIntent::default()
                },
                "invalid_tag",
            ),
            (
                EditIntent {
                    properties: BTreeMap::from([("A B".to_string(), Some("x".to_string()))]),
                    ..EditIntent::default()
                },
                "invalid_property",
            ),
        ];
        for (intent, code) in refusals {
            for dry_run in [true, false] {
                let error = engine
                    .edit(&item_ref, &intent, dry_run, &context())
                    .unwrap_err();
                assert_eq!(error.code(), code, "{intent:?}");
            }
        }
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        assert!(engine.journal().unwrap().is_empty());
    }

    #[test]
    fn ids_must_be_unique_and_never_change_once_set() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tasks.org");
        let original = "* TODO Known\n:PROPERTIES:\n:ID: known-id\n:END:\n* TODO Fresh\n";
        fs::write(&path, original).unwrap();
        std::fs::create_dir(temp.path().join("archive")).unwrap();
        fs::write(
            temp.path().join("archive/old.org"),
            "* Old\n:PROPERTIES:\n:ID: archived-id\n:END:\n",
        )
        .unwrap();
        let mut settings = settings(temp.path());
        settings.agenda_files = vec!["tasks.org".to_string()];
        settings.captures[0].file = Some("tasks.org".to_string());
        settings.captures[0].heading = None;
        let fresh = WorkspaceIndex::load(settings.clone())
            .unwrap()
            .items()
            .find(|item| item.title == "Fresh")
            .unwrap()
            .item_ref
            .clone();
        let engine = WriteEngine::new(settings, temp.path().join("journal.jsonl")).unwrap();
        let set_id = |value: Option<&str>| EditIntent {
            properties: BTreeMap::from([("ID".to_string(), value.map(str::to_string))]),
            ..EditIntent::default()
        };
        let code = |result: Result<Mutation, AgdError>| result.unwrap_err().code().to_string();

        assert_eq!(
            code(engine.edit("known-id", &set_id(Some("other")), false, &context())),
            "id_immutable"
        );
        assert_eq!(
            code(engine.edit("known-id", &set_id(None), false, &context())),
            "id_immutable"
        );
        let message = engine
            .edit("known-id", &set_id(None), false, &context())
            .unwrap_err()
            .to_string();
        assert!(message.contains("identity"), "{message}");
        for used in ["known-id", "archived-id"] {
            assert_eq!(
                code(engine.edit(&fresh, &set_id(Some(used)), false, &context())),
                "duplicate_id"
            );
            let draft = CaptureDraft {
                title: "Copy".to_string(),
                properties: BTreeMap::from([("ID".to_string(), used.to_string())]),
                ..CaptureDraft::default()
            };
            assert_eq!(
                code(engine.capture("t", &draft, "", &BTreeMap::new(), false, &context())),
                "duplicate_id"
            );
        }
        assert_eq!(fs::read_to_string(&path).unwrap(), original);

        let added = engine
            .edit(&fresh, &set_id(Some("fresh-id")), false, &context())
            .unwrap();
        assert_eq!(added.item.id.as_deref(), Some("fresh-id"));
        let draft = CaptureDraft {
            title: "New with ID".to_string(),
            properties: BTreeMap::from([("ID".to_string(), "new-id".to_string())]),
            ..CaptureDraft::default()
        };
        let captured = engine
            .capture("t", &draft, "", &BTreeMap::new(), false, &context())
            .unwrap();
        assert_eq!(captured.item.item_ref, "new-id");
    }
}
