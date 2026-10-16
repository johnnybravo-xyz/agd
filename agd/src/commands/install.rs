//! `agd setup install codex|opencode`: adds the agd MCP server entry and the
//! `agenda` skill to a project. Each person-owned config file is edited, not
//! rewritten: Codex's TOML through toml_edit (comments and order survive),
//! OpenCode's JSON with its keys in their order. An existing agd entry that
//! differs, or a file agd cannot edit safely, is refused before anything is
//! written.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand};
use serde_json::{Map, Value, json};
use similar::TextDiff;
use toml_edit::{Array, DocumentMut, Item, Table, value};

/// The one copy of the skill, shared with the Claude Code plugin.
const SKILL: &str = include_str!("../../claude-plugin/skills/agenda/SKILL.md");

/// Where Codex and OpenCode both read the skill.
const SKILL_PATH: &str = ".agents/skills/agenda/SKILL.md";

#[derive(Args, Debug)]
pub struct InstallCommand {
    /// Project directory that receives the integration files; it must
    /// already exist. Given before or after the target.
    #[arg(long, default_value = ".", global = true)]
    directory: PathBuf,
    /// Print what would change as a diff and write nothing.
    #[arg(long, global = true)]
    dry_run: bool,
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

/// One file the install writes: what it holds now (empty when missing) and
/// what it will hold.
#[derive(Debug)]
struct FileChange {
    path: PathBuf,
    before: String,
    after: String,
}

impl InstallCommand {
    pub fn execute(self) -> Result<()> {
        let directory = existing_directory(&self.directory)?;
        let (name, changes) = match self.target {
            InstallTarget::Codex => ("Codex", codex_changes(&directory)?),
            InstallTarget::Opencode => ("OpenCode", opencode_changes(&directory)?),
        };
        if self.dry_run {
            for change in &changes {
                print!("{}", diff(change));
            }
            if changes.is_empty() {
                println!(
                    "{name} integration in {} is up to date",
                    directory.display()
                );
            }
        } else if changes.is_empty() {
            println!(
                "{name} integration already installed in {}",
                directory.display()
            );
        } else {
            for change in &changes {
                atomic_write(&change.path, change.after.as_bytes())?;
            }
            println!("installed {name} integration in {}", directory.display());
        }
        if let InstallTarget::Codex = self.target {
            println!(
                "note: Codex reads a project's .codex/config.toml only when it trusts the project; trust {} in Codex to load agd",
                directory.display()
            );
        }
        Ok(())
    }
}

/// The project directory as an absolute path; a missing one is an error
/// rather than created, so a typo does not install into a new directory.
fn existing_directory(directory: &Path) -> Result<PathBuf> {
    let path = if directory.is_absolute() {
        directory.to_path_buf()
    } else {
        std::env::current_dir()?.join(directory)
    };
    if !path.is_dir() {
        bail!(
            "integration directory {} does not exist; create it, or pass --directory with an existing project",
            path.display()
        );
    }
    path.canonicalize()
        .with_context(|| format!("cannot resolve integration directory {}", path.display()))
}

fn codex_changes(directory: &Path) -> Result<Vec<FileChange>> {
    let skill = skill_change(directory)?;
    let path = directory.join(".codex/config.toml");
    let before = read_text(&path)?;
    let after = add_codex_server(&before, &path)?;
    Ok(changed([
        skill,
        FileChange {
            path,
            before,
            after,
        },
    ]))
}

fn opencode_changes(directory: &Path) -> Result<Vec<FileChange>> {
    let jsonc = directory.join("opencode.jsonc");
    if jsonc.exists() {
        bail!(
            "{} exists, and agd does not rewrite JSONC (its comments would be lost); add this to its \"mcp\" object by hand:\n\"agd\": {}",
            jsonc.display(),
            opencode_entry()
        );
    }
    let skill = skill_change(directory)?;
    let path = directory.join("opencode.json");
    let before = read_text(&path)?;
    let after = add_opencode_server(&before, &path)?;
    Ok(changed([
        skill,
        FileChange {
            path,
            before,
            after,
        },
    ]))
}

fn changed(changes: [FileChange; 2]) -> Vec<FileChange> {
    changes
        .into_iter()
        .filter(|change| change.before != change.after)
        .collect()
}

/// A file's text, empty when it does not exist. Any other read failure,
/// such as a file that is not UTF-8, is an error: treating it as empty
/// would replace the person's config with agd's entry alone.
fn read_text(path: &Path) -> Result<String> {
    match fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes)
            .with_context(|| format!("{} is not UTF-8; agd leaves it alone", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error).with_context(|| format!("cannot read {}", path.display())),
    }
}

