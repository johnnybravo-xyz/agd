//! `agd setup install [--detected] [codex] [opencode] [pi] [omp]`: installs
//! agd for coding agents, user-wide. The `agenda` skill goes to
//! `~/.agents/skills`, the Agent Skills location that Codex, OpenCode, Pi,
//! oh-my-pi and Gemini CLI all read; each named or detected harness also gets
//! agd's MCP server in its user config. Pi and oh-my-pi also get `/capture`
//! from agd's Pi extension. Claude Code uses agd's plugin, while Paseo reuses
//! the setup of the provider it launches. Each person-owned config file is
//! edited, not rewritten: Codex's TOML through toml_edit (comments and order
//! survive), the JSON configs with their keys in their order. An existing agd
//! entry that differs, or a file agd cannot edit safely, is refused before
//! anything is written.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, ValueEnum};
use serde_json::{Map, Value, json};
use similar::TextDiff;
use toml_edit::{Array, DocumentMut, Item, Table, value};

use crate::words;

/// The one copy of the skill, shared with the Claude Code plugin.
const SKILL: &str = include_str!("../../claude-plugin/skills/agenda/SKILL.md");

/// Pi's and oh-my-pi's `/capture`, which runs `agd capture` without a
/// model turn.
const PI_EXTENSION: &str = include_str!("../../pi/agd.ts");

/// Claude Code gets agd from its plugin (`claude plugin install
/// agd@johnnybravo-xyz`), so it is not a harness here.
#[derive(Args, Debug)]
pub struct InstallCommand {
    /// Print what would change as a diff and write nothing.
    #[arg(long)]
    dry_run: bool,
    /// Select clients whose user config directories already exist.
    #[arg(long)]
    detected: bool,
    /// Harnesses that also get agd's MCP server in their user config. The
    /// skill alone goes to ~/.agents/skills for every harness.
    #[arg(value_enum)]
    harnesses: Vec<Harness>,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Harness {
    /// Codex: ~/.codex/config.toml, or $CODEX_HOME/config.toml
    Codex,
    /// OpenCode: ~/.config/opencode/opencode.json, or under $XDG_CONFIG_HOME
    Opencode,
    /// Pi: ~/.pi/agent/mcp.json and the /capture extension, or under $PI_CODING_AGENT_DIR
    Pi,
    /// oh-my-pi: ~/.omp/agent/mcp.json and the /capture extension
    Omp,
}

/// Where the user-wide files live, read from the environment once so tests
/// can pass their own.
#[derive(Debug)]
struct Places {
    skill: PathBuf,
    codex: PathBuf,
    /// OpenCode's config directory; its file is opencode.json, or
    /// opencode.jsonc, which agd refuses to rewrite.
    opencode: PathBuf,
    pi: PathBuf,
    pi_extension: PathBuf,
    omp: PathBuf,
    omp_extension: PathBuf,
}

impl Places {
    fn new(
        home: &Path,
        codex_home: Option<PathBuf>,
        xdg_config_home: Option<PathBuf>,
        pi_agent_dir: Option<PathBuf>,
    ) -> Self {
        let pi = pi_agent_dir.unwrap_or_else(|| home.join(".pi/agent"));
        Self {
            skill: home.join(".agents/skills/agenda/SKILL.md"),
            codex: codex_home
                .unwrap_or_else(|| home.join(".codex"))
                .join("config.toml"),
            opencode: xdg_config_home
                .unwrap_or_else(|| home.join(".config"))
                .join("opencode"),
            pi: pi.join("mcp.json"),
            pi_extension: pi.join("extensions/agd.ts"),
            omp: home.join(".omp/agent/mcp.json"),
            omp_extension: home.join(".omp/agent/extensions/agd.ts"),
        }
    }

