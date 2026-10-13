//! Integration tests for the `agd mcp` stdio surface.

#[path = "mcp_integration/utils.rs"]
mod utils;

pub(crate) mod mcp_integration {
    pub(crate) mod resources_tests;
    pub(crate) mod server_tests;
    pub(crate) mod tools_tests;
}

pub(crate) use utils::{get_binary_path, setup_test_org_files};

#[macro_export]
macro_rules! create_mcp_service {
    ($temp_dir:expr) => {{
        use rmcp::{
            ServiceExt,
            transport::{ConfigureCommandExt, TokioChildProcess},
        };
        use tracing::error;

        let command =
            tokio::process::Command::new($crate::get_binary_path("agd")).configure(|cmd| {
                cmd.args([
                    "--root-directory",
                    $temp_dir.path().to_str().unwrap(),
                    "mcp",
                ]);
            });

        ().serve(TokioChildProcess::new(command)?)
            .await
            .map_err(|e| {
                error!("Failed to connect to server: {}", e);
                e
            })?
    }};
}
