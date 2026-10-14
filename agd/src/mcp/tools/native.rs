use std::collections::{BTreeMap, BTreeSet};

use chrono::{Local, NaiveDate};
use org_core::{
    AgdError, AgendaEngine, AgendaQuery, AgendaSpan, CaptureDraft, Change, EditIntent,
    PlanningTimestamp, WorkspaceIndex, WriteContext, WriteEngine, parse_date_input,
};
use rmcp::{
    ErrorData as McpError, Peer, RoleServer,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock, ErrorCode},
    schemars, tool, tool_router,
};
use serde::Serialize;

use crate::mcp::OrgModeRouter;

#[derive(Debug, schemars::JsonSchema, serde::Deserialize)]
struct AgendaRequest {
    /// day, week, or month; defaults to the imported agenda span.
    span: Option<String>,
    /// Start date as YYYY-MM-DD; defaults to today.
    from: Option<String>,
    /// Optional Emacs tags/TODO match expression.
    #[serde(rename = "match")]
    match_expression: Option<String>,
}

#[derive(Debug, schemars::JsonSchema, serde::Deserialize)]
struct CaptureRequest {
    /// Configured capture template key.
    template: String,
    title: String,
    scheduled: Option<String>,
    deadline: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    priority: Option<String>,
    #[serde(default)]
    body: String,
    #[serde(default)]
    prompts: BTreeMap<String, String>,
    /// Extra :PROPERTIES: recording where the task came from, e.g. REPO,
    /// BRANCH, or SOURCE as an org link such as [[file:/repo/src/main.rs::42]].
    #[serde(default)]
    properties: BTreeMap<String, String>,
}

#[derive(Debug, schemars::JsonSchema, serde::Deserialize)]
struct SetStateRequest {
    #[serde(rename = "ref")]
    item_ref: String,
    state: String,
    note: Option<String>,
}

#[derive(Debug, schemars::JsonSchema, serde::Deserialize)]
struct PlanRequest {
    #[serde(rename = "ref")]
    item_ref: String,
    scheduled: Option<String>,
    deadline: Option<String>,
    note: Option<String>,
}

#[derive(Debug, schemars::JsonSchema, serde::Deserialize)]
struct EditRequest {
    #[serde(rename = "ref")]
    item_ref: String,
    title: Option<String>,
    #[serde(default)]
    tags_add: Vec<String>,
    #[serde(default)]
    tags_remove: Vec<String>,
    priority: Option<String>,
    #[serde(default)]
    properties: BTreeMap<String, Option<String>>,
    note: Option<String>,
}

#[derive(Debug, schemars::JsonSchema, serde::Deserialize)]
struct QueryRequest {
    /// Supported org-ql subset expression.
    expression: String,
}