/// The entry `add_codex_server` writes, as the file then shows it.
const CODEX_ENTRY: &str = "[mcp_servers.agd]\ncommand = \"agd\"\nargs = [\"mcp\"]\n";

/// True when `entry` is exactly agd's: `command = "agd"`, `args = ["mcp"]`
/// and nothing else, however the file spells it.
fn is_codex_entry(entry: &Item) -> bool {
    let Some(entry) = entry.as_table_like() else {
        return false;
    };
    let args: Option<Vec<&str>> = entry
        .get("args")
        .and_then(Item::as_array)
        .map(|args| args.iter().filter_map(|arg| arg.as_str()).collect());
    entry.len() == 2
        && entry.get("command").and_then(Item::as_str) == Some("agd")
        && args == Some(vec!["mcp"])
}

/// An existing `mcp_servers.agd` entry as the person wrote it.
fn shown_codex_entry(entry: &Item) -> String {
    let mut servers = Table::new();
    servers.set_implicit(true);
    servers.insert("agd", entry.clone());
    let mut shown = DocumentMut::new();
    shown.insert("mcp_servers", Item::Table(servers));
    shown.to_string().trim_start().to_string()
}

/// `source` with `[mcp_servers.agd]` added and every other byte kept; the
/// same text when agd's entry is already there; an error when an `agd`
/// entry differs.
fn add_codex_server(source: &str, path: &Path) -> Result<String> {
    let mut document: DocumentMut = source
        .parse()
        .with_context(|| format!("cannot parse {}", path.display()))?;
    if let Some(existing) = document
        .get("mcp_servers")
        .and_then(|servers| servers.get("agd"))
    {
        if is_codex_entry(existing) {
            return Ok(source.to_string());
        }
        bail!(
            "{} already has a different agd server; fix or remove it by hand, then install again.\nit has:\n{}agd would add:\n{CODEX_ENTRY}",
            path.display(),
            shown_codex_entry(existing)
        );
    }
    let mut agd = Table::new();
    agd.insert("command", value("agd"));
    agd.insert("args", value(Array::from_iter(["mcp"])));
    if !source.trim().is_empty() {
        // A blank line between the person's last line and agd's table.
        agd.decor_mut().set_prefix("\n");
    }
    let servers = document.as_table_mut().entry("mcp_servers").or_insert({
        let mut servers = Table::new();
        servers.set_implicit(true);
        Item::Table(servers)
    });
    match servers {
        Item::Table(servers) => {
            servers.insert("agd", Item::Table(agd));
        }
        Item::Value(toml_edit::Value::InlineTable(servers)) => {
            servers.insert(
                "agd",
                toml_edit::Value::InlineTable(agd.into_inline_table()),
            );
        }
        _ => bail!("{}: mcp_servers must be a table", path.display()),
    }
    Ok(document.to_string())
}

fn opencode_entry() -> Value {
    json!({"type": "local", "command": ["agd", "mcp"], "enabled": true})
}

/// `source` with `mcp.agd` added and its keys kept in order; the same text
/// when agd's entry is already there; an error when an `agd` entry differs.
fn add_opencode_server(source: &str, path: &Path) -> Result<String> {
    let mut root: Map<String, Value> = if source.trim().is_empty() {
        Map::new()
    } else {
        serde_json::from_str::<Value>(source)
            .with_context(|| format!("cannot parse {}", path.display()))?
            .as_object()
            .cloned()
            .ok_or_else(|| anyhow!("{} must contain a JSON object", path.display()))?
    };
    let wanted = opencode_entry();
    match root.get("mcp").and_then(|servers| servers.get("agd")) {
        Some(existing) if *existing == wanted => return Ok(source.to_string()),
        Some(existing) => bail!(
            "{} already has a different agd server; fix or remove it by hand, then install again.\nit has:\n\"agd\": {existing}\nagd would add:\n\"agd\": {wanted}",
            path.display()
        ),
        None => {}
    }
    root.entry("mcp".to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| anyhow!("{}: mcp must be a JSON object", path.display()))?
        .insert("agd".to_string(), wanted);
    Ok(format!("{}\n", serde_json::to_string_pretty(&root)?))
}

