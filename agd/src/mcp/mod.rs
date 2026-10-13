mod resources;
mod tools;
mod utils;

use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Args;
use org_core::{LoggingConfig, OrgMode, OrgModeError, config::OrgConfig};
use rmcp::{ServiceExt, handler::server::tool::ToolRouter, transport::stdio};
use tokio::{runtime::Builder, sync::Mutex};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[derive(Args, Debug)]
pub(crate) struct McpCommand {
    /// Log level (trace, debug, info, warn, error)
    #[arg(long)]
    pub(crate) log_level: Option<String>,
}

pub(crate) struct OrgModeRouter {
    pub(crate) org_mode: Arc<Mutex<OrgMode>>,
}

impl McpCommand {
    pub(crate) fn execute(self, config: OrgConfig, logging: LoggingConfig) -> Result<()> {
        let log_level = self
            .log_level
            .as_deref()
            .unwrap_or(&logging.level)
            .parse::<tracing::Level>()
            .unwrap_or(tracing::Level::INFO);

        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::from_default_env().add_directive(log_level.into()))
            .with_writer(std::io::stderr)
            .try_init()
            .map_err(|error| anyhow::anyhow!("failed to initialize MCP logging: {error}"))?;

        info!(
            "Starting MCP server with org directory: {}",
            config.org_directory
        );

        Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("failed to create MCP runtime")?
            .block_on(serve(config))
    }
}

async fn serve(config: OrgConfig) -> Result<()> {
    let service = OrgModeRouter::with_config(config)?
        .serve(stdio())
        .await
        .inspect_err(|error| {
            error!("Error starting server: {error}");
        })
        .context("failed to start MCP server")?;
    service
        .waiting()
        .await
        .context("MCP server stopped with an error")?;
    Ok(())
}

impl OrgModeRouter {
    fn with_config(config: OrgConfig) -> Result<Self, OrgModeError> {
        let org_mode = OrgMode::new(config)?;
        Ok(Self {
            org_mode: Arc::new(Mutex::new(org_mode)),
        })
    }

    #[cfg(test)]
    fn with_directory(org_dir: &str) -> Result<Self, OrgModeError> {
        let config = OrgConfig {
            org_directory: org_dir.to_string(),
            ..OrgConfig::default()
        };
        let config = config.validate()?;
        Self::with_config(config)
    }

    pub(crate) fn tool_router() -> ToolRouter<Self> {
        Self::tool_router_list_files()
            + Self::tool_router_search()
            + Self::tool_router_agenda()
            + Self::tool_router_capture()
            + Self::tool_router_update_todo()
    }
}
