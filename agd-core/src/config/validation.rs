//! Checks a configuration: the roots exist and differ, then every value in a
//! fixed order (`validate_values`); the first check that fails is the error.
//!
//! Each failure is an `invalid_config` error (`invalid_root` for a root)
//! that names the key and the accepted
//! values. A view's match is checked by the agenda's own parser, so a view
//! that loads is a view that runs. Roots are checked apart from the values so
//! `agd setup import` can check a config before its roots exist.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::{AgdError, ErrorCode};

use super::file::config_error;
use super::model::{
    AgentProfile, COLOR_NAMES, CaptureTemplate, SORTING_KEYS, Settings, VIEW_BLOCK_KINDS,
    VIEW_GROUP_BY, ViewBlock, ViewGroup, ViewSettings, parse_agenda_day,
};

pub(super) fn validate_settings(settings: &Settings) -> Result<(), AgdError> {
    validate_roots(&settings.roots)?;
    validate_values(settings)
}

/// Every root resolves to a distinct directory; a bad root is the same
/// `invalid_root` error the write engine gives.
pub(super) fn validate_roots(roots: &[String]) -> Result<(), AgdError> {
    let root_error = |message: String| AgdError::new(ErrorCode::InvalidRoot, message);
    if roots.is_empty() {
        return Err(root_error(
            "roots must contain at least one directory".to_string(),
        ));
    }
    let mut canonical_roots = BTreeSet::new();
    for root in roots {
        let expanded = shellexpand::tilde(root).into_owned();
        let canonical = PathBuf::from(&expanded)
            .canonicalize()
            .map_err(|error| root_error(format!("cannot resolve root '{root}': {error}")))?;
        if !canonical.is_dir() {
            return Err(root_error(format!("root '{root}' is not a directory")));
        }
        if !canonical_roots.insert(canonical) {
            return Err(root_error(format!("root '{root}' is listed twice")));
        }
    }
    Ok(())
}

/// Every check but the roots' existence, so an import can check the config
/// it writes before its roots exist. The checks run in this order and the
/// first one that fails is the error.
pub(crate) fn validate_values(settings: &Settings) -> Result<(), AgdError> {
    validate_todo_keywords(&settings.todo_keywords)?;
    validate_priorities(settings)?;
    validate_agenda_days(settings)?;
    validate_editor(&settings.editor)?;
    validate_agent_inbox(settings)?;
    validate_theme(&settings.theme)?;
    validate_sorting_strategy(&settings.sorting_strategy)?;
    validate_keyword_colors(&settings.todo_keyword_colors)?;
    validate_views(&settings.views)?;
    validate_start_view(settings)?;
    validate_captures(&settings.captures)?;
    validate_agent_profiles(&settings.agent_profiles)
}

/// Each `todo_keywords` line needs a keyword and at most one `|`.
fn validate_todo_keywords(lines: &[String]) -> Result<(), AgdError> {
    for line in lines {
        let words: Vec<&str> = line.split_whitespace().collect();
        let separators = words.iter().filter(|word| **word == "|").count();
        if words.len() == separators {
            return Err(config_error(format!(
                "todo_keywords line \"{line}\" has no keywords; write one like \"TODO | DONE\""
            )));
        }
        if separators > 1 {
            return Err(config_error(format!(
                "todo_keywords line \"{line}\" has more than one |; put every done keyword after one |"
            )));
        }
    }
    Ok(())
}

/// A priority cookie's value: Org takes the letters A–Z, or the numbers
/// 0–64 when org-priority-highest is below ?A.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
enum PriorityValue {
    Letter(char),
    Number(u8),
}

fn priority_value(value: &str) -> Option<PriorityValue> {
    let mut chars = value.chars();
    if let (Some(letter @ 'A'..='Z'), None) = (chars.next(), chars.next()) {
        return Some(PriorityValue::Letter(letter));
    }
    value
        .parse::<u8>()
        .ok()
        .filter(|number| *number <= 64 && number.to_string() == value)
        .map(PriorityValue::Number)
}

/// The three priorities are all letters or all numbers, and run from the
/// highest (A, or the smallest number) through the default to the lowest.
fn validate_priorities(settings: &Settings) -> Result<(), AgdError> {
    let mut values = Vec::new();
    for (key, value) in [
        ("highest_priority", &settings.highest_priority),
        ("default_priority", &settings.default_priority),
        ("lowest_priority", &settings.lowest_priority),
    ] {
        values.push(priority_value(value).ok_or_else(|| {
            config_error(format!(
                "{key} must be a letter A-Z or a number 0-64, not \"{value}\""
            ))
        })?);
    }
    let [highest, default, lowest] = [values[0], values[1], values[2]];
    let kind = std::mem::discriminant;
    if kind(&highest) != kind(&default) || kind(&default) != kind(&lowest) {
        return Err(config_error(
            "highest_priority, default_priority and lowest_priority must be all letters or all numbers",
        ));
    }
    if highest > default || default > lowest {
        return Err(config_error(format!(
            "priorities must run highest, default, lowest, as A, B, C or 1, 2, 3; not {}, {}, {}",
            settings.highest_priority, settings.default_priority, settings.lowest_priority
        )));
    }
    Ok(())
}

