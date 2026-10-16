use std::fs;

use chrono::Local;
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ReadResourceRequestParams},
    transport::{ConfigureCommandExt, TokioChildProcess},
};
use serde_json::{Map, Value, json};

fn binary() -> std::path::PathBuf {
    std::env::var_os("CARGO_BIN_EXE_agd")
        .map(Into::into)
        .unwrap_or_else(|| {
            let mut path = std::env::current_exe().expect("current executable");
            path.pop();
            if path.ends_with("deps") {
                path.pop();
            }
            path.join(format!("agd{}", std::env::consts::EXE_SUFFIX))
        })
}

#[cfg(not(target_os = "windows"))]
fn journal_data_directory(directory: &tempfile::TempDir) -> std::path::PathBuf {
    directory.path().join("data")
}

#[cfg(target_os = "windows")]
fn configure_data_directory(_: &mut tokio::process::Command, _: &tempfile::TempDir) {}

#[cfg(not(target_os = "windows"))]
fn configure_data_directory(command: &mut tokio::process::Command, directory: &tempfile::TempDir) {
    command.env("XDG_DATA_HOME", journal_data_directory(directory));
}

async fn service(
    directory: &tempfile::TempDir,
) -> Result<rmcp::service::RunningService<rmcp::RoleClient, ()>, Box<dyn std::error::Error>> {
    let command = tokio::process::Command::new(binary()).configure(|command| {
        command.args([
            "--root-directory",
            directory.path().to_str().expect("UTF-8 path"),
            "mcp",
        ]);
        configure_data_directory(command, directory);
        // Never read the developer's own config.
        command.env("XDG_CONFIG_HOME", directory.path().join("config"));
    });
    Ok(().serve(TokioChildProcess::new(command)?).await?)
}

fn fixture() -> tempfile::TempDir {
    let directory = tempfile::tempdir().expect("temporary directory");
    let today = Local::now().date_naive();
    fs::write(
        directory.path().join("tasks.org"),
        format!(
            "#+TODO: TODO NEXT | DONE\n* TODO Today :work:\n:PROPERTIES:\n:ID: today-task\n:END:\nSCHEDULED: <{}>\n* NEXT Later :work:\n",
            today.format("%Y-%m-%d %a")
        ),
    )
    .expect("org fixture");
    directory
}

fn content_json(value: &impl serde::Serialize) -> Value {
    serde_json::to_value(value).expect("MCP result JSON")
}

async fn call(
    service: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    tool: &'static str,
    arguments: Value,
) -> Result<rmcp::model::CallToolResult, Box<dyn std::error::Error>> {
    let arguments = arguments.as_object().expect("arguments object").clone();
    Ok(service
        .call_tool(CallToolRequestParams::new(tool).with_arguments(arguments))
        .await?)
}

/// The JSON a tool result carries in its first content block.
fn first_block(result: &rmcp::model::CallToolResult) -> Value {
    let text = result.content[0]
        .as_text()
        .expect("first content block is text")
        .text
        .as_str();
    serde_json::from_str(text).expect("first content block is JSON")
}

/// The org files in the fixture directory, to show a refused call wrote none.
fn org_files(directory: &tempfile::TempDir) -> usize {
    fs::read_dir(directory.path())
        .expect("fixture directory")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "org"))
        .count()
}

#[tokio::test]
async fn exposes_only_the_short_spec_tool_list() -> Result<(), Box<dyn std::error::Error>> {
    let directory = fixture();
    let service = service(&directory).await?;
    let tools = service.list_tools(Default::default()).await?;
    let mut names: Vec<&str> = tools.tools.iter().map(|tool| tool.name.as_ref()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["agenda", "capture", "edit", "plan", "query", "set_state"]
    );
    service.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn agenda_tool_and_today_resource_return_versioned_rows()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = fixture();
    let service = service(&directory).await?;
    let today = Local::now().date_naive().to_string();
    let arguments: Map<String, Value> = json!({"span": "day", "from": today})
        .as_object()
        .expect("arguments")
        .clone();
    let result = service
        .call_tool(CallToolRequestParams::new("agenda").with_arguments(arguments))
        .await?;
    let encoded = content_json(&result).to_string();
    assert!(encoded.contains("agd.agenda-entry/v1"));
    assert!(encoded.contains("Today"));

    let resources = service.list_resources(Default::default()).await?;
    assert_eq!(resources.resources.len(), 1);
    assert_eq!(resources.resources[0].uri, "agenda://today");
    let resource = service
        .read_resource(ReadResourceRequestParams::new("agenda://today".to_string()))
        .await?;
    let encoded = content_json(&resource).to_string();
    assert!(encoded.contains("agd.agenda-entry/v1"));
    service.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn writes_return_mutations_and_failures_return_error_schemas()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = fixture();
    let service = service(&directory).await?;
    let arguments: Map<String, Value> = json!({"ref": "today-task", "state": "DONE"})
        .as_object()
        .expect("arguments")
        .clone();
    let result = service
        .call_tool(CallToolRequestParams::new("set_state").with_arguments(arguments))
        .await?;
    let encoded = content_json(&result).to_string();
    assert!(encoded.contains("agd.mutation/v1"));
    assert!(encoded.contains("DONE"));
    #[cfg(not(target_os = "windows"))]
    {
        let journal =
            fs::read_to_string(journal_data_directory(&directory).join("agd/journal.jsonl"))?;
        let record: Value = serde_json::from_str(journal.lines().next().expect("journal record"))?;
        // The client's own name: rmcp's test client calls itself "rmcp".
        assert_eq!(record["actor"], "mcp:rmcp");
        assert_eq!(
            record["project"],
            directory.path().to_string_lossy().as_ref()
        );
    }

    let arguments: Map<String, Value> = json!({"ref": "missing", "state": "DONE"})
        .as_object()
        .expect("arguments")
        .clone();
    let result = service
        .call_tool(CallToolRequestParams::new("set_state").with_arguments(arguments))
        .await?;
    let encoded = content_json(&result).to_string();
    assert!(encoded.contains("agd.error/v1"));
    assert!(encoded.contains("stale_ref"));
    // A refused write is a tool error, so the client knows nothing changed.
    assert_eq!(result.is_error, Some(true));
    service.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn bad_arguments_are_error_schemas_naming_the_argument()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = fixture();
    let service = service(&directory).await?;

    let result = call(&service, "set_state", json!({"ref": 3, "state": "DONE"})).await?;
    assert_eq!(result.is_error, Some(true));
    let error = first_block(&result);
    assert_eq!(error["schema"], "agd.error/v1");
    assert_eq!(error["code"], "invalid_arguments");
    let message = error["message"].as_str().expect("message");
    assert!(
        message.starts_with("invalid `ref`: invalid type"),
        "{message}"
    );
    assert!(
        message.contains("accepted arguments: note, ref (required), state (required)"),
        "{message}"
    );

    let result = call(&service, "capture", json!({"title": "Buy milk"})).await?;
    assert_eq!(result.is_error, Some(true));
    let message = first_block(&result)["message"].to_string();
    assert!(message.contains("missing field `template`"), "{message}");
    assert_eq!(org_files(&directory), 1);
    service.cancel().await?;
    Ok(())
}

