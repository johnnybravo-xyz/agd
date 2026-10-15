use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use clap::{Args, Subcommand};
use serde_json::{Map, Value, json};

/// The one copy of the skill, shared with the Claude Code plugin.
const SKILL: &str = include_str!("../../claude-plugin/skills/agd/SKILL.md");

#[derive(Args, Debug)]
pub struct InstallCommand {
    /// Project directory that receives the integration files.
    #[arg(long, default_value = ".")]
    directory: PathBuf,
    #[command(subcommand)]
    target: InstallTarget,
}

/// Claude Code gets agd from its plugin (`claude plugin install
/// agd@johnnybravo-xyz`), so there is no Claude target here.
#[derive(Subcommand, Debug)]
enum InstallTarget {
    /// Install the Codex MCP server and skill.
    Codex,
    /// Install the OpenCode MCP server and skill.
    Opencode,
}

impl InstallCommand {
    pub fn execute(self) -> Result<()> {
        let directory = absolute_directory(&self.directory)?;
        match self.target {
            InstallTarget::Codex => install_codex(&directory)?,
            InstallTarget::Opencode => install_opencode(&directory)?,
        }
        Ok(())
    }
}

fn install_codex(directory: &Path) -> Result<()> {
    let config_path = directory.join(".codex/config.toml");
    let source = fs::read_to_string(&config_path).unwrap_or_default();
    let mut config: toml::Value = if source.trim().is_empty() {
        toml::Value::Table(Default::default())
    } else {
        toml::from_str(&source)
            .with_context(|| format!("cannot parse {}", config_path.display()))?
    };
    let root = config
        .as_table_mut()
        .ok_or_else(|| anyhow!("{} must contain a TOML table", config_path.display()))?;
    let servers = table_at(root, "mcp_servers", &config_path)?;
    servers.insert(
        "agd".to_string(),
        toml::Value::Table(toml::Table::from_iter([
            (
                "command".to_string(),
                toml::Value::String("agd".to_string()),
            ),
            (
                "args".to_string(),
                toml::Value::Array(vec![toml::Value::String("mcp".to_string())]),
            ),
        ])),
    );
    atomic_write(
        &config_path,
        format!("{}\n", toml::to_string_pretty(&config)?).as_bytes(),
    )?;
    write_skill(directory.join(".agents/skills/agd/SKILL.md"))?;
    println!("installed Codex integration in {}", directory.display());
    Ok(())
}

fn install_opencode(directory: &Path) -> Result<()> {
    let config_path = directory.join("opencode.json");
    let mut config = read_json_object(&config_path)?;
    object_at(&mut config, "mcp")?.insert(
        "agd".to_string(),
        json!({"type": "local", "command": ["agd", "mcp"], "enabled": true}),
    );
    write_json(&config_path, &config)?;
    write_skill(directory.join(".agents/skills/agd/SKILL.md"))?;
    println!("installed OpenCode integration in {}", directory.display());
    Ok(())
}

fn absolute_directory(directory: &Path) -> Result<PathBuf> {
    let path = if directory.is_absolute() {
        directory.to_path_buf()
    } else {
        std::env::current_dir()?.join(directory)
    };
    fs::create_dir_all(&path)
        .with_context(|| format!("cannot create integration directory {}", path.display()))?;
    path.canonicalize()
        .with_context(|| format!("cannot resolve integration directory {}", path.display()))
}

fn read_json_object(path: &Path) -> Result<Map<String, Value>> {
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(error) => return Err(error).with_context(|| format!("cannot read {}", path.display())),
    };
    serde_json::from_str::<Value>(&source)
        .with_context(|| format!("cannot parse {}", path.display()))?
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow!("{} must contain a JSON object", path.display()))
}

fn object_at<'a>(
    root: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Map<String, Value>> {
    root.entry(key.to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| anyhow!("{key} must be a JSON object"))
}

fn table_at<'a>(root: &'a mut toml::Table, key: &str, path: &Path) -> Result<&'a mut toml::Table> {
    root.entry(key.to_string())
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
        .ok_or_else(|| anyhow!("{}.{key} must be a TOML table", path.display()))
}

fn write_json(path: &Path, object: &Map<String, Value>) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(object)?;
    bytes.push(b'\n');
    atomic_write(path, &bytes)
}

fn write_skill(path: PathBuf) -> Result<()> {
    atomic_write(&path, SKILL.as_bytes())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("cannot create {}", parent.display()))?;
    let temporary = parent.join(format!(
        ".{}.agd.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("config"),
        std::process::id()
    ));
    let permissions = fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .with_context(|| format!("cannot create {}", temporary.display()))?;
    let result = (|| -> Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        if let Some(permissions) = permissions {
            fs::set_permissions(&temporary, permissions)?;
        }
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.with_context(|| format!("cannot write {}", path.display()))
}
