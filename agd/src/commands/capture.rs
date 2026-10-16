use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::Path;

use agd_core::{
    AgdError, CaptureTemplate, ErrorCode, RootSet, Settings, WriteEngine, capture_prompts,
    load_settings, parse_capture_line,
};
use anyhow::Result;
use chrono::Local;
use clap::Args;

use super::write_stdout;
use crate::words;

const USAGE: &str = "capture needs text: agd capture [KEY] TEXT (KEY defaults to the first template), or agd capture -i";

#[derive(Args, Debug)]
pub struct CaptureCommand {
    /// A template key and the text, quoted or not: `t Renew passport
    /// due:fri`. One argument alone is the text for the first template.
    /// Tokens: due:fri on:+2d +tag #A
    #[arg(value_name = "KEY TEXT")]
    words: Vec<String>,
    /// Open the capture popup (for tmux display-popup or a pane keybinding)
    #[arg(
        short = 'i',
        long,
        conflicts_with_all = ["words", "body", "prompts", "dry_run", "json"]
    )]
    interactive: bool,
    /// Literal body text inserted at %i
    #[arg(long, default_value = "")]
    body: String,
    /// Answer for a %^{prompt} in the template
    #[arg(long = "prompt", value_name = "KEY=VALUE")]
    prompts: Vec<String>,
    /// Print the exact diff without writing
    #[arg(long)]
    dry_run: bool,
    /// Print the agd.mutation/v1 result
    #[arg(long)]
    json: bool,
}

impl CaptureCommand {
    /// True when the output, and so any error, is JSON.
    pub fn json(&self) -> bool {
        self.json
    }

    /// Prints the diff with --dry-run, the mutation with --json, a sentence
    /// on a terminal, and otherwise the new item's ref alone, which scripts
    /// and the Claude Code mod read.
    pub fn execute(self, config: Option<String>, root: Option<String>) -> Result<()> {
        if self.interactive {
            return crate::tui::run_capture(config, root);
        }
        if self.words.is_empty() {
            return Err(AgdError::new(ErrorCode::InvalidArguments, USAGE).into());
        }
        let settings = load_settings(
            config.as_deref().map(Path::new),
            root.as_deref().map(Path::new),
        )?
        .settings;
        let (template, text) = template_and_text(&self.words, &settings)?;
        let draft = parse_capture_line(&text, Local::now().date_naive())?;
        let prompts = parse_prompts(self.prompts)?;
        if let Some(capture) = settings.capture_template(&template) {
            check_prompts(&template, capture, &prompts)?;
        }
        let mutation = WriteEngine::with_default_journal(settings.clone())?.capture(
            &template,
            &draft,
            &self.body,
            &prompts,
            self.dry_run,
            &super::cli_context(),
        )?;
        let output = if self.json {
            format!("{}\n", serde_json::to_string_pretty(&mutation)?)
        } else if self.dry_run {
            mutation.diff
        } else if std::io::stdout().is_terminal() {
            let roots = RootSet::new(&settings.roots, &settings.agenda_files)
                .map(|roots| roots.roots().to_vec())
                .unwrap_or_default();
            let item = &mutation.item;
            let location = words::location(&item.file, item.line, &roots);
            format!(
                "{}\n",
                words::captured(item, &location, Local::now().date_naive())
            )
        } else {
            format!("{}\n", mutation.item.item_ref)
        };
        Ok(write_stdout(&output)?)
    }
}

/// The template key and the capture text. Two or more words are a key and
/// the text's words; one word is the text for the first template.
fn template_and_text(words: &[String], settings: &Settings) -> Result<(String, String), AgdError> {
    let keys = || {
        let keys: Vec<&str> = settings
            .captures
            .iter()
            .map(|template| template.key.as_str())
            .collect();
        keys.join(", ")
    };
    match words {
        [] => Err(AgdError::new(ErrorCode::InvalidArguments, USAGE)),
        [text] if settings.capture_template(text).is_some() => Err(AgdError::new(
            ErrorCode::InvalidArguments,
            format!("capture template '{text}' needs text: agd capture {text} Renew passport"),
        )),
        [text] => {
            let first = settings.captures.first().ok_or_else(|| {
                AgdError::new(
                    ErrorCode::UnknownTemplate,
                    "no capture templates are configured",
                )
            })?;
            Ok((first.key.clone(), text.clone()))
        }
        [key, text @ ..] if settings.capture_template(key).is_some() => {
            Ok((key.clone(), text.join(" ")))
        }
        [key, ..] => Err(AgdError::new(
            ErrorCode::UnknownTemplate,
            format!(
                "capture template '{key}' is not configured; templates: {}. To use the first \
                 template, quote the text: agd capture \"{}\"",
                keys(),
                words.join(" ")
            ),
        )),
    }
}

/// Names the `--prompt` a template's `%^{...}` field still needs; a date
/// prompt may stay empty.
fn check_prompts(
    key: &str,
    template: &CaptureTemplate,
    prompts: &BTreeMap<String, String>,
) -> Result<(), AgdError> {
    let missing = capture_prompts(template)
        .into_iter()
        .find(|prompt| !prompt.date && !prompts.contains_key(&prompt.name));
    match missing {
        Some(prompt) => Err(AgdError::new(
            ErrorCode::InvalidPrompt,
            format!(
                "capture template '{key}' asks for {name}; answer with --prompt '{name}=...'",
                name = prompt.name
            ),
        )),
        None => Ok(()),
    }
}

fn parse_prompts(values: Vec<String>) -> Result<BTreeMap<String, String>, AgdError> {
    values
        .into_iter()
        .map(|value| {
            let (key, value) = value.split_once('=').ok_or_else(|| {
                AgdError::new(
                    ErrorCode::InvalidPrompt,
                    format!("--prompt must be KEY=VALUE: {value}"),
                )
            })?;
            if key.is_empty() {
                return Err(AgdError::new(
                    ErrorCode::InvalidPrompt,
                    "--prompt key cannot be empty",
                ));
            }
            Ok((key.to_string(), value.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use agd_core::{Item, Planning, PlanningTimestamp};
    use std::path::PathBuf;

    #[test]
    fn a_terminal_reads_the_capture_as_the_tui_says_it() {
        let item = Item {
            schema: agd_core::ITEM_SCHEMA.to_string(),
            item_ref: "ref".to_string(),
            id: None,
            file: "/notes/org/inbox.org".to_string(),
            line: 19,
            olpath: vec!["Inbox".to_string(), "Renew passport".to_string()],
            todo: Some("TODO".to_string()),
            done: false,
            priority: None,
            title: "Renew passport".to_string(),
            tags: Vec::new(),
            inherited_tags: Vec::new(),
            category: "inbox".to_string(),
            planning: Planning {
                deadline: Some(PlanningTimestamp {
                    date: "2026-10-23".to_string(),
                    time: None,
                    end_time: None,
                    repeater: None,
                    warning: None,
                    raw: None,
                }),
                ..Planning::default()
            },
        };
        let location = words::location(&item.file, item.line, &[PathBuf::from("/notes/org")]);
        assert_eq!(
            words::captured(&item, &location, "2026-10-16".parse().unwrap()),
            "inbox.org:19  captured · Renew passport · deadline Fri 23 Oct"
        );
    }
}
