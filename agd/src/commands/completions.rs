use anyhow::Result;
use clap::Args;
use clap_complete::Shell;

use super::write_stdout;

#[derive(Args, Debug)]
pub struct CompletionsCommand {
    /// The shell to complete in
    #[arg(value_enum)]
    shell: Shell,
}

impl CompletionsCommand {
    /// Prints the static completion script for `command`, agd's own
    /// command line, so every subcommand and flag completes. It reads no
    /// config and no files, so it works before setup.
    pub fn execute(self, mut command: clap::Command) -> Result<()> {
        let mut script = Vec::new();
        clap_complete::generate(self.shell, &mut command, "agd", &mut script);
        Ok(write_stdout(&String::from_utf8(script)?)?)
    }
}
