use std::collections::BTreeMap;
use std::path::Path;

use agd_core::{AgdError, WriteContext, WriteEngine, load_settings, parse_capture_line};
use anyhow::Result;
use chrono::Local;
use clap::Args;

#[derive(Args, Debug)]
pub struct CaptureCommand {
    /// Capture template key, as in org-capture (t, n, ...)
    template: Option<String>,
    /// One line with tokens: due:fri on:+2d +tag #A
    text: Option<String>,
    /// Open the capture popup (for tmux display-popup or a pane keybinding)
    #[arg(
        short = 'i',
        long,
        conflicts_with_all = ["template", "text", "body", "prompts", "dry_run", "json"]
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

    pub fn execute(self, config: Option<String>, root: Option<String>) -> Result<()> {
        if self.interactive {
            return crate::tui::run_capture(config, root);
        }
        let (Some(template), Some(text)) = (self.template, self.text) else {
            return Err(AgdError::new(
                "usage",
                "usage: agd capture <template> \"<text>\" or agd capture -i",
            )
            .into());
        };
        let settings = load_settings(
            config.as_deref().map(Path::new),
            root.as_deref().map(Path::new),
        )?
        .settings;
        let draft = parse_capture_line(&text, Local::now().date_naive())?;
        let prompts = parse_prompts(self.prompts)?;
        let mutation = WriteEngine::with_default_journal(settings)?.capture(
            &template,
            &draft,
            &self.body,
            &prompts,
            self.dry_run,
            &WriteContext {
                actor: "cli".to_string(),
                project: std::env::current_dir()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| "unknown".to_string()),
                now: None,
            },
        )?;
        if self.json {
            println!("{}", serde_json::to_string_pretty(&mutation)?);
        } else if self.dry_run {
            print!("{}", mutation.diff);
        } else {
            println!("{}", mutation.item.item_ref);
        }
        Ok(())
    }
}

fn parse_prompts(values: Vec<String>) -> Result<BTreeMap<String, String>, AgdError> {
    values
        .into_iter()
        .map(|value| {
            let (key, value) = value.split_once('=').ok_or_else(|| {
                AgdError::new(
                    "invalid_prompt",
                    format!("--prompt must be KEY=VALUE: {value}"),
                )
            })?;
            if key.is_empty() {
                return Err(AgdError::new(
                    "invalid_prompt",
                    "--prompt key cannot be empty",
                ));
            }
            Ok((key.to_string(), value.to_string()))
        })
        .collect()
}
