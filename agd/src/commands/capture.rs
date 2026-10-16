use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::Path;

use agd_core::{
    AgdError, CaptureTemplate, ErrorCode, RootSet, WriteEngine, capture_prompts, load_settings,
    parse_capture_line,
};
use anyhow::Result;
use chrono::Local;
use clap::Args;

use super::write_stdout;
use crate::words;

const USAGE: &str = "capture needs text: agd capture [KEY] TEXT (KEY defaults to the first template), or agd capture -i";

#[derive(Args, Debug)]
pub struct CaptureCommand {
    /// The capture line: its first word is a template key when it names one
    /// and more text follows (`t Renew passport due:fri`); otherwise the
    /// whole line goes through the first template. Tokens: due:fri on:+2d
    /// #tag !A, or :tag: #A with notation = "org". Put text that starts
    /// with - after --: agd capture t -- -x
    #[arg(value_name = "[KEY] TEXT", allow_negative_numbers = true)]
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
    /// Print the agd.mutation/v1 result, with the item's file:line under
    /// its root as `location`
    #[arg(long)]
    pub json: bool,
}

impl CaptureCommand {
    /// Prints the diff with --dry-run, the mutation with --json (and its
    /// `location` as people read it, `projects/alpha.org:3`, which the
    /// Claude Code mod's /capture says), a sentence on a terminal, and
    /// otherwise the new item's ref alone, which scripts read.
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
        )?;
        let line = self.words.join(" ");
        let (template, text) = settings.capture_line_template(&line)?;
        let draft = parse_capture_line(text, Local::now().date_naive(), settings.notation)?;
        let prompts = parse_prompts(self.prompts)?;
        check_prompts(template, &prompts)?;
        let mutation = WriteEngine::with_default_journal(settings.clone())?.capture(
            &template.key,
            &draft,
            &self.body,
            &prompts,
            self.dry_run,
            &super::cli_context(),
        )?;
        let roots = RootSet::new(&settings.roots, &settings.agenda_files)
            .map(|roots| roots.roots().to_vec())
            .unwrap_or_default();
        let item = &mutation.item;
        let location = words::location(&item.file, item.line, &roots);
        let output = if self.json {
            let mut json = serde_json::to_value(&mutation)?;
            json["location"] = location.into();
            format!("{}\n", serde_json::to_string_pretty(&json)?)
        } else if self.dry_run {
            mutation.diff
        } else if std::io::stdout().is_terminal() {
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

/// Names the `--prompt` a template's `%^{...}` field still needs; a date
/// prompt may stay empty.
fn check_prompts(
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
                "capture template '{key}' asks for '{name}'; answer with --prompt '{name}=...'",
                key = template.key,
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
            checklist: None,
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
