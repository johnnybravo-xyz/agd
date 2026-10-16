//! `agd mcp`: the stdio MCP server. `server` holds the handler (instructions
//! and the agenda://today resource), `tools` the tools, and `describe` what
//! the `describe` tool and the instructions tell an agent about the setup.

mod describe;
mod server;
mod tools;

use std::path::Path;

use agd_core::{Settings, load_settings};
use anyhow::{Context, Result};
use clap::Args;
use clap::builder::{PossibleValuesParser, TypedValueParser};
use rmcp::{ServiceExt, transport::stdio};
use tokio::runtime::Builder;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[derive(Args, Debug)]
pub(crate) struct McpCommand {
    /// Log level for stderr
    #[arg(
        long,
        default_value = "info",
        value_parser = PossibleValuesParser::new(["trace", "debug", "info", "warn", "error"])
            .map(|level| level.parse::<tracing::Level>().expect("a listed level")),
    )]
    pub(crate) log_level: tracing::Level,
}

pub(crate) struct OrgModeRouter {
    pub(crate) settings: Settings,
    pub(crate) project: String,
}

impl McpCommand {
    pub(crate) fn execute(
        self,
        config: Option<String>,
        root_directory: Option<String>,
    ) -> Result<()> {
        let project = root_directory.clone().unwrap_or_else(|| {
            std::env::current_dir()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|_| "unknown".to_string())
        });
        let settings = load_settings(
            config.as_deref().map(Path::new),
            root_directory.as_deref().map(Path::new),
        )?;
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::from_default_env().add_directive(self.log_level.into()))
            .with_writer(std::io::stderr)
            .try_init()
            .map_err(|error| anyhow::anyhow!("failed to initialize MCP logging: {error}"))?;
        info!(roots = ?settings.roots, "starting stdio MCP server");
        Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("failed to create MCP runtime")?
            .block_on(serve(settings, project))
    }
}

async fn serve(settings: Settings, project: String) -> Result<()> {
    let service = OrgModeRouter { settings, project }
        .serve(stdio())
        .await
        .inspect_err(|error| error!("error starting MCP server: {error}"))
        .context("failed to start MCP server")?;
    service
        .waiting()
        .await
        .context("MCP server stopped with an error")?;
    Ok(())
}
