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

/// The config.toml the server under test reads, beside its org files.
fn write_config(directory: &tempfile::TempDir, config: &str) {
    let path = directory.path().join("config/agd/config.toml");
    fs::create_dir_all(path.parent().expect("config directory")).expect("config directory");
    fs::write(path, config).expect("config file");
}

/// An org date stamp `days` from today, such as `2026-10-15 Thu`.
fn day(days: i64) -> String {
    (Local::now().date_naive() + chrono::Duration::days(days))
        .format("%Y-%m-%d %a")
        .to_string()
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
        [
            "agenda",
            "capture",
            "describe",
            "edit",
            "plan",
            "query",
            "set_state"
        ]
    );

    // The instructions carry the ref lifecycle, the date grammar, describe,
    // the default template and the local-only promise.
    let instructions = service
        .peer_info()
        .and_then(|info| info.instructions.clone())
        .expect("instructions");
    for needle in [
        "nothing leaves it",
        "use the item.ref it returns, or the current.ref of a ref_conflict error",
        "today, tomorrow, a weekday (fri), +2d",
        "2026-10-30T14:00; plan also takes none",
        "describe (templates, TODO keywords",
        "Capture with the first template",
    ] {
        assert!(instructions.contains(needle), "{needle}: {instructions}");
    }
    assert!(instructions.len() < 1000, "{}", instructions.len());

    // Reads say they change nothing; writes do not.
    for tool in &tools.tools {
        let read_only = tool
            .annotations
            .as_ref()
            .and_then(|annotations| annotations.read_only_hint);
        let expected = ["agenda", "query", "describe"]
            .contains(&tool.name.as_ref())
            .then_some(true);
        assert_eq!(read_only, expected, "{}", tool.name);
    }
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
    let encoded = content_json(&resource);
    assert!(encoded.to_string().contains("agd.agenda-entry/v1"));
    // What it returns is what the listing declares.
    assert_eq!(
        encoded["contents"][0]["mimeType"],
        content_json(&resources.resources[0])["mimeType"]
    );
    assert_eq!(encoded["contents"][0]["mimeType"], "application/json");
    service.cancel().await?;
    Ok(())
}

/// 150 tasks today and one tomorrow, the first with notes under it.
fn busy_fixture() -> tempfile::TempDir {
    let directory = tempfile::tempdir().expect("temporary directory");
    let today = Local::now().date_naive();
    let tomorrow = today + chrono::Duration::days(1);
    let mut source = String::new();
    for number in 0..150 {
        source.push_str(&format!(
            "* TODO Task {number:03}\nSCHEDULED: <{}>\n",
            today.format("%Y-%m-%d %a")
        ));
        if number == 0 {
            source.push_str("Call Ana first.\n");
        }
    }
    source.push_str(&format!(
        "* TODO Tomorrow's task\nSCHEDULED: <{}>\n",
        tomorrow.format("%Y-%m-%d %a")
    ));
    fs::write(directory.path().join("busy.org"), source).expect("org fixture");
    directory
}

#[tokio::test]
async fn reads_default_to_a_day_and_100_rows_and_say_what_they_left_out()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = busy_fixture();
    let service = service(&directory).await?;

    // No span is today only; no limit is 100 rows, and a second block says
    // how many there were. The first block keeps its array shape.
    let result = call(&service, "agenda", json!({})).await?;
    let entries = first_block(&result);
    assert_eq!(entries.as_array().map(Vec::len), Some(100));
    assert_eq!(entries[0]["schema"], "agd.agenda-entry/v1");
    assert_eq!(entries[0]["item"].get("body"), None);
    assert_eq!(
        result.content[1].as_text().map(|text| text.text.as_str()),
        Some("showing 100 of 150; pass limit or narrow the query")
    );
    let encoded = content_json(&result).to_string();
    assert!(!encoded.contains("Tomorrow's task"));

    let result = call(&service, "agenda", json!({"limit": 500, "with_body": true})).await?;
    assert_eq!(result.content.len(), 1);
    let entries = first_block(&result);
    assert_eq!(entries.as_array().map(Vec::len), Some(150));
    assert_eq!(entries[0]["item"]["title"], "Task 000");
    assert_eq!(entries[0]["item"]["body"], "Call Ana first.\n");
    assert_eq!(entries[1]["item"]["body"], "");

    let result = call(
        &service,
        "query",
        json!({"expression": "(todo)", "limit": 3}),
    )
    .await?;
    let items = first_block(&result);
    assert_eq!(items.as_array().map(Vec::len), Some(3));
    assert_eq!(items[0]["schema"], "agd.item/v1");
    assert_eq!(
        result.content[1].as_text().map(|text| text.text.as_str()),
        Some("showing 3 of 151; pass limit or narrow the query")
    );
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
    assert!(encoded.contains("ref_conflict"));
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
    // In the request's own order, required ones first.
    assert!(
        message.ends_with("accepted arguments: ref (required), state (required), note"),
        "{message}"
    );

    let result = call(&service, "capture", json!({"template": "t"})).await?;
    assert_eq!(result.is_error, Some(true));
    let message = first_block(&result)["message"].to_string();
    assert!(message.contains("missing field `title`"), "{message}");
    assert_eq!(org_files(&directory), 1);

    // Without a template, a capture goes through the first one.
    let result = call(&service, "capture", json!({"title": "Buy milk"})).await?;
    assert_eq!(result.is_error, Some(false), "{:?}", result.content);
    assert!(
        first_block(&result)["item"]["file"]
            .as_str()
            .is_some_and(|file| file.ends_with("inbox.org"))
    );
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
    assert_eq!(
        message,
        "unknown argument `due`; accepted arguments: template, title (required), \
         scheduled, deadline, tags, priority, body, prompts, properties"
    );
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

