//! The MCP tools: one request struct per tool, then the router with one
//! `#[tool]` method each, then the helpers they share. Every tool answers
//! with one JSON content block in an agd schema; a failure is an
//! agd.error/v1 block with `isError` set.

use std::collections::{BTreeMap, BTreeSet};

use agd_core::{
    AgdError, AgendaEngine, AgendaQuery, AgendaSpan, CaptureDraft, Change, EditIntent, ITEM_SCHEMA,
    PlanningTimestamp, WorkspaceIndex, WriteContext, WriteEngine, parse_date_input,
};
use chrono::{Local, NaiveDate};
use rmcp::{
    ErrorData as McpError, RoleServer,
    handler::server::tool::{schema_for_input, schema_for_type},
    model::{CallToolResult, ContentBlock, ErrorCode, JsonObject},
    schemars::{self, JsonSchema},
    service::RequestContext,
    tool, tool_router,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::mcp::OrgModeRouter;

// Request structs refuse unknown arguments, so a misspelled or invented one
// (`due` for `deadline`) is an error instead of being silently ignored; the
// published schema says so with `additionalProperties: false`. Each field's
// doc comment is its description in that schema.

#[derive(Debug, schemars::JsonSchema, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AgendaRequest {
    /// day, week or month; defaults to day.
    span: Option<String>,
    /// First day: today, tomorrow, a weekday (fri), +2d (also +1w, +1m,
    /// +1y) or 2026-10-30; defaults to today.
    from: Option<String>,
    /// Emacs tags/TODO match, such as `work-someday/NEXT|TODO`. TODO
    /// keywords come from each file's #+TODO line or the configured
    /// todo_keywords; `describe` lists them.
    #[serde(rename = "match")]
    match_expression: Option<String>,
    /// Most rows to return; defaults to 100. A second content block says
    /// when rows were left out.
    limit: Option<usize>,
    /// Also return each item's body: the text under its heading, such as
    /// notes and its LOGBOOK.
    #[serde(default)]
    with_body: bool,
}

#[derive(Debug, schemars::JsonSchema, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CaptureRequest {
    /// Capture template key from the configuration, such as `t`;
    /// `describe` lists the templates.
    template: String,
    /// Heading text, taken literally.
    title: String,
    /// SCHEDULED date: today, tomorrow, a weekday (fri), +2d (also +1w,
    /// +1m, +1y), 2026-10-30 or 2026-10-30T14:00.
    scheduled: Option<String>,
    /// DEADLINE date: today, tomorrow, a weekday (fri), +2d (also +1w,
    /// +1m, +1y), 2026-10-30 or 2026-10-30T14:00.
    deadline: Option<String>,
    /// Tags without colons, such as ["work"].
    #[serde(default)]
    tags: Vec<String>,
    /// Priority letter within the configured range, such as A.
    priority: Option<String>,
    /// Literal text kept under the heading: at the template's %i, else
    /// after the heading's own lines.
    #[serde(default)]
    body: String,
    /// Answers to the template's %^{name} prompts, keyed by name;
    /// `describe` lists each template's prompts.
    #[serde(default)]
    prompts: BTreeMap<String, String>,
    /// Extra :PROPERTIES: recording where the task came from, e.g. REPO,
    /// BRANCH, or SOURCE as an org link such as [[file:/repo/src/main.rs::42]].
    #[serde(default)]
    properties: BTreeMap<String, String>,
}

#[derive(Debug, schemars::JsonSchema, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SetStateRequest {
    /// The item's ref from agenda, query or the last write to it; a write
    /// changes it.
    #[serde(rename = "ref")]
    item_ref: String,
    /// A TODO keyword of the item's file (its #+TODO line, else the
    /// configured todo_keywords), such as DONE; `describe` lists them.
    state: String,
    /// Note logged with the state change.
    note: Option<String>,
}

