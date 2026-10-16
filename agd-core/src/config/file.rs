//! `config.toml` on disk: where it lives, reading it into `Settings`
//! (`load_settings`, `parse_config_file`), explaining TOML errors such as a
//! top-level key written below a record, reading each `template_file`, and
//! writing settings back as a commented file (`render_config`).
//!
//! Loading validates before it returns, so a caller never holds settings that
//! fail validation. Rendering leaves out values equal to the defaults, so the
//! file shows only what differs. `CONFIG_GROUPS` is the one list of top-level
//! keys, shared by both directions.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::{AgdError, AgendaSpan, ErrorCode};

use super::model::{AgentProfile, CaptureTemplate, Settings, ViewGroup, ViewSettings};
use super::validation::{validate_roots, validate_settings};

#[derive(Clone, Debug)]
pub struct LoadedSettings {
    pub settings: Settings,
    pub config_path: PathBuf,
}

pub fn default_config_path() -> Result<PathBuf, AgdError> {
    Ok(crate::path::config_dir()?.join("config.toml"))
}

pub fn load_settings(
    path: Option<&Path>,
    root_override: Option<&Path>,
) -> Result<LoadedSettings, AgdError> {
    let config_path = path
        .map(Path::to_path_buf)
        .unwrap_or(default_config_path()?);
    let mut settings = if config_path.exists() {
        parse_config_file(&config_path)?
    } else if path.is_some() {
        return Err(config_error(format!(
            "configuration file does not exist: {}",
            config_path.display()
        )));
    } else {
        Settings::default()
    };
    if let Some(root) = root_override {
        settings.roots = vec![root.to_string_lossy().into_owned()];
    } else if !config_path.exists() && validate_roots(&settings.roots).is_err() {
        return Err(first_run_error(&config_path, &settings.roots));
    }
    validate_settings(&settings)?;
    read_template_files(&mut settings.captures, &settings.roots)?;
    Ok(LoadedSettings {
        settings,
        config_path,
    })
}

/// With neither a config file nor the default root there is nothing to
/// read, so the error names both and the ways to start.
fn first_run_error(config_path: &Path, roots: &[String]) -> AgdError {
    let root = roots.join(", ");
    config_error(format!(
        "no config at {} and no {root} to read. Start with one of:\n  mkdir {root}    agd then reads it with its defaults\n  agd setup import <your Emacs or Neovim config directory>\n  agd -r DIR    read another directory for this run",
        config_path.display()
    ))
}

/// Parses a config file without validating roots, so `agd setup import` can
/// read one whose roots do not exist yet.
pub fn parse_config_file(path: &Path) -> Result<Settings, AgdError> {
    let source = fs::read_to_string(path).map_err(|error| {
        AgdError::with_source(
            ErrorCode::Io,
            format!("cannot read {}: {error}", path.display()),
            error,
        )
    })?;
    toml::from_str(&source).map_err(|error| {
        config_error(format!(
            "cannot parse {}: {}",
            path.display(),
            explain_parse_error(&error)
        ))
    })
}

/// The TOML error with its location. TOML puts every key below a
/// `[[capture]]`, `[[view]]` or `[[agent_profile]]` line into that record,
/// so a top-level setting written there is explained instead of reported as
/// an unknown record field.
fn explain_parse_error(error: &toml::de::Error) -> String {
    let text = error.to_string();
    let field = error
        .message()
        .strip_prefix("unknown field `")
        .and_then(|rest| rest.split('`').next());
    match field {
        Some(field) if is_top_level_key(field) => text.replace(
            error.message(),
            &format!(
                "`{field}` is a top-level setting; move it above the first [[capture]], [[view]] or [[agent_profile]] record"
            ),
        ),
        _ => text,
    }
}

fn is_top_level_key(name: &str) -> bool {
    CONFIG_GROUPS.iter().any(|(_, keys)| keys.contains(&name))
}

pub(super) fn config_error(message: impl Into<String>) -> AgdError {
    AgdError::new(ErrorCode::Config, message)
}

