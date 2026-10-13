use anyhow::Result;
use clap::{Args, Subcommand};

use super::{CheckCommand, ImportCommand, InstallCommand};

/// One-time and occasional setup: editor import, agent install, file check.
#[derive(Args)]
pub struct SetupCommand {
    #[command(subcommand)]
    action: SetupAction,
}

#[derive(Subcommand)]
enum SetupAction {
    /// Import resolved settings from Emacs or nvim-orgmode into a snapshot
    Import(ImportCommand),
    /// Add the agd MCP server and skill to Claude Code, Codex or OpenCode
    Install(InstallCommand),
    /// Check org files for parse drift, invalid UTF-8 and duplicate IDs
    Check(CheckCommand),
}

impl SetupCommand {
    /// Returns true when `check` found problems, so the caller can exit 1.
    pub fn execute(self, config: Option<String>, root: Option<String>) -> Result<bool> {
        match self.action {
            SetupAction::Import(command) => command.execute(config, root).map(|()| false),
            SetupAction::Install(command) => command.execute(config, root).map(|()| false),
            SetupAction::Check(command) => command.execute(config, root),
        }
    }
}