#[tool_router(router = "tool_router_native", vis = "pub(crate)")]
impl OrgModeRouter {
    #[tool(
        name = "agenda",
        description = "Return native agenda-entry/v1 rows for a date span"
    )]
    async fn agenda(
        &self,
        Parameters(request): Parameters<AgendaRequest>,
    ) -> Result<CallToolResult, McpError> {
        let result = (|| {
            let index = WorkspaceIndex::load(self.settings.clone())?;
            let today = Local::now().date_naive();
            let from = request
                .from
                .as_deref()
                .map(|value| parse_day(value, today))
                .transpose()?
                .unwrap_or(today);
            let span = match request
                .span
                .as_deref()
                .unwrap_or(&self.settings.agenda_span)
            {
                "day" => AgendaSpan::Day,
                "week" => AgendaSpan::Week,
                "month" => AgendaSpan::Month,
                value => {
                    return Err(AgdError::new(
                        "invalid_span",
                        format!("unsupported agenda span: {value}"),
                    ));
                }
            };
            let (from, to) = span.bounds(from, &self.settings.week_start);
            AgendaEngine::new(&index).agenda(&AgendaQuery {
                from,
                to,
                today,
                match_expression: request.match_expression,
                log_mode: false,
            })
        })();
        schema_result(result)
    }

    #[tool(
        name = "capture",
        description = "Capture a literal item with a configured template"
    )]
    async fn capture(
        &self,
        Parameters(request): Parameters<CaptureRequest>,
        client: Peer<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let result = (|| {
            let today = Local::now().date_naive();
            let draft = CaptureDraft {
                title: request.title,
                scheduled: parse_optional_date(request.scheduled.as_deref(), today)?,
                deadline: parse_optional_date(request.deadline.as_deref(), today)?,
                tags: request.tags,
                priority: request.priority,
                properties: request.properties,
            };
            WriteEngine::with_default_journal(self.settings.clone())?.capture(
                &request.template,
                &draft,
                &request.body,
                &request.prompts,
                false,
                &write_context(&client, &self.project),
            )
        })();
        schema_result(result)
    }

    #[tool(
        name = "set_state",
        description = "Set an item's TODO state after ref revalidation"
    )]
    async fn set_state(
        &self,
        Parameters(request): Parameters<SetStateRequest>,
        client: Peer<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let intent = EditIntent {
            state: Change::Set(request.state),
            note: request.note,
            ..EditIntent::default()
        };
        schema_result(edit(
            &self.settings,
            &request.item_ref,
            intent,
            &client,
            &self.project,
        ))
    }

    #[tool(
        name = "plan",
        description = "Set an item's scheduled and/or deadline timestamp"
    )]
    async fn plan(
        &self,
        Parameters(request): Parameters<PlanRequest>,
        client: Peer<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let result = (|| {
            if request.scheduled.is_none() && request.deadline.is_none() {
                return Err(AgdError::new(
                    "invalid_plan",
                    "scheduled or deadline is required",
                ));
            }
            let today = Local::now().date_naive();
            let intent = EditIntent {
                scheduled: request
                    .scheduled
                    .as_deref()
                    .map(|value| parse_date_input(value, today).map(Change::Set))
                    .transpose()?
                    .unwrap_or_default(),
                deadline: request
                    .deadline
                    .as_deref()
                    .map(|value| parse_date_input(value, today).map(Change::Set))
                    .transpose()?
                    .unwrap_or_default(),
                note: request.note,
                ..EditIntent::default()
            };
            edit(
                &self.settings,
                &request.item_ref,
                intent,
                &client,
                &self.project,
            )
        })();
        schema_result(result)
    }

    #[tool(
        name = "edit",
        description = "Edit item title, tags, priority, properties, or note"
    )]
    async fn edit(
        &self,
        Parameters(request): Parameters<EditRequest>,
        client: Peer<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let intent = EditIntent {
            title: request.title.map(Change::Set).unwrap_or_default(),
            priority: request.priority.map(Change::Set).unwrap_or_default(),
            tags_add: request.tags_add.into_iter().collect::<BTreeSet<_>>(),
            tags_remove: request.tags_remove.into_iter().collect::<BTreeSet<_>>(),
            properties: request.properties,
            note: request.note,
            ..EditIntent::default()
        };
        schema_result(edit(
            &self.settings,
            &request.item_ref,
            intent,
            &client,
            &self.project,
        ))
    }

    #[tool(
        name = "query",
        description = "Query items with the supported org-ql subset"
    )]
    async fn query(
        &self,
        Parameters(request): Parameters<QueryRequest>,
    ) -> Result<CallToolResult, McpError> {
        let result = WorkspaceIndex::load(self.settings.clone()).and_then(|index| {
            AgendaEngine::new(&index).query(&request.expression, Local::now().date_naive())
        });
        schema_result(result)
    }
}

fn edit(
    settings: &org_core::Settings,
    item_ref: &str,
    intent: EditIntent,
    client: &Peer<RoleServer>,
    project: &str,
) -> Result<org_core::Mutation, AgdError> {
    WriteEngine::with_default_journal(settings.clone())?.edit(
        item_ref,
        &intent,
        false,
        &write_context(client, project),
    )
}

fn schema_result<T: Serialize>(result: Result<T, AgdError>) -> Result<CallToolResult, McpError> {
    let content = match result {
        Ok(value) => ContentBlock::json(value),
        Err(error) => ContentBlock::json(error.response()),
    }
    .map_err(|error| McpError {
        code: ErrorCode::INTERNAL_ERROR,
        message: format!("cannot serialize schema response: {error}").into(),
        data: None,
    })?;
    Ok(CallToolResult::success(vec![content]))
}

fn parse_optional_date(
    value: Option<&str>,
    today: NaiveDate,
) -> Result<Option<PlanningTimestamp>, AgdError> {
    value
        .map(|value| parse_date_input(value, today))
        .transpose()
}

fn parse_day(value: &str, today: NaiveDate) -> Result<NaiveDate, AgdError> {
    parse_date_input(value, today)?
        .date
        .parse()
        .map_err(|_| AgdError::new("invalid_date", format!("invalid date: {value}")))
}

fn write_context(client: &Peer<RoleServer>, project: &str) -> WriteContext {
    WriteContext {
        actor: client
            .peer_info()
            .map(|info| format!("mcp:{}", info.client_info.name))
            .unwrap_or_else(|| "mcp:unknown".to_string()),
        project: project.to_string(),
        now: None,
    }
}
