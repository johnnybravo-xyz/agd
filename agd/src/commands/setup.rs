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
    /// Import your Emacs or Neovim org settings into config.toml
    Import(ImportCommand),
    /// Add the agd MCP server and skill to Codex or OpenCode (Claude Code: the plugin)
    Install(InstallCommand),
    /// Check org files for parse drift, invalid UTF-8, duplicate IDs and interrupted writes
    Check(CheckCommand),
}

impl SetupCommand {
    /// Returns true when `check` found problems, so the caller can exit 1.
    pub fn execute(self, config: Option<String>, root: Option<String>) -> Result<bool> {
        match self.action {
            SetupAction::Import(command) => command.execute(config, root).map(|()| false),
            SetupAction::Install(command) => command.execute().map(|()| false),
            SetupAction::Check(command) => command.execute(config, root),
        }
    }
}