    fn from_env() -> Result<Self> {
        let home =
            dirs::home_dir().ok_or_else(|| anyhow!("cannot determine the home directory"))?;
        Ok(Self::new(
            &home,
            absolute_variable("CODEX_HOME"),
            absolute_variable("XDG_CONFIG_HOME"),
            absolute_variable("PI_CODING_AGENT_DIR"),
        ))
    }
}

/// Harnesses whose user config directory or file already exists. Detection is
/// deliberately filesystem-only and does not inspect config contents.
fn detected_harnesses(places: &Places) -> Vec<Harness> {
    let file_or_parent_exists =
        |path: &Path| path.exists() || path.parent().is_some_and(Path::exists);
    let mut harnesses = Vec::new();
    if file_or_parent_exists(&places.codex) {
        harnesses.push(Harness::Codex);
    }
    if places.opencode.exists() {
        harnesses.push(Harness::Opencode);
    }
    if file_or_parent_exists(&places.pi) {
        harnesses.push(Harness::Pi);
    }
    if file_or_parent_exists(&places.omp) {
        harnesses.push(Harness::Omp);
    }
    harnesses
}

/// An environment variable naming a directory, ignored when it is relative,
/// as the XDG Base Directory spec says.
fn absolute_variable(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
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
        let places = Places::from_env()?;
        for line in install_selected(&places, &self.harnesses, self.detected, self.dry_run)? {
            println!("{line}");
        }
        Ok(())
    }
}

fn install_selected(
    places: &Places,
    harnesses: &[Harness],
    detected: bool,
    dry_run: bool,
) -> Result<Vec<String>> {
    let mut selected = harnesses.to_vec();
    if detected {
        selected.extend(detected_harnesses(places));
        if selected.is_empty() {
            bail!(
                "no supported agent config directories found; pass one or more of: codex, opencode, pi, omp"
            );
        }
    }
    install(places, &selected, dry_run)
}