/// An invented argument used to be ignored: `capture` with `due` wrote a
/// task without its deadline, and `edit` with `state` said "nothing to edit".
#[tokio::test]
async fn unknown_arguments_are_refused_before_any_write() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = fixture();
    let service = service(&directory).await?;

    let result = call(
        &service,
        "capture",
        json!({"template": "t", "title": "Buy milk", "due": "fri"}),
    )
    .await?;
    assert_eq!(result.is_error, Some(true));
    let error = first_block(&result);
    assert_eq!(error["code"], "invalid_arguments");
    let message = error["message"].as_str().expect("message");
    assert!(
        message.starts_with("unknown argument `due`; accepted arguments: body, deadline,"),
        "{message}"
    );
    assert!(message.contains("deadline,"), "{message}");
    assert!(message.contains("template (required)"), "{message}");
    assert_eq!(org_files(&directory), 1);

    let result = call(
        &service,
        "edit",
        json!({"ref": "today-task", "state": "DONE"}),
    )
    .await?;
    assert_eq!(first_block(&result)["code"], "invalid_arguments");
    let before = fs::read_to_string(directory.path().join("tasks.org"))?;
    assert!(before.contains("* TODO Today"));

    // The published schemas say so, and describe every argument.
    for tool in service.list_tools(Default::default()).await?.tools {
        let schema = &tool.input_schema;
        assert_eq!(
            schema.get("additionalProperties"),
            Some(&json!(false)),
            "{}",
            tool.name
        );
        let properties = schema["properties"].as_object().expect("properties");
        for (name, property) in properties {
            assert!(
                property["description"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty()),
                "{}.{name} has no description",
                tool.name
            );
        }
    }
    service.cancel().await?;
    Ok(())
}

/// Claude Code speaks the 2026-07-28 protocol: `server/discover` first, no
/// `initialize`, and its name in every request's metadata. The journal must
/// still name it.
#[cfg(not(target_os = "windows"))]
#[test]
fn writes_from_a_client_without_initialize_are_journaled_under_its_name()
-> Result<(), Box<dyn std::error::Error>> {
    use std::io::{BufRead, BufReader, Write};

    let directory = fixture();
    let mut child = std::process::Command::new(binary())
        .args([
            "--root-directory",
            directory.path().to_str().expect("UTF-8 path"),
            "mcp",
        ])
        .env("XDG_DATA_HOME", journal_data_directory(&directory))
        .env("XDG_CONFIG_HOME", directory.path().join("config"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let meta = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": {"name": "claude-code", "version": "2.1.296"},
        "io.modelcontextprotocol/clientCapabilities": {}
    });
    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));
    let mut send = |request: Value| -> Result<String, Box<dyn std::error::Error>> {
        writeln!(stdin, "{request}")?;
        let mut response = String::new();
        stdout.read_line(&mut response)?;
        Ok(response)
    };
    let discovered = send(json!({
        "jsonrpc": "2.0", "id": 1, "method": "server/discover", "params": {"_meta": meta}
    }))?;
    assert!(discovered.contains("\"result\""), "{discovered}");
    let written = send(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "set_state",
            "arguments": {"ref": "today-task", "state": "DONE"},
            "_meta": meta
        }
    }))?;
    assert!(written.contains("agd.mutation/v1"), "{written}");
    drop(stdin);
    child.wait()?;

    let journal = fs::read_to_string(journal_data_directory(&directory).join("agd/journal.jsonl"))?;
    let record: Value = serde_json::from_str(journal.lines().next().expect("journal record"))?;
    assert_eq!(record["actor"], "mcp:claude-code");
    Ok(())
}