/// `agenda` is a common name, so a skill already there is replaced only when
/// it is agd's own (any version says it works through "the agd MCP server");
/// anyone else's is refused, before anything is written.
fn skill_change(directory: &Path) -> Result<FileChange> {
    let path = directory.join(SKILL_PATH);
    let before = read_text(&path)?;
    if !before.is_empty() && !before.contains("the agd MCP server") {
        bail!(
            "{} is another skill named agenda; move it aside, then install again",
            path.display()
        );
    }
    Ok(FileChange {
        path,
        before,
        after: SKILL.to_string(),
    })
}

/// A unified diff of one change, `/dev/null` standing for a new file.
fn diff(change: &FileChange) -> String {
    let path = change.path.display().to_string();
    let old = if change.before.is_empty() {
        "/dev/null".to_string()
    } else {
        path.clone()
    };
    TextDiff::from_lines(&change.before, &change.after)
        .unified_diff()
        .header(&old, &path)
        .to_string()
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
    use clap::{FromArgMatches, Parser};

    use super::*;

    /// Runs `agd setup install` as the command line would, minus printing.
    fn install(directory: &Path, target: &str) -> Result<()> {
        let command = parse(&[
            "install",
            target,
            "--directory",
            directory.to_str().unwrap(),
        ]);
        command.execute()
    }

    fn parse(arguments: &[&str]) -> InstallCommand {
        #[derive(Parser)]
        struct Cli {
            #[command(flatten)]
            install: InstallCommand,
        }
        let matches = <Cli as clap::CommandFactory>::command()
            .try_get_matches_from(arguments)
            .unwrap();
        InstallCommand::from_arg_matches(&matches).unwrap()
    }

    #[test]
    fn directory_and_dry_run_go_before_or_after_the_target() {
        let after = parse(&["install", "codex", "--directory", "proj", "--dry-run"]);
        assert_eq!(after.directory, PathBuf::from("proj"));
        assert!(after.dry_run);
        assert!(matches!(after.target, InstallTarget::Codex));
        let before = parse(&["install", "--directory", "proj", "opencode"]);
        assert_eq!(before.directory, PathBuf::from("proj"));
        assert!(!before.dry_run);
    }

    #[test]
    fn a_missing_directory_is_refused_not_created() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("proj");
        let error = install(&missing, "codex").unwrap_err().to_string();
        assert!(error.contains("does not exist"), "{error}");
        assert!(!missing.exists());
    }

    #[test]
    fn installs_the_skill_and_replaces_only_agds_own() {
        let directory = tempfile::tempdir().unwrap();
        install(directory.path(), "codex").unwrap();
        let skill = directory.path().join(SKILL_PATH);
        assert_eq!(fs::read_to_string(&skill).unwrap(), SKILL);

        // An older agd skill is replaced.
        fs::write(
            &skill,
            "---\nname: agenda\ndescription: through the agd MCP server.\n---\n",
        )
        .unwrap();
        install(directory.path(), "opencode").unwrap();
        assert_eq!(fs::read_to_string(&skill).unwrap(), SKILL);
    }

    #[test]
    fn refuses_someone_elses_agenda_skill_before_writing_anything() {
        let directory = tempfile::tempdir().unwrap();
        let skill = directory.path().join(SKILL_PATH);
        fs::create_dir_all(skill.parent().unwrap()).unwrap();
        let theirs = "---\nname: agenda\ndescription: My meeting agendas.\n---\n";
        fs::write(&skill, theirs).unwrap();

        let error = install(directory.path(), "codex").unwrap_err().to_string();
        assert!(error.contains("is another skill named agenda"), "{error}");
        assert_eq!(fs::read_to_string(&skill).unwrap(), theirs);
        assert!(!directory.path().join(".codex/config.toml").exists());

        assert!(install(directory.path(), "opencode").is_err());
        assert!(!directory.path().join("opencode.json").exists());
    }

    #[test]
    fn codex_config_keeps_comments_and_order_and_gains_only_agd() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(".codex/config.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let theirs = "# my Codex settings\nmodel = \"o3\"  # the good one\n\n[mcp_servers.other]\ncommand = \"other\"\n";
        fs::write(&path, theirs).unwrap();

        install(directory.path(), "codex").unwrap();
        let installed = fs::read_to_string(&path).unwrap();
        assert_eq!(
            installed,
            format!("{theirs}\n[mcp_servers.agd]\ncommand = \"agd\"\nargs = [\"mcp\"]\n")
        );

        // Again: nothing changes.
        install(directory.path(), "codex").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), installed);
    }

    #[test]
    fn codex_config_that_is_not_utf8_is_left_alone() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(".codex/config.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let theirs = b"model = \"o3\"\n# caf\xe9\n".to_vec();
        fs::write(&path, &theirs).unwrap();

        let error = install(directory.path(), "codex").unwrap_err().to_string();
        assert!(error.contains("is not UTF-8"), "{error}");
        assert_eq!(fs::read(&path).unwrap(), theirs);
        assert!(!directory.path().join(SKILL_PATH).exists());
    }

    #[test]
    fn a_different_agd_entry_is_refused_with_both_entries() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(".codex/config.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let theirs = "[mcp_servers.agd]\ncommand = \"/opt/agd\"\nargs = [\"mcp\"]\n";
        fs::write(&path, theirs).unwrap();
        let error = install(directory.path(), "codex").unwrap_err().to_string();
        assert!(
            error.contains("it has:\n[mcp_servers.agd]\ncommand = \"/opt/agd\""),
            "{error}"
        );
        assert!(
            error.contains("agd would add:\n[mcp_servers.agd]\ncommand = \"agd\""),
            "{error}"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), theirs);

        let path = directory.path().join("opencode.json");
        let theirs = "{\"mcp\": {\"agd\": {\"type\": \"local\", \"command\": [\"agd\"]}}}\n";
        fs::write(&path, theirs).unwrap();
        let error = install(directory.path(), "opencode")
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("it has:\n\"agd\": {\"type\":\"local\""),
            "{error}"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), theirs);
    }

    #[test]
    fn opencode_config_keeps_its_key_order_and_installs_once() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("opencode.json");
        fs::write(
            &path,
            "{\"theme\": \"tokyonight\", \"$schema\": \"https://opencode.ai/config.json\", \"mcp\": {\"zeta\": {\"type\": \"local\"}}}",
        )
        .unwrap();
        install(directory.path(), "opencode").unwrap();
        let installed = fs::read_to_string(&path).unwrap();
        let keys: Vec<&str> = ["\"theme\"", "\"$schema\"", "\"mcp\"", "\"zeta\"", "\"agd\""]
            .into_iter()
            .collect();
        let positions: Vec<usize> = keys
            .iter()
            .map(|key| installed.find(key).unwrap())
            .collect();
        assert!(positions.is_sorted(), "{installed}");

        install(directory.path(), "opencode").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), installed);
    }

    #[test]
    fn opencode_jsonc_is_refused_with_the_entry_to_add() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("opencode.jsonc"), "{ // mine\n}\n").unwrap();
        let error = install(directory.path(), "opencode")
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(
                "add this to its \"mcp\" object by hand:\n\"agd\": {\"type\":\"local\",\"command\":[\"agd\",\"mcp\"],\"enabled\":true}"
            ),
            "{error}"
        );
        assert!(!directory.path().join("opencode.json").exists());
        assert!(!directory.path().join(SKILL_PATH).exists());
    }

    #[test]
    fn dry_run_writes_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let command = parse(&[
            "install",
            "codex",
            "--dry-run",
            "--directory",
            directory.path().to_str().unwrap(),
        ]);
        command.execute().unwrap();
        assert!(!directory.path().join(".codex").exists());
        assert!(!directory.path().join(".agents").exists());

        let changes = codex_changes(directory.path()).unwrap();
        let config = diff(&changes[1]);
        assert!(config.starts_with("--- /dev/null\n+++ "), "{config}");
        assert!(config.contains("+[mcp_servers.agd]\n"), "{config}");
    }
}
