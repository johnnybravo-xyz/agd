use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand};
use serde_json::{Map, Value, json};

/// The one copy of the skill, shared with the Claude Code plugin.
const SKILL: &str = include_str!("../../claude-plugin/skills/agenda/SKILL.md");

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
    let skill_path = directory.join(SKILL_PATH);
    check_skill(&skill_path)?;
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
    write_skill(skill_path)?;
    super::write_stdout(&format!(
        "installed Codex integration in {}\n",
        directory.display()
    ))?;
    Ok(())
}

fn install_opencode(directory: &Path) -> Result<()> {
    let skill_path = directory.join(SKILL_PATH);
    check_skill(&skill_path)?;
    let config_path = directory.join("opencode.json");
    let mut config = read_json_object(&config_path)?;
    object_at(&mut config, "mcp")?.insert(
        "agd".to_string(),
        json!({"type": "local", "command": ["agd", "mcp"], "enabled": true}),
    );
    write_json(&config_path, &config)?;
    write_skill(skill_path)?;
    super::write_stdout(&format!(
        "installed OpenCode integration in {}\n",
        directory.display()
    ))?;
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

/// Where Codex and OpenCode both read the skill.
const SKILL_PATH: &str = ".agents/skills/agenda/SKILL.md";

/// `agenda` is a common name, so a skill already there is replaced only when
/// it is agd's own (any version says it works through "the agd MCP server");
/// anyone else's is left alone, before anything is written.
fn check_skill(path: &Path) -> Result<()> {
    match fs::read_to_string(path) {
        Ok(existing) if existing.contains("the agd MCP server") => Ok(()),
        Ok(_) => bail!(
            "{} is another skill named agenda; move it aside, then install again",
            path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("cannot read {}", path.display())),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_the_skill_and_replaces_only_agds_own() {
        let directory = tempfile::tempdir().unwrap();
        install_codex(directory.path()).unwrap();
        let skill = directory.path().join(SKILL_PATH);
        assert_eq!(fs::read_to_string(&skill).unwrap(), SKILL);

        // An older agd skill is replaced.
        fs::write(
            &skill,
            "---\nname: agenda\ndescription: through the agd MCP server.\n---\n",
        )
        .unwrap();
        install_opencode(directory.path()).unwrap();
        assert_eq!(fs::read_to_string(&skill).unwrap(), SKILL);
    }

    #[test]
    fn refuses_someone_elses_agenda_skill_before_writing_anything() {
        let directory = tempfile::tempdir().unwrap();
        let skill = directory.path().join(SKILL_PATH);
        fs::create_dir_all(skill.parent().unwrap()).unwrap();
        let theirs = "---\nname: agenda\ndescription: My meeting agendas.\n---\n";
        fs::write(&skill, theirs).unwrap();

        let error = install_codex(directory.path()).unwrap_err().to_string();
        assert!(error.contains("is another skill named agenda"), "{error}");
        assert_eq!(fs::read_to_string(&skill).unwrap(), theirs);
        assert!(!directory.path().join(".codex/config.toml").exists());

        assert!(install_opencode(directory.path()).is_err());
        assert!(!directory.path().join("opencode.json").exists());
    }
}