#[derive(Debug, schemars::JsonSchema, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanRequest {
    /// The item's ref from agenda, query or the last write to it; a write
    /// changes it.
    #[serde(rename = "ref")]
    item_ref: String,
    /// New SCHEDULED date: today, tomorrow, a weekday (fri), +2d (also
    /// +1w, +1m, +1y), 2026-10-30 or 2026-10-30T14:00; `none` removes it.
    scheduled: Option<String>,
    /// New DEADLINE date: today, tomorrow, a weekday (fri), +2d (also +1w,
    /// +1m, +1y), 2026-10-30 or 2026-10-30T14:00; `none` removes it.
    deadline: Option<String>,
    /// Note logged with the change.
    note: Option<String>,
}

#[derive(Debug, schemars::JsonSchema, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct EditRequest {
    /// The item's ref from agenda, query or the last write to it; a write
    /// changes it.
    #[serde(rename = "ref")]
    item_ref: String,
    /// New heading title, taken literally.
    title: Option<String>,
    /// Tags to add, without colons.
    #[serde(default)]
    tags_add: Vec<String>,
    /// Tags to remove, without colons.
    #[serde(default)]
    tags_remove: Vec<String>,
    /// New priority letter within the configured range, such as A.
    priority: Option<String>,
    /// :PROPERTIES: to set, by name; a null value removes that property.
    #[serde(default)]
    properties: BTreeMap<String, Option<String>>,
    /// Note logged under the heading.
    note: Option<String>,
}

#[derive(Debug, schemars::JsonSchema, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryRequest {
    /// org-ql expression, such as (and (todo) (deadline :to 7)): open
    /// tasks due within a week, overdue ones included. Predicates: and,
    /// or, not, (todo) for any not-done keyword or (todo "NEXT" "WAIT"),
    /// done, tags, priority, category, text, deadline :to DAYS,
    /// scheduled :to DAYS.
    expression: String,
    /// Most items to return; defaults to 100. A second content block says
    /// when items were left out.
    limit: Option<usize>,
    /// Also return each item's body: the text under its heading, such as
    /// notes and its LOGBOOK.
    #[serde(default)]
    with_body: bool,
}

/// Rows `agenda` and `query` return when the call gives no `limit`; a busy
/// agenda's full week runs to hundreds of kilobytes.
const DEFAULT_LIMIT: usize = 100;