const VIEW_CONFIG: &str = r#"
agenda_start_day = "-1d"

[[view]]
key = "w"
desc = "Work"
blocks = [{ kind = "agenda", span = "day" }, { kind = "tags-todo", match = "work", header = "Work" }]

[[agent_profile]]
name = "home"
match = "home"
"#;

fn views_fixture() -> tempfile::TempDir {
    let directory = tempfile::tempdir().expect("temporary directory");
    fs::write(
        directory.path().join("tasks.org"),
        format!(
            "* TODO Work task :work:\nSCHEDULED: <{yesterday}>\n* TODO Home task :home:\nSCHEDULED: <{yesterday}>\n* TODO Second work task :work:\n* DONE Closed today :work:\nCLOSED: [{today} 10:00]\n",
            yesterday = day(-1),
            today = day(0)
        ),
    )
    .expect("org fixture");
    write_config(&directory, VIEW_CONFIG);
    directory
}

fn titles(rows: &Value) -> Vec<String> {
    rows.as_array()
        .expect("rows")
        .iter()
        .map(|row| row["item"]["title"].as_str().expect("title").to_string())
        .collect()
}

#[tokio::test]
async fn agenda_runs_views_and_profiles_from_agenda_start_day_as_the_cli_does()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = views_fixture();
    let service = service(&directory).await?;

    // agenda_start_day = "-1d": yesterday's tasks, scheduled on that day.
    let result = call(&service, "agenda", json!({})).await?;
    assert_eq!(titles(&first_block(&result)), ["Work task", "Home task"]);
    // The profile's match narrows it; from overrides the start day; log
    // adds what was closed.
    let result = call(&service, "agenda", json!({"profile": "home"})).await?;
    assert_eq!(titles(&first_block(&result)), ["Home task"]);
    let result = call(&service, "agenda", json!({"from": "today", "log": true})).await?;
    assert!(titles(&first_block(&result)).contains(&"Closed today".to_string()));

    // A view's sections, exactly as `agd agenda --view w --json` prints them.
    let result = call(&service, "agenda", json!({"view": "w"})).await?;
    assert_eq!(result.content.len(), 1);
    let cli = std::process::Command::new(binary())
        .args([
            "--root-directory",
            directory.path().to_str().expect("UTF-8 path"),
            "agenda",
            "--view",
            "w",
            "--json",
        ])
        .env("XDG_CONFIG_HOME", directory.path().join("config"))
        .env("XDG_DATA_HOME", directory.path().join("data"))
        .output()?;
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    let printed: Value = serde_json::from_slice(&cli.stdout)?;
    assert_eq!(first_block(&result), printed);
    assert_eq!(printed[1]["header"], "Work");

    // limit cuts rows across the sections and keeps every section.
    let result = call(&service, "agenda", json!({"view": "w", "limit": 3})).await?;
    let sections = first_block(&result);
    assert_eq!(sections.as_array().map(Vec::len), Some(2));
    assert_eq!(sections[1]["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        result.content[1].as_text().map(|text| text.text.as_str()),
        Some("showing 3 of 4; pass limit or narrow the query")
    );

    let result = call(&service, "agenda", json!({"view": "w", "span": "week"})).await?;
    assert_eq!(first_block(&result)["code"], "invalid_arguments");
    let result = call(&service, "agenda", json!({"view": "x"})).await?;
    let error = first_block(&result);
    assert_eq!(error["code"], "unknown_view");
    assert_eq!(
        error["message"],
        "unknown view 'x'; the views are 'w' and 'n'"
    );
    let result = call(&service, "agenda", json!({"profile": "work"})).await?;
    assert_eq!(first_block(&result)["code"], "unknown_profile");
    service.cancel().await?;
    Ok(())
}