/// Writes the skill and each harness's entry, unless `dry_run`, and returns
/// what to print. Every change is computed before the first write, so a
/// refusal leaves every file as it was.
fn install(places: &Places, harnesses: &[Harness], dry_run: bool) -> Result<Vec<String>> {
    let mut harnesses = harnesses.to_vec();
    harnesses.sort();
    harnesses.dedup();
    let mut changes = vec![skill_change(&places.skill)?];
    for harness in &harnesses {
        changes.push(server_change(places, *harness)?);
        let extension = match harness {
            Harness::Pi => Some(&places.pi_extension),
            Harness::Omp => Some(&places.omp_extension),
            Harness::Codex | Harness::Opencode => None,
        };
        if let Some(path) = extension {
            changes.push(owned_file_change(
                path,
                PI_EXTENSION,
                "agd's Pi extension",
                "another extension named agd.ts",
            )?);
        }
    }
    let mut lines = Vec::new();
    for change in &changes {
        let shown = words::home_path(&change.path.display().to_string());
        if change.before == change.after {
            lines.push(format!("{shown} is up to date"));
        } else if dry_run {
            lines.push(diff(change).trim_end().to_string());
        } else {
            atomic_write(&change.path, change.after.as_bytes())?;
            lines.push(format!("wrote {shown}"));
        }
    }
    if harnesses.is_empty() {
        lines.push(
            "the skill needs agd's MCP server: add it to a harness with `agd setup install codex` (or opencode, pi, omp)"
                .to_string(),
        );
    }
    Ok(lines)
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

/// `agenda` is a common name, so a skill already there is replaced only when
/// it is agd's own (any version says it works through "the agd MCP server").
fn skill_change(path: &Path) -> Result<FileChange> {
    owned_file_change(
        path,
        SKILL,
        "the agd MCP server",
        "another skill named agenda",
    )
}

/// A file agd owns whole, such as the skill: one already there is replaced
/// only when it contains `marker`, so it is agd's own; anyone else's is
/// refused, before anything is written.
fn owned_file_change(path: &Path, content: &str, marker: &str, other: &str) -> Result<FileChange> {
    let before = read_text(path)?;
    if !before.is_empty() && !before.contains(marker) {
        bail!(
            "{} is {other}; move it aside, then install again",
            path.display()
        );
    }
    Ok(FileChange {
        path: path.to_path_buf(),
        before,
        after: content.to_string(),
    })
}

/// The harness's user config with agd's server entry.
fn server_change(places: &Places, harness: Harness) -> Result<FileChange> {
    let path = match harness {
        Harness::Codex => places.codex.clone(),
        Harness::Opencode => {
            let jsonc = places.opencode.join("opencode.jsonc");
            if jsonc.exists() {
                bail!(
                    "{} exists, and agd does not rewrite JSONC (its comments would be lost); add this to its \"mcp\" object by hand:\n\"agd\": {}",
                    jsonc.display(),
                    opencode_entry()
                );
            }
            places.opencode.join("opencode.json")
        }
        Harness::Pi => places.pi.clone(),
        Harness::Omp => places.omp.clone(),
    };
    let before = read_text(&path)?;
    let after = match harness {
        Harness::Codex => add_codex_server(&before, &path)?,
        Harness::Opencode => add_json_server(&before, &path, "mcp", opencode_entry())?,
        Harness::Pi | Harness::Omp => {
            add_json_server(&before, &path, "mcpServers", mcp_servers_entry())?
        }
    };
    Ok(FileChange {
        path,
        before,
        after,
    })
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

/// OpenCode's shape for a local server.
fn opencode_entry() -> Value {
    json!({"type": "local", "command": ["agd", "mcp"], "enabled": true})
}

/// The `mcpServers` shape that Pi, oh-my-pi and most MCP clients share.
fn mcp_servers_entry() -> Value {
    json!({"command": "agd", "args": ["mcp"]})
}

/// `source` with `<servers>.agd` added and its keys kept in order; the same
/// text when agd's entry is already there; an error when an `agd` entry
/// differs.
fn add_json_server(source: &str, path: &Path, servers: &str, wanted: Value) -> Result<String> {
    let mut root: Map<String, Value> = if source.trim().is_empty() {
        Map::new()
    } else {
        serde_json::from_str::<Value>(source)
            .with_context(|| format!("cannot parse {}", path.display()))?
            .as_object()
            .cloned()
            .ok_or_else(|| anyhow!("{} must contain a JSON object", path.display()))?
    };
    match root.get(servers).and_then(|servers| servers.get("agd")) {
        Some(existing) if *existing == wanted => return Ok(source.to_string()),
        Some(existing) => bail!(
            "{} already has a different agd server; fix or remove it by hand, then install again.\nit has:\n\"agd\": {existing}\nagd would add:\n\"agd\": {wanted}",
            path.display()
        ),
        None => {}
    }
    root.entry(servers.to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| anyhow!("{}: {servers} must be a JSON object", path.display()))?
        .insert("agd".to_string(), wanted);
    Ok(format!("{}\n", serde_json::to_string_pretty(&root)?))
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

    fn places(home: &Path) -> Places {
        Places::new(home, None, None, None)
    }

    #[test]
    fn harnesses_are_optional_and_flags_go_anywhere() {
        let none = parse(&["install"]);
        assert!(none.harnesses.is_empty());
        assert!(!none.dry_run);
        assert!(!none.detected);

        let detected = parse(&["install", "--detected"]);
        assert!(detected.harnesses.is_empty());
        assert!(detected.detected);

        let some = parse(&["install", "codex", "--detected", "--dry-run", "pi"]);
        assert_eq!(some.harnesses, [Harness::Codex, Harness::Pi]);
        assert!(some.dry_run);
        assert!(some.detected);
    }

    #[test]
    fn places_follow_each_harness_directory_variable() {
        let home = Path::new("/home/a");
        let default = places(home);
        assert_eq!(
            default.skill,
            Path::new("/home/a/.agents/skills/agenda/SKILL.md")
        );
        assert_eq!(default.codex, Path::new("/home/a/.codex/config.toml"));
        assert_eq!(default.opencode, Path::new("/home/a/.config/opencode"));
        assert_eq!(default.pi, Path::new("/home/a/.pi/agent/mcp.json"));
        assert_eq!(
            default.pi_extension,
            Path::new("/home/a/.pi/agent/extensions/agd.ts")
        );
        assert_eq!(default.omp, Path::new("/home/a/.omp/agent/mcp.json"));
        assert_eq!(
            default.omp_extension,
            Path::new("/home/a/.omp/agent/extensions/agd.ts")
        );

        let moved = Places::new(
            home,
            Some(PathBuf::from("/srv/codex")),
            Some(PathBuf::from("/srv/config")),
            Some(PathBuf::from("/srv/pi")),
        );
        assert_eq!(moved.codex, Path::new("/srv/codex/config.toml"));
        assert_eq!(moved.opencode, Path::new("/srv/config/opencode"));
        assert_eq!(moved.pi, Path::new("/srv/pi/mcp.json"));
        assert_eq!(moved.pi_extension, Path::new("/srv/pi/extensions/agd.ts"));
    }

    #[test]
    fn detection_follows_each_harness_config_directory() {
        let codex_home = tempfile::tempdir().unwrap();
        let codex = places(codex_home.path());
        fs::create_dir_all(codex.codex.parent().unwrap()).unwrap();
        assert_eq!(detected_harnesses(&codex), [Harness::Codex]);

        let opencode_home = tempfile::tempdir().unwrap();
        let opencode = places(opencode_home.path());
        fs::create_dir_all(&opencode.opencode).unwrap();
        assert_eq!(detected_harnesses(&opencode), [Harness::Opencode]);

        let pi_home = tempfile::tempdir().unwrap();
        let pi = places(pi_home.path());
        fs::create_dir_all(pi.pi.parent().unwrap()).unwrap();
        assert_eq!(detected_harnesses(&pi), [Harness::Pi]);

        let omp_home = tempfile::tempdir().unwrap();
        let omp = places(omp_home.path());
        fs::create_dir_all(omp.omp.parent().unwrap()).unwrap();
        assert_eq!(detected_harnesses(&omp), [Harness::Omp]);
    }

    #[test]
    fn detected_and_explicit_harnesses_form_one_deduplicated_install() {
        let home = tempfile::tempdir().unwrap();
        let places = places(home.path());
        fs::create_dir_all(places.codex.parent().unwrap()).unwrap();
        fs::create_dir_all(places.pi.parent().unwrap()).unwrap();

        let lines =
            install_selected(&places, &[Harness::Codex, Harness::Codex], true, true).unwrap();
        assert_eq!(lines.len(), 4, "{lines:?}");
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.contains("config.toml"))
                .count(),
            1,
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|line| line.contains("mcp.json")),
            "{lines:?}"
        );
        assert!(!places.skill.exists());
    }

    #[test]
    fn detected_install_without_a_config_directory_writes_nothing() {
        let home = tempfile::tempdir().unwrap();
        let places = places(home.path());

        let error = install_selected(&places, &[], true, false)
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            "no supported agent config directories found; pass one or more of: codex, opencode, pi, omp"
        );
        assert!(fs::read_dir(home.path()).unwrap().next().is_none());
    }

    #[test]
    fn a_detected_conflict_leaves_every_candidate_unchanged() {
        let home = tempfile::tempdir().unwrap();
        let places = places(home.path());
        fs::create_dir_all(places.codex.parent().unwrap()).unwrap();
        fs::create_dir_all(places.omp.parent().unwrap()).unwrap();
        let codex_before = "# person's Codex config\nmodel = \"o3\"\n";
        let omp_before = "{\"mcpServers\":{\"agd\":{\"command\":\"other\"}}}\n";
        fs::write(&places.codex, codex_before).unwrap();
        fs::write(&places.omp, omp_before).unwrap();

        let error = install_selected(&places, &[], true, false)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("already has a different agd server"),
            "{error}"
        );
        assert_eq!(fs::read_to_string(&places.codex).unwrap(), codex_before);
        assert_eq!(fs::read_to_string(&places.omp).unwrap(), omp_before);
        assert!(!places.skill.exists());
        assert!(!places.omp_extension.exists());
    }

    #[test]
    fn the_skill_alone_serves_every_harness_and_says_what_is_missing() {
        let home = tempfile::tempdir().unwrap();
        let places = places(home.path());
        let lines = install(&places, &[], false).unwrap();
        assert_eq!(fs::read_to_string(&places.skill).unwrap(), SKILL);
        assert!(lines[0].starts_with("wrote "), "{lines:?}");
        assert!(lines[1].contains("agd setup install codex"), "{lines:?}");
        assert!(!places.codex.exists());

        let again = install(&places, &[], false).unwrap();
        assert!(again[0].ends_with("SKILL.md is up to date"), "{again:?}");
    }

    #[test]
    fn replaces_only_agds_own_skill() {
        let home = tempfile::tempdir().unwrap();
        let places = places(home.path());
        fs::create_dir_all(places.skill.parent().unwrap()).unwrap();
        fs::write(
            &places.skill,
            "---\nname: agenda\ndescription: through the agd MCP server.\n---\n",
        )
        .unwrap();
        install(&places, &[Harness::Codex], false).unwrap();
        assert_eq!(fs::read_to_string(&places.skill).unwrap(), SKILL);
    }

    #[test]
    fn refuses_someone_elses_agenda_skill_before_writing_anything() {
        let home = tempfile::tempdir().unwrap();
        let places = places(home.path());
        fs::create_dir_all(places.skill.parent().unwrap()).unwrap();
        let theirs = "---\nname: agenda\ndescription: My meeting agendas.\n---\n";
        fs::write(&places.skill, theirs).unwrap();

        let error = install(&places, &[Harness::Codex, Harness::Pi], false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("is another skill named agenda"), "{error}");
        assert_eq!(fs::read_to_string(&places.skill).unwrap(), theirs);
        assert!(!places.codex.exists());
        assert!(!places.pi.exists());
    }

    #[test]
    fn a_refused_harness_leaves_the_skill_and_the_others_unwritten() {
        let home = tempfile::tempdir().unwrap();
        let places = places(home.path());
        fs::create_dir_all(places.omp.parent().unwrap()).unwrap();
        fs::write(&places.omp, "not json").unwrap();

        let error = install(&places, &[Harness::Codex, Harness::Omp], false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("cannot parse"), "{error}");
        assert!(!places.skill.exists());
        assert!(!places.codex.exists());
    }

    #[test]
    fn codex_config_keeps_comments_and_order_and_gains_only_agd() {
        let home = tempfile::tempdir().unwrap();
        let places = places(home.path());
        fs::create_dir_all(places.codex.parent().unwrap()).unwrap();
        let theirs = "# my Codex settings\nmodel = \"o3\"  # the good one\n\n[mcp_servers.other]\ncommand = \"other\"\n";
        fs::write(&places.codex, theirs).unwrap();

        install(&places, &[Harness::Codex], false).unwrap();
        let installed = fs::read_to_string(&places.codex).unwrap();
        assert_eq!(installed, format!("{theirs}\n{CODEX_ENTRY}"));

        // Again: nothing changes.
        let lines = install(&places, &[Harness::Codex], false).unwrap();
        assert!(lines[1].ends_with("config.toml is up to date"), "{lines:?}");
        assert_eq!(fs::read_to_string(&places.codex).unwrap(), installed);
    }

    #[test]
    fn a_config_that_is_not_utf8_is_left_alone() {
        let home = tempfile::tempdir().unwrap();
        let places = places(home.path());
        fs::create_dir_all(places.codex.parent().unwrap()).unwrap();
        let theirs = b"model = \"o3\"\n# caf\xe9\n".to_vec();
        fs::write(&places.codex, &theirs).unwrap();

        let error = install(&places, &[Harness::Codex], false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("is not UTF-8"), "{error}");
        assert_eq!(fs::read(&places.codex).unwrap(), theirs);
        assert!(!places.skill.exists());
    }

    #[test]
    fn a_different_agd_entry_is_refused_with_both_entries() {
        let home = tempfile::tempdir().unwrap();
        let places = places(home.path());
        fs::create_dir_all(places.codex.parent().unwrap()).unwrap();
        let theirs = "[mcp_servers.agd]\ncommand = \"/opt/agd\"\nargs = [\"mcp\"]\n";
        fs::write(&places.codex, theirs).unwrap();
        let error = install(&places, &[Harness::Codex], false)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("it has:\n[mcp_servers.agd]\ncommand = \"/opt/agd\""),
            "{error}"
        );
        assert!(
            error.contains("agd would add:\n[mcp_servers.agd]\ncommand = \"agd\""),
            "{error}"
        );
        assert_eq!(fs::read_to_string(&places.codex).unwrap(), theirs);

        fs::create_dir_all(places.pi.parent().unwrap()).unwrap();
        let theirs = "{\"mcpServers\": {\"agd\": {\"command\": \"agd\"}}}\n";
        fs::write(&places.pi, theirs).unwrap();
        let error = install(&places, &[Harness::Pi], false)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("it has:\n\"agd\": {\"command\":\"agd\"}\nagd would add:\n\"agd\": {\"command\":\"agd\",\"args\":[\"mcp\"]}"),
            "{error}"
        );
        assert_eq!(fs::read_to_string(&places.pi).unwrap(), theirs);
    }

    #[test]
    fn opencode_config_keeps_its_key_order_and_installs_once() {
        let home = tempfile::tempdir().unwrap();
        let places = places(home.path());
        fs::create_dir_all(&places.opencode).unwrap();
        let path = places.opencode.join("opencode.json");
        fs::write(
            &path,
            "{\"theme\": \"tokyonight\", \"$schema\": \"https://opencode.ai/config.json\", \"mcp\": {\"zeta\": {\"type\": \"local\"}}}",
        )
        .unwrap();
        install(&places, &[Harness::Opencode], false).unwrap();
        let installed = fs::read_to_string(&path).unwrap();
        let positions: Vec<usize> = ["\"theme\"", "\"$schema\"", "\"mcp\"", "\"zeta\"", "\"agd\""]
            .iter()
            .map(|key| installed.find(key).unwrap())
            .collect();
        assert!(positions.is_sorted(), "{installed}");
        assert!(
            installed.contains("\"command\": [\n        \"agd\",\n        \"mcp\"\n      ]"),
            "{installed}"
        );

        install(&places, &[Harness::Opencode], false).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), installed);
    }

    #[test]
    fn opencode_jsonc_is_refused_with_the_entry_to_add() {
        let home = tempfile::tempdir().unwrap();
        let places = places(home.path());
        fs::create_dir_all(&places.opencode).unwrap();
        fs::write(places.opencode.join("opencode.jsonc"), "{ // mine\n}\n").unwrap();
        let error = install(&places, &[Harness::Opencode], false)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(
                "add this to its \"mcp\" object by hand:\n\"agd\": {\"type\":\"local\",\"command\":[\"agd\",\"mcp\"],\"enabled\":true}"
            ),
            "{error}"
        );
        assert!(!places.opencode.join("opencode.json").exists());
        assert!(!places.skill.exists());
    }

    #[test]
    fn pi_and_omp_get_the_shared_mcp_servers_entry_and_capture() {
        let home = tempfile::tempdir().unwrap();
        let places = places(home.path());
        fs::create_dir_all(places.omp.parent().unwrap()).unwrap();
        fs::write(
            &places.omp,
            "{\"mcpServers\": {\"other\": {\"command\": \"other\"}}}\n",
        )
        .unwrap();

        install(&places, &[Harness::Omp, Harness::Pi, Harness::Pi], false).unwrap();
        let expected_pi = "{\n  \"mcpServers\": {\n    \"agd\": {\n      \"command\": \"agd\",\n      \"args\": [\n        \"mcp\"\n      ]\n    }\n  }\n}\n";
        assert_eq!(fs::read_to_string(&places.pi).unwrap(), expected_pi);
        assert_eq!(
            fs::read_to_string(&places.pi_extension).unwrap(),
            PI_EXTENSION
        );
        assert_eq!(
            fs::read_to_string(&places.omp_extension).unwrap(),
            PI_EXTENSION
        );
        let omp = fs::read_to_string(&places.omp).unwrap();
        assert!(
            omp.find("\"other\"").unwrap() < omp.find("\"agd\"").unwrap(),
            "{omp}"
        );
    }

    #[test]
    fn replaces_only_agds_own_pi_extension() {
        let home = tempfile::tempdir().unwrap();
        let places = places(home.path());
        fs::create_dir_all(places.pi_extension.parent().unwrap()).unwrap();
        fs::write(&places.pi_extension, "// agd's Pi extension, older\n").unwrap();
        install(&places, &[Harness::Pi], false).unwrap();
        assert_eq!(
            fs::read_to_string(&places.pi_extension).unwrap(),
            PI_EXTENSION
        );

        let theirs = "export default function () {}\n";
        fs::write(&places.pi_extension, theirs).unwrap();
        let error = install(&places, &[Harness::Pi], false)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("is another extension named agd.ts"),
            "{error}"
        );
        assert_eq!(fs::read_to_string(&places.pi_extension).unwrap(), theirs);
    }

    #[test]
    fn dry_run_shows_diffs_and_writes_nothing() {
        let home = tempfile::tempdir().unwrap();
        let places = places(home.path());
        let lines = install(&places, &[Harness::Codex], true).unwrap();
        assert!(!home.path().join(".agents").exists());
        assert!(!home.path().join(".codex").exists());
        assert!(lines[0].starts_with("--- /dev/null\n+++ "), "{lines:?}");
        assert!(lines[1].contains("+[mcp_servers.agd]\n"), "{lines:?}");
    }
}