/// Each tool takes its arguments as a raw object and parses them itself
/// (`parse_arguments`), so a bad argument is an agd.error/v1 result rather
/// than a JSON-RPC error; `input_schema` still publishes the request struct.
#[tool_router(vis = "pub(crate)")]
impl OrgModeRouter {
    #[tool(
        name = "agenda",
        description = "Return native agenda-entry/v1 rows for a date span",
        input_schema = input_schema::<AgendaRequest>()
    )]
    async fn agenda(&self, arguments: JsonObject) -> Result<CallToolResult, McpError> {
        rows_result(
            parse_arguments::<AgendaRequest>(arguments)
                .and_then(|request| agenda(&self.settings, request)),
        )
    }

    #[tool(
        name = "capture",
        description = "Capture a literal item with a configured template",
        input_schema = input_schema::<CaptureRequest>()
    )]
    async fn capture(
        &self,
        arguments: JsonObject,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let write = write_context(&context, &self.project);
        tool_result(
            parse_arguments::<CaptureRequest>(arguments)
                .and_then(|request| capture(&self.settings, request, &write)),
        )
    }

    #[tool(
        name = "set_state",
        description = "Set an item's TODO state after ref revalidation",
        input_schema = input_schema::<SetStateRequest>()
    )]
    async fn set_state(
        &self,
        arguments: JsonObject,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let write = write_context(&context, &self.project);
        tool_result(
            parse_arguments::<SetStateRequest>(arguments).and_then(|request| {
                let intent = EditIntent {
                    state: Change::Set(request.state),
                    note: request.note,
                    ..EditIntent::default()
                };
                edit(&self.settings, &request.item_ref, intent, &write)
            }),
        )
    }

    #[tool(
        name = "plan",
        description = "Set an item's scheduled and/or deadline timestamp",
        input_schema = input_schema::<PlanRequest>()
    )]
    async fn plan(
        &self,
        arguments: JsonObject,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let write = write_context(&context, &self.project);
        tool_result(
            parse_arguments::<PlanRequest>(arguments)
                .and_then(|request| plan(&self.settings, request, &write)),
        )
    }

    #[tool(
        name = "edit",
        description = "Edit item title, tags, priority, properties, or note",
        input_schema = input_schema::<EditRequest>()
    )]
    async fn edit(
        &self,
        arguments: JsonObject,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let write = write_context(&context, &self.project);
        tool_result(
            parse_arguments::<EditRequest>(arguments).and_then(|request| {
                let intent = EditIntent {
                    title: request.title.map(Change::Set).unwrap_or_default(),
                    priority: request.priority.map(Change::Set).unwrap_or_default(),
                    tags_add: request.tags_add.into_iter().collect::<BTreeSet<_>>(),
                    tags_remove: request.tags_remove.into_iter().collect::<BTreeSet<_>>(),
                    properties: request.properties,
                    note: request.note,
                    ..EditIntent::default()
                };
                edit(&self.settings, &request.item_ref, intent, &write)
            }),
        )
    }

    #[tool(
        name = "query",
        description = "Query items with the supported org-ql subset",
        input_schema = input_schema::<QueryRequest>()
    )]
    async fn query(&self, arguments: JsonObject) -> Result<CallToolResult, McpError> {
        rows_result(
            parse_arguments::<QueryRequest>(arguments)
                .and_then(|request| query(&self.settings, request)),
        )
    }
}

fn agenda(settings: &agd_core::Settings, request: AgendaRequest) -> Result<Rows, AgdError> {
    let index = WorkspaceIndex::load(settings.clone())?;
    let today = Local::now().date_naive();
    let from = request
        .from
        .as_deref()
        .map(|value| parse_day(value, today))
        .transpose()?
        .unwrap_or(today);
    let span = match request.span.as_deref().unwrap_or("day") {
        "day" => AgendaSpan::Day,
        "week" => AgendaSpan::Week,
        "month" => AgendaSpan::Month,
        value => {
            return Err(AgdError::new(
                "invalid_span",
                format!("unsupported agenda span: {value}; use day, week or month"),
            ));
        }
    };
    let (from, to) = span.bounds(from, &settings.week_start);
    let mut entries = AgendaEngine::new(&index).agenda(&AgendaQuery {
        from,
        to,
        today,
        match_expression: request.match_expression,
        log_mode: false,
    })?;
    let total = entries.len();
    entries.truncate(request.limit.unwrap_or(DEFAULT_LIMIT));
    Rows::new(&entries, total, request.with_body.then_some(&index))
}

fn query(settings: &agd_core::Settings, request: QueryRequest) -> Result<Rows, AgdError> {
    let index = WorkspaceIndex::load(settings.clone())?;
    let mut items =
        AgendaEngine::new(&index).query(&request.expression, Local::now().date_naive())?;
    let total = items.len();
    items.truncate(request.limit.unwrap_or(DEFAULT_LIMIT));
    Rows::new(&items, total, request.with_body.then_some(&index))
}

/// What `agenda` and `query` answer: their JSON, which keeps its shape when
/// `limit` cuts it short, and how many rows there were before the cut.
struct Rows {
    json: Value,
    shown: usize,
    total: usize,
}

impl Rows {
    /// `rows` as JSON, with each item's body added when `bodies` gives the
    /// index they were read from.
    fn new<T: Serialize>(
        rows: &[T],
        total: usize,
        bodies: Option<&WorkspaceIndex>,
    ) -> Result<Self, AgdError> {
        let mut json = serde_json::to_value(rows).map_err(|error| {
            AgdError::with_source("internal", "cannot encode the rows as JSON", error)
        })?;
        if let Some(index) = bodies {
            add_bodies(&mut json, index);
        }
        Ok(Self {
            json,
            shown: rows.len(),
            total,
        })
    }
}

