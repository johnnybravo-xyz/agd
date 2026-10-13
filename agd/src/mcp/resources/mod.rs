use chrono::Local;
use org_core::{AgendaEngine, AgendaQuery, WorkspaceIndex};
use rmcp::model::{
    Implementation, InitializeRequestParams, InitializeResult, ListResourcesResult,
    PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult,
    Resource, ResourceContents, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, tool_handler};
use serde_json::json;

use crate::mcp::OrgModeRouter;

#[tool_handler]
impl ServerHandler for OrgModeRouter {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_instructions(
            "Local-only org agenda tools. Responses use agendum.item/v1, agendum.agenda-entry/v1, agendum.mutation/v1, or agendum.error/v1 schemas.",
        )
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
            })
        }) {
            Ok(entries) => serde_json::to_string(&entries),
            Err(error) => serde_json::to_string(error.response()),
        }
        .map_err(|error| McpError::internal_error(error.to_string(), None))?;
        Ok(ReadResourceResult::new(vec![ResourceContents::text(payload, uri)]).into())
    }

    async fn initialize(
        &self,
        _request: InitializeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        Ok(self.get_info())
    }
}