/// With agent captures gated to the inbox, a capture used to answer ok:true
/// and the model reported a deadline the file never got.
#[tokio::test]
async fn inbox_captures_say_what_the_gate_changed() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    write_config(
        &directory,
        r#"
agent_inbox = "i"

[[capture]]
key = "w"
desc = "Work"
file = "work.org"
template = "* TODO %?"

[[capture]]
key = "i"
desc = "Inbox"
file = "inbox.org"
template = "* TODO %?"
"#,
    );
    let service = service(&directory).await?;
    let result = call(
        &service,
        "capture",
        json!({"template": "w", "title": "Ship it", "deadline": "2026-10-30", "scheduled": "2026-10-28T09:00"}),
    )
    .await?;
    assert_eq!(result.is_error, Some(false), "{:?}", result.content);
    let mutation = first_block(&result);
    assert_eq!(mutation["schema"], "agd.mutation/v1");
    assert_eq!(mutation["ok"], true);
    assert_eq!(mutation["item"]["planning"]["deadline"], Value::Null);
    assert_eq!(
        mutation["notes"],
        json!([
            "captured with the agent inbox template 'i', not 'w': agent captures go to the inbox",
            "dropped scheduled 2026-10-28 09:00: agent captures land in the inbox undated, for the person to plan",
            "dropped deadline 2026-10-30: agent captures land in the inbox undated, for the person to plan"
        ])
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("inbox.org"))?,
        "* TODO Ship it\n"
    );
    assert!(!directory.path().join("work.org").exists());

    // Nothing changed, so nothing to say.
    let result = call(
        &service,
        "capture",
        json!({"template": "i", "title": "Plain"}),
    )
    .await?;
    assert_eq!(first_block(&result).get("notes"), None);
    let described = first_block(&call(&service, "describe", json!({})).await?);
    assert_eq!(described["agent_inbox"], "i");
    assert_eq!(described["default_template"], "i");
    service.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn describe_lists_what_writes_accept() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    fs::write(
        directory.path().join("tasks.org"),
        "* TODO Report :work:\n* NEXT Call :work:phone:\n",
    )?;
    fs::write(
        directory.path().join("projects.org"),
        "#+TODO: OPEN | SHUT\n#+FILETAGS: :proj:\n* OPEN Garden\n",
    )?;
    write_config(
        &directory,
        r#"
todo_keywords = ["TODO NEXT | DONE"]
lowest_priority = "E"
default_priority = "C"
start_view = "w"

[[capture]]
key = "t"
desc = "Todo"
file = "inbox.org"
heading = "Inbox"
template = "* TODO %?"

[[capture]]
key = "m"
desc = "Meeting"
file = "meetings.org"
outline_path = ["Work", "Meetings"]
template = "* MEET %^{Who} about %?\nSCHEDULED: %^{When}t"

[[view]]
key = "w"
desc = "Work"
blocks = [{ kind = "tags-todo", match = "work" }]
"#,
    );
    let service = service(&directory).await?;
    let result = call(&service, "describe", json!({})).await?;
    assert_eq!(result.is_error, Some(false), "{:?}", result.content);
    let mut described = first_block(&result);
    let files = described["todo_keywords"]["overrides"][0]["files"].take();
    assert!(
        files[0]
            .as_str()
            .is_some_and(|file| file.ends_with("projects.org")),
        "{files}"
    );
    assert_eq!(
        described,
        json!({
            "schema": "agd.describe/v1",
            "capture_templates": [
                {"key": "t", "desc": "Todo", "target": "inbox.org > Inbox", "prompts": []},
                {
                    "key": "m",
                    "desc": "Meeting",
                    "target": "meetings.org > Work > Meetings",
                    "prompts": ["Who", "When"]
                }
            ],
            "default_template": "t",
            "agent_inbox": null,
            "todo_keywords": {
                "default": ["TODO NEXT | DONE"],
                "overrides": [{"sequences": ["OPEN | SHUT"], "files": null}]
            },
            "priorities": {"highest": "A", "lowest": "E", "default": "C"},
            "views": [
                {"key": "w", "desc": "Work", "start": true},
                {"key": "n", "desc": "Agenda and all TODOs", "start": false}
            ],
            "tags": ["phone", "proj", "work"],
            "date_grammar": "today, tomorrow, a weekday (fri), +2d (also +1w, +1m, +1y), 2026-10-30 or 2026-10-30T14:00"
        })
    );

    let result = call(&service, "describe", json!({"verbose": true})).await?;
    assert_eq!(first_block(&result)["code"], "invalid_arguments");
    service.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn plan_none_removes_a_date() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("plan.org");
    fs::write(
        &path,
        "* TODO Plan me\nSCHEDULED: <2026-10-20 Tue> DEADLINE: <2026-10-22 Thu>\n:PROPERTIES:\n:ID: plan-task\n:END:\n",
    )?;
    let service = service(&directory).await?;
    let result = call(
        &service,
        "plan",
        json!({"ref": "plan-task", "scheduled": "none"}),
    )
    .await?;
    assert_eq!(result.is_error, Some(false), "{:?}", result.content);
    let mutation = first_block(&result);
    assert_eq!(mutation["schema"], "agd.mutation/v1");
    assert_eq!(mutation["item"]["planning"]["scheduled"], Value::Null);
    assert_eq!(
        fs::read_to_string(&path)?,
        "* TODO Plan me\nDEADLINE: <2026-10-22 Thu>\n:PROPERTIES:\n:ID: plan-task\n:END:\n"
    );
    service.cancel().await?;
    Ok(())
}