/// Adds `body`, the text under the heading (notes, LOGBOOK, plain text),
/// to every agd.item/v1 object in `json`, wherever the schema nests it.
fn add_bodies(json: &mut Value, index: &WorkspaceIndex) {
    match json {
        Value::Array(values) => {
            for value in values {
                add_bodies(value, index);
            }
        }
        Value::Object(object) => {
            if object.get("schema").and_then(Value::as_str) == Some(ITEM_SCHEMA) {
                let body = item_body(object, index).unwrap_or_default();
                object.insert("body".to_string(), Value::String(body));
            }
            for value in object.values_mut() {
                add_bodies(value, index);
            }
        }
        _ => {}
    }
}

/// The body of the heading an item JSON object names by `file` and `line`.
fn item_body(item: &JsonObject, index: &WorkspaceIndex) -> Option<String> {
    let file = item.get("file")?.as_str()?;
    let line = usize::try_from(item.get("line")?.as_u64()?).ok()?;
    let document = index.document(std::path::Path::new(file))?;
    let span = document.headline_at_line(line)?.body_span;
    document
        .source()
        .get(span.start..span.end)
        .map(str::to_string)
}

fn capture(
    settings: &agd_core::Settings,
    request: CaptureRequest,
    context: &WriteContext,
) -> Result<agd_core::Mutation, AgdError> {
    let today = Local::now().date_naive();
    let draft = CaptureDraft {
        title: request.title,
        scheduled: parse_optional_date(request.scheduled.as_deref(), today)?,
        deadline: parse_optional_date(request.deadline.as_deref(), today)?,
        tags: request.tags,
        priority: request.priority,
        properties: request.properties,
    };
    WriteEngine::with_default_journal(settings.clone())?.capture(
        &request.template,
        &draft,
        &request.body,
        &request.prompts,
        false,
        context,
    )
}

fn plan(
    settings: &agd_core::Settings,
    request: PlanRequest,
    context: &WriteContext,
) -> Result<agd_core::Mutation, AgdError> {
    if request.scheduled.is_none() && request.deadline.is_none() {
        return Err(AgdError::new(
            "invalid_plan",
            "scheduled or deadline is required",
        ));
    }
    let today = Local::now().date_naive();
    let intent = EditIntent {
        scheduled: planning_change(request.scheduled.as_deref(), today)?,
        deadline: planning_change(request.deadline.as_deref(), today)?,
        note: request.note,
        ..EditIntent::default()
    };
    edit(settings, &request.item_ref, intent, context)
}

/// A `plan` date argument: left out keeps the date, `none` removes it, and
/// anything else is a date to set.
fn planning_change(
    value: Option<&str>,
    today: NaiveDate,
) -> Result<Change<PlanningTimestamp>, AgdError> {
    match value {
        None => Ok(Change::Keep),
        Some(value) if value.trim().eq_ignore_ascii_case("none") => Ok(Change::Clear),
        Some(value) => parse_date_input(value, today).map(Change::Set),
    }
}

fn edit(
    settings: &agd_core::Settings,
    item_ref: &str,
    intent: EditIntent,
    context: &WriteContext,
) -> Result<agd_core::Mutation, AgdError> {
    WriteEngine::with_default_journal(settings.clone())?.edit(item_ref, &intent, false, context)
}

/// A tool's JSON schema for its request struct, as `Parameters<T>` would
/// publish it.
fn input_schema<T: JsonSchema + 'static>() -> std::sync::Arc<JsonObject> {
    schema_for_input::<T>()
        .unwrap_or_else(|error| panic!("invalid input schema for a tool request: {error}"))
}