/// agenda_start_day parses and deadline_warning_days is not negative.
fn validate_agenda_days(settings: &Settings) -> Result<(), AgdError> {
    if let Some(value) = &settings.agenda_start_day
        && parse_agenda_day(value, chrono::Local::now().date_naive()).is_err()
    {
        return Err(config_error(format!(
            "agenda_start_day \"{value}\" is not a day; use a date such as 2026-10-13 or an offset such as -1d or +2w"
        )));
    }
    if settings.deadline_warning_days < 0 {
        return Err(config_error(format!(
            "deadline_warning_days must be 0 or more, not {}",
            settings.deadline_warning_days
        )));
    }
    Ok(())
}

fn validate_editor(editor: &[String]) -> Result<(), AgdError> {
    if editor.is_empty() {
        return Err(config_error("editor needs at least the program to run"));
    }
    Ok(())
}

/// `agent_inbox`, when set, is a `[[capture]]` key.
fn validate_agent_inbox(settings: &Settings) -> Result<(), AgdError> {
    if let Some(key) = &settings.agent_inbox
        && settings.capture_template(key).is_none()
    {
        let keys: Vec<&str> = settings
            .captures
            .iter()
            .map(|template| template.key.as_str())
            .collect();
        return Err(config_error(format!(
            "agent_inbox names capture template '{key}', which is not a [[capture]] key; use one of {}",
            keys.join(", ")
        )));
    }
    Ok(())
}

fn validate_theme(theme: &str) -> Result<(), AgdError> {
    if !["auto", "dark", "light"].contains(&theme) {
        return Err(config_error(format!(
            "theme must be auto, dark or light, not '{theme}'"
        )));
    }
    Ok(())
}

/// Every key is one of SORTING_KEYS.
fn validate_sorting_strategy(keys: &[String]) -> Result<(), AgdError> {
    for key in keys {
        if !SORTING_KEYS.contains(&key.as_str()) {
            return Err(config_error(format!(
                "sorting_strategy key '{key}' is not supported; use {}",
                SORTING_KEYS.join(", ")
            )));
        }
    }
    Ok(())
}

/// Every keyword's color is one of COLOR_NAMES.
fn validate_keyword_colors(colors: &BTreeMap<String, String>) -> Result<(), AgdError> {
    for (keyword, color) in colors {
        if !COLOR_NAMES.contains(&color.as_str()) {
            return Err(config_error(format!(
                "todo_keyword_colors gives {keyword} '{color}'; use one of {}",
                COLOR_NAMES.join(", ")
            )));
        }
    }
    Ok(())
}

/// View keys are unique and every view has at least one block, each valid.
fn validate_views(views: &[ViewSettings]) -> Result<(), AgdError> {
    let mut view_keys = BTreeSet::new();
    for view in views {
        if !view_keys.insert(view.key.as_str()) {
            return Err(config_error(format!("duplicate view key '{}'", view.key)));
        }
        if view.blocks.is_empty() {
            return Err(config_error(format!(
                "view '{}' needs at least one block",
                view.key
            )));
        }
        for block in &view.blocks {
            validate_block(&view.key, block)?;
        }
    }
    Ok(())
}

/// A view block's kind, its match as the agenda will read it, its colors
/// and its groups.
fn validate_block(view_key: &str, block: &ViewBlock) -> Result<(), AgdError> {
    if !VIEW_BLOCK_KINDS.contains(&block.kind.as_str()) {
        return Err(config_error(format!(
            "view '{view_key}' has unsupported block kind '{}'; use one of {}",
            block.kind,
            VIEW_BLOCK_KINDS.join(", ")
        )));
    }
    crate::agenda::validate_view_block(block)
        .map_err(|error| config_error(format!("view '{view_key}': {error}")))?;
    let colors = std::iter::once(&block.color).chain(block.groups.iter().map(|group| &group.color));
    for color in colors.flatten() {
        if !COLOR_NAMES.contains(&color.as_str()) {
            return Err(config_error(format!(
                "view '{view_key}' has color '{color}'; use one of {}",
                COLOR_NAMES.join(", ")
            )));
        }
    }
    for group in &block.groups {
        validate_group(view_key, group)?;
    }
    Ok(())
}