/// Reads each `template_file` into its template's `template`, so a capture
/// reads no file but its target. Relative paths are under `roots[0]`.
fn read_template_files(captures: &mut [CaptureTemplate], roots: &[String]) -> Result<(), AgdError> {
    for template in captures {
        let Some(file) = &template.template_file else {
            continue;
        };
        let path = PathBuf::from(shellexpand::tilde(file).into_owned());
        let path = match roots.first() {
            Some(root) if path.is_relative() => {
                PathBuf::from(shellexpand::tilde(root).into_owned()).join(path)
            }
            _ => path,
        };
        template.template = fs::read_to_string(&path).map_err(|error| {
            AgdError::with_source(
                ErrorCode::Config,
                format!(
                    "capture template '{}': cannot read template_file {}: {error}",
                    template.key,
                    path.display()
                ),
                error,
            )
        })?;
    }
    Ok(())
}

/// Top-level keys in the order `render_config` writes them, under these
/// comments.
const CONFIG_GROUPS: [(&str, &[&str]); 6] = [
    ("Files agd may read and write", &["roots", "agenda_files"]),
    (
        "Org syntax, as in your editor",
        &[
            "todo_keywords",
            "todo_keyword_colors",
            "ids",
            "tags_column",
            "tag_inheritance_exclusions",
            "highest_priority",
            "lowest_priority",
            "default_priority",
        ],
    ),
    (
        "Agenda",
        &[
            "agenda_span",
            "agenda_start_day",
            "week_start",
            "deadline_warning_days",
            "skip_scheduled_if_done",
            "skip_deadline_if_done",
            "skip_timestamp_if_done",
            "sorting_strategy",
        ],
    ),
    (
        "Logging; #+STARTUP options in a file win",
        &[
            "log_done",
            "log_repeat",
            "log_reschedule",
            "log_redeadline",
            "log_into_drawer",
        ],
    ),
    (
        "Editor and terminal",
        &["editor", "theme", "nerd_font", "start_view"],
    ),
    (
        "Agents and triage",
        &[
            "agent_inbox",
            "someday_tag",
            "someday_clears_scheduled",
            "reschedule_nudge",
        ],
    ),
];

/// Writes `settings` as a config file: grouped, commented top-level keys,
/// then `[[capture]]`, `[[view]]` and `[[agent_profile]]` records. Values
/// equal to the defaults are left out, so the file shows only what differs.
pub fn render_config(settings: &Settings, header: &str) -> Result<String, AgdError> {
    let encode = |value: &Settings| {
        toml::Value::try_from(value)
            .map_err(|error| config_error(format!("cannot encode settings: {error}")))
    };
    let current = encode(settings)?;
    let defaults = encode(&Settings::default())?;
    let (Some(current), Some(defaults)) = (current.as_table(), defaults.as_table()) else {
        return Err(config_error("settings did not encode as a table"));
    };
    let mut output = String::new();
    for line in header.lines() {
        output.push_str(&format!("# {line}\n").replace("# \n", "#\n"));
    }
    for (comment, keys) in CONFIG_GROUPS {
        let lines: Vec<String> = keys
            .iter()
            .filter_map(|key| {
                let value = current.get(*key)?;
                (defaults.get(*key) != Some(value)).then(|| render_key(key, value))
            })
            .collect();
        if !lines.is_empty() {
            output.push_str(&format!("\n# {comment}\n"));
            output.extend(lines);
        }
    }
    // Records are encoded from the structs, not the sorted table above, so
    // each keeps its declared field order with `key` first.
    let defaults = Settings::default();
    let records = Records {
        capture: (settings.captures != defaults.captures).then_some(&settings.captures),
        agent_profile: (!settings.agent_profiles.is_empty()).then_some(&settings.agent_profiles),
    };
    let records = toml::to_string_pretty(&records)
        .map_err(|error| config_error(format!("cannot encode records: {error}")))?;
    if !records.is_empty() {
        output.push('\n');
        output.push_str(&records);
    }
    if settings.views != defaults.views {
        for view in &settings.views {
            output.push_str(&render_view(view));
        }
    }
    Ok(output)
}