/// Reads a tool's arguments into its request struct. A bad argument is an
/// `invalid_arguments` error naming it and listing the accepted arguments,
/// so the model can correct its call.
fn parse_arguments<T: DeserializeOwned + JsonSchema + 'static>(
    arguments: JsonObject,
) -> Result<T, AgdError> {
    let schema = schema_for_type::<T>();
    let accepted = schema.get("properties").and_then(Value::as_object);
    let unknown = arguments
        .keys()
        .find(|name| accepted.is_some_and(|accepted| !accepted.contains_key(name.as_str())));
    let problem = if let Some(name) = unknown {
        format!("unknown argument `{name}`")
    } else {
        let error = match serde_json::from_value::<T>(Value::Object(arguments.clone())) {
            Ok(request) => return Ok(request),
            Err(error) => error.to_string(),
        };
        // serde names a missing argument; a type error names only the type
        // it expected.
        match invalid_argument::<T>(&arguments) {
            Some(name) if !error.starts_with("missing field") => {
                format!("invalid `{name}`: {error}")
            }
            _ => error,
        }
    };
    Err(AgdError::new(
        "invalid_arguments",
        format!(
            "{problem}; accepted arguments: {}",
            accepted_arguments(&schema)
        ),
    ))
}

/// The argument whose value fails to parse: the one that, left out, lets
/// the rest parse or be missing only itself.
fn invalid_argument<T: DeserializeOwned>(arguments: &JsonObject) -> Option<String> {
    arguments
        .keys()
        .find(|name| {
            let mut rest = arguments.clone();
            rest.remove(name.as_str());
            match serde_json::from_value::<T>(Value::Object(rest)) {
                Ok(_) => true,
                Err(error) => error.to_string() == format!("missing field `{name}`"),
            }
        })
        .cloned()
}

/// `template (required), title (required), scheduled, ...` from the
/// request's schema.
fn accepted_arguments(schema: &JsonObject) -> String {
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|names| names.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|properties| {
            properties
                .keys()
                .map(|name| {
                    if required.contains(&name.as_str()) {
                        format!("{name} (required)")
                    } else {
                        name.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

/// The tool's answer: the value as JSON on success; the agd.error/v1
/// response with `isError` set on failure, so the client sees the call
/// did not do what was asked.
fn tool_result<T: Serialize>(result: Result<T, AgdError>) -> Result<CallToolResult, McpError> {
    match result {
        Ok(value) => Ok(CallToolResult::success(vec![json_block(value)?])),
        Err(error) => Ok(CallToolResult::error(vec![json_block(error.response())?])),
    }
}

/// A read's answer: its rows' JSON, then, when `limit` left rows out, a
/// text block saying how many.
fn rows_result(result: Result<Rows, AgdError>) -> Result<CallToolResult, McpError> {
    let rows = match result {
        Ok(rows) => rows,
        Err(error) => return tool_result::<()>(Err(error)),
    };
    let mut content = vec![json_block(&rows.json)?];
    if rows.shown < rows.total {
        content.push(ContentBlock::text(format!(
            "showing {} of {}; pass limit or narrow the query",
            rows.shown, rows.total
        )));
    }
    Ok(CallToolResult::success(content))
}

fn json_block(value: impl Serialize) -> Result<ContentBlock, McpError> {
    ContentBlock::json(value).map_err(|error| McpError {
        code: ErrorCode::INTERNAL_ERROR,
        message: format!("cannot serialize schema response: {error}").into(),
        data: None,
    })
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

/// Journals the write under the agent's own name. A client on the 2026-07-28
/// protocol (Claude Code) sends its name with every request rather than in an
/// `initialize` handshake; `RequestContext::client_info` covers both.
fn write_context(context: &RequestContext<RoleServer>, project: &str) -> WriteContext {
    WriteContext {
        actor: context
            .client_info()
            .map(|info| format!("mcp:{}", info.name))
            .unwrap_or_else(|| "mcp:unknown".to_string()),
        project: project.to_string(),
        now: None,
    }
}