/// A `by` group names a known grouping and takes nothing that filters
/// rows; every effort bound is a duration.
fn validate_group(view_key: &str, group: &ViewGroup) -> Result<(), AgdError> {
    if let Some(by) = &group.by {
        if !VIEW_GROUP_BY.contains(&by.as_str()) {
            return Err(config_error(format!(
                "view '{view_key}' groups by '{by}'; use one of {}",
                VIEW_GROUP_BY.join(", ")
            )));
        }
        if group.match_expression.is_some()
            || group.discard
            || group.effort_max.is_some()
            || group.effort_min.is_some()
        {
            return Err(config_error(format!(
                "view '{view_key}': a group with by takes no match, effort or discard"
            )));
        }
    }
    for effort in [&group.effort_max, &group.effort_min].into_iter().flatten() {
        if crate::agenda::duration_minutes(effort).is_none() {
            return Err(config_error(format!(
                "view '{view_key}': effort '{effort}' is not a duration such as 0:15, 30 or 1h"
            )));
        }
    }
    Ok(())
}

/// `start_view`, when set, names a view.
fn validate_start_view(settings: &Settings) -> Result<(), AgdError> {
    if !settings.start_view.is_empty() && settings.view(&settings.start_view).is_none() {
        let keys: Vec<&str> = settings
            .all_views()
            .iter()
            .map(|view| view.key.as_str())
            .collect();
        return Err(config_error(format!(
            "start_view '{}' is not a view; use one of {}, or remove start_view to open Today",
            settings.start_view,
            keys.join(", ")
        )));
    }
    Ok(())
}

/// Template keys are unique words and every template has one text source
/// and one of org-capture's target shapes.
fn validate_captures(captures: &[CaptureTemplate]) -> Result<(), AgdError> {
    let mut capture_keys = BTreeSet::new();
    for template in captures {
        let key = &template.key;
        if key.is_empty() || key.chars().any(char::is_whitespace) {
            return Err(config_error(format!(
                "invalid capture template key '{key}'"
            )));
        }
        if !capture_keys.insert(key.as_str()) {
            return Err(config_error(format!(
                "duplicate capture template key '{key}'"
            )));
        }
        validate_capture_shape(template)
            .map_err(|detail| config_error(format!("capture template '{key}' {detail}")))?;
    }
    Ok(())
}

/// A template has one text source and one of org-capture's target shapes:
/// id; file; file+headline; file+olp; file+datetree; file+olp+datetree.
fn validate_capture_shape(template: &CaptureTemplate) -> Result<(), &'static str> {
    match (
        template.template.is_empty(),
        template.template_file.is_some(),
    ) {
        (true, false) => return Err("needs template or template_file"),
        (false, true) => return Err("takes template or template_file, not both"),
        _ => {}
    }
    let heading = template.heading.is_some();
    let outline = template.outline_path.is_some();
    match (template.file.is_some(), template.id.is_some()) {
        (false, false) => Err("needs a target: file or id"),
        (true, true) => Err("targets a file or an id, not both"),
        (false, true) if heading || outline || template.datetree => {
            Err("targets an id, which takes no heading, outline_path or datetree")
        }
        (true, false) if heading && outline => Err("takes heading or outline_path, not both"),
        (true, false) if heading && template.datetree => Err(
            "puts a datetree under the file or an outline_path, not a heading, as Org's file+datetree and file+olp+datetree do",
        ),
        _ => Ok(()),
    }
}