/// A `[[view]]` record with its blocks as one inline table per line, so the
/// file has no nested `[[view.blocks]]` tables.
fn render_view(view: &ViewSettings) -> String {
    let quote = |value: &str| toml::Value::String(value.to_string()).to_string();
    let blocks: String = view
        .blocks
        .iter()
        .map(|block| {
            let mut fields = vec![format!("kind = {}", quote(&block.kind))];
            if let Some(expression) = &block.match_expression {
                fields.push(format!("match = {}", quote(expression)));
            }
            match block.span {
                Some(AgendaSpan::Days(days)) => fields.push(format!("span = {days}")),
                Some(span) => fields.push(format!("span = {}", quote(&span.to_string()))),
                None => {}
            }
            for (name, value) in [("header", &block.header), ("color", &block.color)] {
                if let Some(value) = value {
                    fields.push(format!("{name} = {}", quote(value)));
                }
            }
            if block.groups.is_empty() {
                return format!("  {{ {} }},\n", fields.join(", "));
            }
            let groups: String = block
                .groups
                .iter()
                .map(|group| format!("    {{ {} }},\n", render_group(group).join(", ")))
                .collect();
            format!("  {{ {}, groups = [\n{groups}  ] }},\n", fields.join(", "))
        })
        .collect();
    format!(
        "\n[[view]]\nkey = {}\ndesc = {}\nblocks = [\n{blocks}]\n",
        quote(&view.key),
        quote(&view.desc)
    )
}

fn render_group(group: &ViewGroup) -> Vec<String> {
    let quote = |value: &str| toml::Value::String(value.to_string()).to_string();
    let mut fields = Vec::new();
    for (name, value) in [
        ("name", &group.name),
        ("match", &group.match_expression),
        ("by", &group.by),
        ("effort_max", &group.effort_max),
        ("effort_min", &group.effort_min),
        ("color", &group.color),
    ] {
        if let Some(value) = value {
            fields.push(format!("{name} = {}", quote(value)));
        }
    }
    if group.discard {
        fields.push("discard = true".to_string());
    }
    if group.order != 0 {
        fields.push(format!("order = {}", group.order));
    }
    fields
}

#[derive(Serialize)]
struct Records<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    capture: Option<&'a Vec<CaptureTemplate>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_profile: Option<&'a Vec<AgentProfile>>,
}

