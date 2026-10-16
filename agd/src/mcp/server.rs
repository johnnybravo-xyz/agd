//! The MCP handler: the server's instructions, its tools (from `tools`)
//! and the agenda://today resource.

use agd_core::{AgendaEngine, AgendaQuery, Settings, WorkspaceIndex};
use chrono::Local;
use rmcp::model::{
    Implementation, ListResourcesResult, PaginatedRequestParams, ReadResourceRequestParams,
    ReadResourceResponse, ReadResourceResult, Resource, ResourceContents, ServerCapabilities,
    ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, tool_handler};
use serde_json::json;

use crate::mcp::{OrgModeRouter, describe};

#[tool_handler]
impl ServerHandler for OrgModeRouter {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_instructions(instructions(&self.settings))
        .with_server_info(Implementation::new("agd", env!("CARGO_PKG_VERSION")))
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult::with_all_items(vec![
            Resource::new("agenda://today", "agenda-today")
                .with_description("Today's native agenda-entry/v1 rows")
                .with_mime_type("application/json"),
        ]))
    }

    async fn read_resource(
        &self,
        ReadResourceRequestParams { uri, .. }: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        if uri != "agenda://today" {
            return Err(McpError::resource_not_found(
                format!("unknown resource: {uri}"),
                Some(json!({ "uri": uri })),
            ));
        }
        let today = Local::now().date_naive();
        let payload = match WorkspaceIndex::load(self.settings.clone()).and_then(|index| {
            AgendaEngine::new(&index).agenda(&AgendaQuery {
                from: today,
                to: today,
                today,
                match_expression: None,
                log_mode: false,
            })
        }) {
            Ok(entries) => serde_json::to_string(&entries),
            Err(error) => serde_json::to_string(error.response()),
        }
        .map_err(|error| McpError::internal_error(error.to_string(), None))?;
        Ok(ReadResourceResult::new(vec![ResourceContents::text(payload, uri)]).into())
    }

    // rmcp's own `initialize` records the client's name, which journals each
    // agent write as `mcp:<client>`; do not override it.
}

/// What a model needs before its first call. Kept short: clients put it in
/// every prompt, and `describe` carries the details.
fn instructions(settings: &Settings) -> String {
    let capture = match (
        describe::agent_inbox(settings),
        describe::default_template(settings),
    ) {
        (Some(inbox), _) => format!(
            "Every capture goes through the inbox template `{inbox}`, undated, for the person to triage."
        ),
        (None, Some(key)) => {
            format!("Capture with template `{key}` unless the person names another.")
        }
        (None, None) => "No capture templates are configured.".to_string(),
    };
    format!(
        "agd reads and writes the person's org files on this machine; nothing leaves it, so keep agenda data in this session. \
Read with agenda (today unless told otherwise), query (org-ql) or describe (templates, TODO keywords, priorities, views, tags). \
Write with capture, set_state, plan or edit. A write revalidates the item's ref, and a ref without an :ID: changes with every write: \
use the item.ref it returns, or the current.ref of a stale_ref error, never an older one. Dates: {}; plan also takes none. {capture} \
A failed call sets isError and answers agd.error/v1.",
        describe::DATE_GRAMMAR
    )
}