/// Agent profile names are unique.
fn validate_agent_profiles(profiles: &[AgentProfile]) -> Result<(), AgdError> {
    let mut profile_names = BTreeSet::new();
    for profile in profiles {
        if !profile_names.insert(profile.name.as_str()) {
            return Err(config_error(format!(
                "duplicate agent_profile name '{}'",
                profile.name
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use crate::config::test_support::{load_body, load_error};

    #[test]
    fn unknown_start_view_says_how_to_open_today() {
        assert_eq!(
            load_error("start_view = \"d\"\n"),
            "start_view 'd' is not a view; use one of n, or remove start_view to open Today"
        );
    }

    fn capture_error(fields: &str) -> String {
        load_error(&format!(
            "[[capture]]\nkey = \"j\"\ndesc = \"Journal\"\n{fields}"
        ))
    }

    #[test]
    fn capture_templates_take_org_capture_target_shapes() {
        for fields in [
            "id = \"abc\"\ntemplate = \"* %?\"\n",
            "file = \"j.org\"\ntemplate = \"* %?\"\n",
            "file = \"j.org\"\nheading = \"Log\"\ntemplate = \"* %?\"\n",
            "file = \"j.org\"\noutline_path = [\"A\", \"B\"]\ntemplate = \"* %?\"\n",
            "file = \"j.org\"\ndatetree = true\ntemplate = \"* %?\"\n",
            "file = \"j.org\"\noutline_path = [\"A\"]\ndatetree = true\ntemplate = \"* %?\"\n",
        ] {
            let body = format!("[[capture]]\nkey = \"j\"\ndesc = \"Journal\"\n{fields}");
            assert!(load_body(&body).is_ok(), "{fields}");
        }
        let cases = [
            ("file = \"j.org\"\n", "needs template or template_file"),
            (
                "file = \"j.org\"\ntemplate = \"* %?\"\ntemplate_file = \"t.txt\"\n",
                "takes template or template_file, not both",
            ),
            ("template = \"* %?\"\n", "needs a target: file or id"),
            (
                "file = \"j.org\"\nid = \"abc\"\ntemplate = \"* %?\"\n",
                "targets a file or an id, not both",
            ),
            (
                "id = \"abc\"\nheading = \"Log\"\ntemplate = \"* %?\"\n",
                "targets an id, which takes no heading, outline_path or datetree",
            ),
            (
                "file = \"j.org\"\nheading = \"Log\"\noutline_path = [\"Log\"]\ntemplate = \"* %?\"\n",
                "takes heading or outline_path, not both",
            ),
            (
                "file = \"j.org\"\nheading = \"Log\"\ndatetree = true\ntemplate = \"* %?\"\n",
                "puts a datetree under the file or an outline_path, not a heading, as Org's file+datetree and file+olp+datetree do",
            ),
        ];
        for (fields, detail) in cases {
            assert_eq!(
                capture_error(fields),
                format!("capture template 'j' {detail}")
            );
        }
    }

    #[test]
    fn agent_inbox_names_a_capture_template() {
        let settings = load_body("agent_inbox = \"t\"\n").unwrap();
        assert_eq!(settings.agent_inbox.as_deref(), Some("t"));
        assert_eq!(load_body("").unwrap().agent_inbox, None);
        assert_eq!(
            load_error("agent_inbox = \"i\"\n"),
            "agent_inbox names capture template 'i', which is not a [[capture]] key; use one of t"
        );
    }

    #[test]
    fn priorities_are_letters_or_numbers_in_org_order() {
        let numeric = load_body(
            "highest_priority = \"1\"\ndefault_priority = \"3\"\nlowest_priority = \"64\"\n",
        )
        .unwrap();
        assert_eq!(numeric.lowest_priority, "64");
        assert!(load_body("lowest_priority = \"Z\"\n").is_ok());
        assert_eq!(
            load_error("highest_priority = \"a\"\n"),
            "highest_priority must be a letter A-Z or a number 0-64, not \"a\""
        );
        assert_eq!(
            load_error("lowest_priority = \"65\"\n"),
            "lowest_priority must be a letter A-Z or a number 0-64, not \"65\""
        );
        assert_eq!(
            load_error("lowest_priority = \"5\"\n"),
            "highest_priority, default_priority and lowest_priority must be all letters or all numbers"
        );
        assert_eq!(
            load_error("default_priority = \"D\"\n"),
            "priorities must run highest, default, lowest, as A, B, C or 1, 2, 3; not A, D, C"
        );
    }

    #[test]
    fn agenda_start_day_parses_and_warning_days_are_not_negative() {
        let settings = load_body("agenda_start_day = \"-1d\"\n").unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 10, 16).unwrap();
        assert_eq!(
            settings.agenda_start(today).unwrap(),
            NaiveDate::from_ymd_opt(2026, 10, 15).unwrap()
        );
        assert!(load_body("agenda_start_day = \"2026-10-01\"\n").is_ok());
        assert_eq!(
            load_error("agenda_start_day = \"yesterdayish\"\n"),
            "agenda_start_day \"yesterdayish\" is not a day; use a date such as 2026-10-13 or an offset such as -1d or +2w"
        );
        assert_eq!(
            load_error("deadline_warning_days = -3\n"),
            "deadline_warning_days must be 0 or more, not -3"
        );
        assert!(load_body("deadline_warning_days = 0\n").is_ok());
    }

    #[test]
    fn todo_keyword_lines_reject_lists_and_misplaced_separators() {
        assert_eq!(
            load_error("todo_keywords = [[\"TODO\", \"|\", \"DONE\"]]\n"),
            concat!(
                "cannot parse CONFIG: TOML parse error at line 2, column 17\n",
                "  |\n",
                "2 | todo_keywords = [[\"TODO\", \"|\", \"DONE\"]]\n",
                "  |                 ^^^^^^^^^^^^^^^^^^^^^^^\n",
                "todo_keywords takes one #+TODO: line per sequence; write \"TODO | DONE\"\n",
            )
        );
        assert_eq!(
            load_error("todo_keywords = [\"TODO | DONE | GONE\"]\n"),
            "todo_keywords line \"TODO | DONE | GONE\" has more than one |; put every done keyword after one |"
        );
        assert_eq!(
            load_error("todo_keywords = [\" | \"]\n"),
            "todo_keywords line \" | \" has no keywords; write one like \"TODO | DONE\""
        );
    }
}