/// `key = value`, with a list that does not fit in 80 columns written one
/// item per line.
fn render_key(key: &str, value: &toml::Value) -> String {
    let line = format!("{key} = {value}\n");
    match value.as_array() {
        Some(items) if line.len() > 80 && !items.is_empty() => {
            let items: String = items.iter().map(|item| format!("  {item},\n")).collect();
            format!("{key} = [\n{items}]\n")
        }
        _ => line,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::ViewBlock;
    use crate::config::model::strings;
    use crate::config::test_support::{load_body, load_error};

    #[test]
    fn flat_keys_and_records_load_and_unknown_keys_are_errors() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("config.toml");
        fs::write(
            &config,
            format!(
                concat!(
                    "roots = [{root}]\n",
                    "deadline_warning_days = 9\n",
                    "todo_keywords = [\"TODO | DONE\"]\n",
                    "[[capture]]\n",
                    "key = \"n\"\n",
                    "desc = \"Note\"\n",
                    "file = \"notes.org\"\n",
                    "template = \"* %?\"\n",
                    "[[view]]\n",
                    "key = \"w\"\n",
                    "desc = \"Work\"\n",
                    "blocks = [{{ kind = \"tags-todo\", match = \"work\" }}]\n",
                ),
                root = toml::Value::String(temp.path().to_string_lossy().into_owned())
            ),
        )
        .unwrap();
        let loaded = load_settings(Some(&config), None).unwrap();
        assert_eq!(loaded.settings.deadline_warning_days, 9);
        assert_eq!(loaded.settings.captures.len(), 1);
        assert_eq!(loaded.settings.capture_template("n").unwrap().desc, "Note");
        assert_eq!(
            loaded.settings.view("w").unwrap().blocks[0].kind,
            "tags-todo"
        );

        fs::write(
            &config,
            format!(
                "roots = [{}]\n[agenda]\nspan = \"day\"\n",
                toml::Value::String(temp.path().to_string_lossy().into_owned())
            ),
        )
        .unwrap();
        let error = load_settings(Some(&config), None).unwrap_err();
        assert!(error.to_string().contains("agenda"), "{error}");
    }

    #[test]
    fn rendered_config_round_trips_and_omits_defaults() {
        let settings = Settings {
            roots: strings(&["~/notes"]),
            deadline_warning_days: 5,
            todo_keyword_colors: BTreeMap::from([
                ("CANCELLED".to_string(), "dim-red".to_string()),
                ("WAIT".to_string(), "yellow".to_string()),
            ]),
            captures: vec![CaptureTemplate {
                key: "j".to_string(),
                desc: "Journal".to_string(),
                file: Some("journal.org".to_string()),
                heading: None,
                outline_path: None,
                id: None,
                datetree: true,
                template: "* %?".to_string(),
                template_file: None,
            }],
            ..Settings::default()
        };
        let rendered = render_config(&settings, "written by a test").unwrap();
        assert!(rendered.starts_with("# written by a test\n"), "{rendered}");
        assert!(rendered.contains("deadline_warning_days = 5"), "{rendered}");
        assert!(
            rendered
                .contains("todo_keyword_colors = { CANCELLED = \"dim-red\", WAIT = \"yellow\" }\n"),
            "{rendered}"
        );
        assert!(!rendered.contains("agenda_span"), "{rendered}");
        assert!(!rendered.contains("[[view]]"), "{rendered}");
        let parsed: Settings = toml::from_str(&rendered).unwrap();
        assert_eq!(parsed, settings);
    }

    #[test]
    fn rendered_view_groups_round_trip() {
        let settings = Settings {
            views: vec![ViewSettings {
                key: "d".to_string(),
                desc: "Day".to_string(),
                blocks: vec![ViewBlock {
                    kind: "alltodo".to_string(),
                    match_expression: None,
                    span: None,
                    header: Some("Backlog".to_string()),
                    color: Some("orange".to_string()),
                    groups: vec![
                        ViewGroup {
                            match_expression: Some("TODO=\"HOLD\"".to_string()),
                            discard: true,
                            ..ViewGroup::default()
                        },
                        ViewGroup {
                            name: Some("Next".to_string()),
                            match_expression: Some("/NEXT".to_string()),
                            order: -1,
                            ..ViewGroup::default()
                        },
                        ViewGroup {
                            by: Some("category".to_string()),
                            order: 9,
                            ..ViewGroup::default()
                        },
                    ],
                }],
            }],
            ..Settings::default()
        };
        let rendered = render_config(&settings, "groups").unwrap();
        assert!(
            rendered.contains("color = \"orange\", groups = ["),
            "{rendered}"
        );
        let mut unknown = settings.clone();
        unknown.roots = vec![std::env::temp_dir().to_string_lossy().into_owned()];
        unknown.views[0].blocks[0].groups[1].color = Some("crimson".to_string());
        let error = validate_settings(&unknown).unwrap_err().to_string();
        assert!(
            error.contains("color 'crimson'; use one of blue"),
            "{error}"
        );
        assert!(
            rendered.contains("    { name = \"Next\", match = \"/NEXT\", order = -1 },\n"),
            "{rendered}"
        );
        let parsed: Settings = toml::from_str(&rendered).unwrap();
        assert_eq!(parsed, settings);
    }

    #[test]
    fn removed_keys_are_unknown() {
        for (key, value) in [
            ("tag_alist", "[\"work\"]"),
            ("persistent_tags", "[\"work\"]"),
            ("agent_capture_to", "\"inbox\""),
            ("agent_inbox_template", "\"t\""),
        ] {
            let error = load_error(&format!("{key} = {value}\n"));
            assert!(error.contains(&format!("unknown field `{key}`")), "{error}");
        }
    }

    #[test]
    fn a_setting_below_a_record_is_explained_with_its_location() {
        let error = load_error(concat!(
            "[[capture]]\nkey = \"t\"\ndesc = \"Todo\"\nfile = \"inbox.org\"\n",
            "template = \"* %?\"\n",
            "start_view = \"n\"\n",
        ));
        assert_eq!(
            error,
            concat!(
                "cannot parse CONFIG: TOML parse error at line 7, column 1\n",
                "  |\n",
                "7 | start_view = \"n\"\n",
                "  | ^^^^^^^^^^\n",
                "`start_view` is a top-level setting; move it above the first [[capture]], [[view]] or [[agent_profile]] record\n",
            )
        );
        // A key that is no setting at all keeps TOML's own message.
        let error = load_error("[[agent_profile]]\nname = \"a\"\nfils = []\n");
        assert!(
            error.contains("unknown field `fils`, expected one of `name`, `files`, `match`"),
            "{error}"
        );
    }

    #[test]
    fn every_top_level_setting_is_rendered_and_explained() {
        let settings = Settings {
            agenda_start_day: Some("-1d".to_string()),
            agent_inbox: Some("t".to_string()),
            ..Settings::default()
        };
        let encoded = toml::Value::try_from(&settings).unwrap();
        for key in encoded.as_table().unwrap().keys() {
            if !["capture", "view", "agent_profile"].contains(&key.as_str()) {
                assert!(is_top_level_key(key), "{key} is missing from CONFIG_GROUPS");
            }
        }
    }

    #[test]
    fn template_file_is_read_at_load_relative_to_the_first_root() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("journal.txt"), "* %U %?\n").unwrap();
        let config = temp.path().join("config.toml");
        let root = toml::Value::String(temp.path().to_string_lossy().into_owned());
        fs::write(
            &config,
            format!(
                "roots = [{root}]\n[[capture]]\nkey = \"j\"\ndesc = \"Journal\"\nfile = \"j.org\"\ntemplate_file = \"journal.txt\"\n"
            ),
        )
        .unwrap();
        let loaded = load_settings(Some(&config), None).unwrap();
        assert_eq!(
            loaded.settings.capture_template("j").unwrap().template,
            "* %U %?\n"
        );

        fs::remove_file(temp.path().join("journal.txt")).unwrap();
        let error = load_settings(Some(&config), None).unwrap_err().to_string();
        assert!(
            error.starts_with(&format!(
                "capture template 'j': cannot read template_file {}: ",
                temp.path().join("journal.txt").display()
            )),
            "{error}"
        );
    }

    #[test]
    fn spans_take_a_name_or_a_toml_number_of_days() {
        let settings = load_body(concat!(
            "agenda_span = 10\n",
            "[[view]]\n",
            "key = \"f\"\n",
            "desc = \"Fortnight\"\n",
            "blocks = [{ kind = \"agenda\", span = 14 }, { kind = \"agenda\", span = \"day\" }]\n",
        ))
        .unwrap();
        assert_eq!(settings.agenda_span, AgendaSpan::Days(10));
        let spans: Vec<_> = settings.views[0]
            .blocks
            .iter()
            .map(|block| block.span)
            .collect();
        assert_eq!(spans, [Some(AgendaSpan::Days(14)), Some(AgendaSpan::Day)]);
        let rendered = render_config(&settings, "spans").unwrap();
        assert!(rendered.contains("agenda_span = 10\n"), "{rendered}");
        assert!(
            rendered.contains("{ kind = \"agenda\", span = 14 },\n"),
            "{rendered}"
        );
        assert_eq!(toml::from_str::<Settings>(&rendered).unwrap(), settings);

        assert_eq!(
            load_error("agenda_span = \"10\"\n"),
            concat!(
                "cannot parse CONFIG: TOML parse error at line 2, column 15\n",
                "  |\n",
                "2 | agenda_span = \"10\"\n",
                "  |               ^^^^\n",
                "write a number of days without quotes: 10, not \"10\"\n",
            )
        );
        assert!(
            load_error("agenda_span = \"wek\"\n")
                .ends_with("expected day, week, month or a number of days, not \"wek\"\n")
        );
        assert!(load_error("agenda_span = 0\n").ends_with("a span is at least 1 day, not 0\n"));
        assert!(
            load_error(concat!(
                "[[view]]\nkey = \"x\"\ndesc = \"X\"\n",
                "blocks = [{ kind = \"agenda\", span = \"fortnightly\" }]\n",
            ))
            .ends_with("expected day, week, month or a number of days, not \"fortnightly\"\n")
        );
    }

    #[test]
    fn explicit_missing_config_file_is_an_error() {
        let temp = tempfile::tempdir().unwrap();
        let error = load_settings(Some(&temp.path().join("missing.toml")), None).unwrap_err();
        assert_eq!(error.code(), "config");
    }

    #[test]
    fn cli_root_override_has_highest_precedence() {
        let temp = tempfile::tempdir().unwrap();
        let configured = temp.path().join("configured");
        let overridden = temp.path().join("overridden");
        fs::create_dir(&configured).unwrap();
        fs::create_dir(&overridden).unwrap();
        let config = temp.path().join("config.toml");
        fs::write(
            &config,
            format!(
                "roots = [{}]\n",
                toml::Value::String(configured.to_string_lossy().into_owned())
            ),
        )
        .unwrap();

        let loaded = load_settings(Some(&config), Some(&overridden)).unwrap();
        assert_eq!(loaded.settings.roots, vec![overridden.to_string_lossy()]);
    }
}