/// Sends raw JSON-RPC lines to a fresh server, reading one answer per
/// request (a message with an id), and returns them with the journal's
/// first actor once the server exits.
#[cfg(not(target_os = "windows"))]
fn session_actor(messages: &[Value]) -> Result<(Vec<String>, Value), Box<dyn std::error::Error>> {
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
    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));
    let mut answers = Vec::new();
    for message in messages {
        writeln!(stdin, "{message}")?;
        if message.get("id").is_some() {
            let mut answer = String::new();
            stdout.read_line(&mut answer)?;
            answers.push(answer);
        }
    }
    drop(stdin);
    child.wait()?;
    let journal = fs::read_to_string(journal_data_directory(&directory).join("agd/journal.jsonl"))?;
    let record: Value = serde_json::from_str(journal.lines().next().expect("journal record"))?;
    Ok((answers, record["actor"].clone()))
}

#[cfg(not(target_os = "windows"))]
fn set_state_call(id: u64, meta: Option<&Value>) -> Value {
    let mut params = json!({
        "name": "set_state",
        "arguments": {"ref": "today-task", "state": "DONE"},
    });
    if let Some(meta) = meta {
        params["_meta"] = meta.clone();
    }
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": params})
}

/// Claude Code speaks the 2026-07-28 protocol: `server/discover` first, no
/// `initialize`, and its name in every request's metadata. The journal must
/// still name it; Codex's "-mcp-client" suffix is dropped either way.
#[cfg(not(target_os = "windows"))]
#[test]
fn writes_from_a_client_without_initialize_are_journaled_under_its_name()
-> Result<(), Box<dyn std::error::Error>> {
    for (client, actor) in [
        ("claude-code", "mcp:claude-code"),
        ("codex-mcp-client", "mcp:codex"),
    ] {
        let meta = json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientInfo": {"name": client, "version": "2.1.296"},
            "io.modelcontextprotocol/clientCapabilities": {}
        });
        let (answers, journaled) = session_actor(&[
            json!({"jsonrpc": "2.0", "id": 1, "method": "server/discover", "params": {"_meta": meta}}),
            set_state_call(2, Some(&meta)),
        ])?;
        assert!(answers[0].contains("\"result\""), "{}", answers[0]);
        assert!(answers[1].contains("agd.mutation/v1"), "{}", answers[1]);
        assert_eq!(journaled, actor);
    }
    Ok(())
}

/// Codex initializes as "codex-mcp-client"; the journal says `mcp:codex`.
/// Other names are kept as sent.
#[cfg(not(target_os = "windows"))]
#[test]
fn initialize_client_names_drop_the_mcp_client_suffix() -> Result<(), Box<dyn std::error::Error>> {
    for (client, actor) in [("codex-mcp-client", "mcp:codex"), ("zed", "mcp:zed")] {
        let (answers, journaled) = session_actor(&[
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": client, "version": "0.50.0"}
                }
            }),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            set_state_call(2, None),
        ])?;
        assert!(answers[1].contains("agd.mutation/v1"), "{}", answers[1]);
        assert_eq!(journaled, actor);
    }
    Ok(())
}
