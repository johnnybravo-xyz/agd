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
        assert!(record["actor"].as_str().expect("actor").starts_with("mcp:"));
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
    service.cancel().await?;
    Ok(())
}
