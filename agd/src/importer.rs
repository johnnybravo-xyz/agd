use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use agd_core::{
    AgendaEngine, AgendaQuery, ConfigEditor, ConfigSources, ImportResult, ImportStatus,
    ResolvedEditorConfig, Settings, WorkspaceIndex, agenda_csv, apply_import, find_config_sources,
    items_csv, render_config,
};
use anyhow::{Context, Result, anyhow, bail};
use chrono::Local;
use similar::TextDiff;

const EMACS_READER: &str = include_str!("../readers/emacs.el");
const NEOVIM_READER: &str = include_str!("../readers/nvim.lua");
/// How long an editor may take to read the config and, with --verify, run
/// every imported view.
const READER_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone, Debug)]
pub struct VerificationResult {
    pub status: ImportStatus,
    pub key: String,
    pub description: String,
    pub detail: String,
}

#[derive(Clone, Debug)]
pub struct ImportExecution {
    pub sources: ConfigSources,
    pub result: ImportResult,
    pub toml: String,
    pub diff: String,
    pub verification: Vec<VerificationResult>,
}

/// Reads the editor config under `paths` and applies it over `base`. The
/// files are read by a clean editor (`emacs -Q`, `nvim --clean`) running
/// agd's reader, never by the user's own startup.
pub fn import_config(
    paths: &[PathBuf],
    base: &Settings,
    derive_roots: bool,
    destination: &Path,
    write: bool,
    verify: bool,
) -> Result<ImportExecution> {
    let sources = find_config_sources(paths)?;
    let files = sources
        .files
        .iter()
        .map(|file| file.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    let mut command = match sources.editor {
        ConfigEditor::Emacs => {
            let reader = reader_file("emacs.el", EMACS_READER)?;
            let mut command = Command::new("emacs");
            command.args(["-Q", "--batch", "-l"]).arg(reader.path());
            if verify {
                command.env("AGD_IMPORT_VERIFY", "1");
            }
            (command, reader)
        }
        ConfigEditor::Neovim => {
            let reader = reader_file("nvim.lua", NEOVIM_READER)?;
            let mut command = Command::new("nvim");
            command
                .args(["--clean", "--headless", "-l"])
                .arg(reader.path());
            command.env("AGD_IMPORT_ROOT", neovim_config_root(&sources.files[0]));
            (command, reader)
        }
    };
    command.0.env("AGD_IMPORT_FILES", files);
    let label = format!("{} config reader", sources.editor.name());
    let output = run(&mut command.0, &label, READER_TIMEOUT)?;
    let resolved = parse_process_json(&output, sources.editor.name())?;
    finish_import(
        sources,
        resolved,
        base,
        derive_roots,
        destination,
        write,
        verify,
    )
}

/// The Neovim config directory a file belongs to, so `require("x")` finds
/// `lua/x.lua`: the directory above `lua/`, or the file's own directory.
fn neovim_config_root(file: &Path) -> PathBuf {
    file.ancestors()
        .find(|ancestor| ancestor.file_name().is_some_and(|name| name == "lua"))
        .and_then(Path::parent)
        .or_else(|| file.parent())
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

/// Runs the reader with no stdin, so an editor prompt reads end of file
/// instead of waiting, and stops it after `timeout`.
fn run(command: &mut Command, label: &str, timeout: Duration) -> Result<Output> {
    tracing::info!(?command, ?timeout, "starting editor config reader");
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("cannot start {label}; is the editor on PATH?"))?;
    // Both pipes drain on their own threads, so a full pipe never stalls
    // the child.
    let stdout = read_to_end_in_background(child.stdout.take());
    let stderr = read_to_end_in_background(child.stderr.take());
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .with_context(|| format!("cannot wait for {label}"))?
        {
            break status;
        }
        if Instant::now() >= deadline {
            tracing::warn!(?timeout, "stopping editor config reader");
            // Kill fails only when the child has already exited.
            let _ = child.kill();
            let _ = child.wait();
            bail!(
                "{label} did not finish within {timeout:?} and was stopped; an editor prompt or, with --verify, a slow agenda view can cause this"
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let output = Output {
        status,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    };
    tracing::info!(status = %output.status, "editor config reader finished");
    if !output.status.success() {
        bail!(
            "{label} failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output)
}

fn read_to_end_in_background(
    pipe: Option<impl Read + Send + 'static>,
) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            // A read error leaves what was read; the exit status reports why.
            let _ = pipe.read_to_end(&mut bytes);
        }
        bytes
    })
}

fn parse_process_json(output: &Output, editor: &str) -> Result<ResolvedEditorConfig> {
    parse_marked_json(&output.stdout, editor).or_else(|_| parse_marked_json(&output.stderr, editor))
}

fn parse_marked_json(bytes: &[u8], editor: &str) -> Result<ResolvedEditorConfig> {
    let output = String::from_utf8(bytes.to_vec())
        .with_context(|| format!("{editor} reader output is not UTF-8"))?;
    let marker = "AGD_JSON:";
    let payload = output
        .lines()
        .find_map(|line| line.strip_prefix(marker))
        .ok_or_else(|| anyhow!("{editor} reader produced no {marker} payload"))?;
    serde_json::from_str(payload)
        .with_context(|| format!("{editor} reader returned an unsupported payload shape"))
}

fn finish_import(
    sources: ConfigSources,
    mut resolved: ResolvedEditorConfig,
    base: &Settings,
    derive_roots: bool,
    destination: &Path,
    write: bool,
    verify: bool,
) -> Result<ImportExecution> {
    let editor_rows = std::mem::take(&mut resolved.verification);
    let result = apply_import(&resolved, base, derive_roots)?;
    let verification = verify_import(&result, &resolved.editor, &editor_rows, verify);
    let read: Vec<String> = sources
        .files
        .iter()
        .map(|file| file.to_string_lossy().into_owned())
        .collect();
    let header = format!(
        "agd configuration, imported by `agd setup import` from:\n  {}\nEdit freely; rerun the import to start over. Keys are described at\nhttps://github.com/johnnybravo-xyz/agd#configuration",
        read.join("\n  ")
    );
    let toml = render_config(&result.settings, &header)?;
    let diff = config_diff(destination, &toml)?;
    // An unchanged config is left alone.
    if write && !diff.is_empty() {
        write_config(destination, &toml)?;
    }
    Ok(ImportExecution {
        sources,
        result,
        toml,
        diff,
        verification,
    })
}

/// A unified diff from the config at `destination` (/dev/null when there is
/// none yet) to `toml`, headed by the real path; empty when they match.
fn config_diff(destination: &Path, toml: &str) -> Result<String> {
    let path = destination.display().to_string();
    let (previous, old_label) = match fs::read_to_string(destination) {
        Ok(previous) => (previous, path.clone()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (String::new(), "/dev/null".to_string())
        }
        Err(error) => return Err(error).with_context(|| format!("cannot read {path}")),
    };
    Ok(TextDiff::from_lines(previous.as_str(), toml)
        .unified_diff()
        .header(&old_label, &path)
        .to_string())
}

/// Replaces the config file atomically with owner-only permissions.
fn write_config(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    let temporary = path.with_extension("toml.tmp");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .with_context(|| format!("cannot write {}", temporary.display()))?;
    file.write_all(contents.as_bytes())
        .and_then(|()| file.sync_all())
        .with_context(|| format!("cannot write {}", temporary.display()))?;
    fs::rename(&temporary, path).with_context(|| format!("cannot replace {}", path.display()))?;
    Ok(())
}

fn verify_import(
    result: &ImportResult,
    editor: &str,
    editor_rows: &std::collections::BTreeMap<String, String>,
    verify: bool,
) -> Vec<VerificationResult> {
    if !verify {
        return Vec::new();
    }
    let settings = result.settings.clone();
    let today = Local::now().date_naive();
    let loaded = WorkspaceIndex::load(settings.clone())
        .and_then(|index| Ok((index, settings.agenda_start(today)?)));
    let (index, start) = match loaded {
        Ok(loaded) => loaded,
        Err(error) => {
            return vec![VerificationResult {
                status: ImportStatus::Unsupported,
                key: "a".to_string(),
                description: "Agenda".to_string(),
                detail: format!("native verification failed: {error}"),
            }];
        }
    };
    let engine = AgendaEngine::new(&index);
    let (from, to) = settings.agenda_span.bounds(start, settings.week_start);
    let mut results = Vec::new();
    let native_default = engine
        .agenda(&AgendaQuery {
            from,
            to,
            today,
            match_expression: None,
            log_mode: false,
        })
        .map(|entries| normalize_csv(&agenda_csv(&entries, today, &index)));
    if editor == "emacs" {
        results.push(compare_rows(
            "a",
            "Agenda",
            editor_rows.get("a"),
            native_default,
        ));
    } else {
        results.push(match native_default {
            Ok(_) => VerificationResult {
                status: ImportStatus::Difference,
                key: "a".to_string(),
                description: "Agenda".to_string(),
                detail: "native rows evaluated; nvim-orgmode has no stable row-export API, so the pinned Emacs oracle is the semantic reference".to_string(),
            },
            Err(error) => VerificationResult {
                status: ImportStatus::Unsupported,
                key: "a".to_string(),
                description: "Agenda".to_string(),
                detail: error.to_string(),
            },
        });
    }

    for view in &settings.views {
        let key = &view.key;
        if editor != "emacs" {
            results.push(VerificationResult {
                status: ImportStatus::Difference,
                key: key.clone(),
                description: view.desc.clone(),
                detail: "view imported; nvim-orgmode has no stable row-export API".to_string(),
            });
            continue;
        }
        // The pinned Emacs has no org-super-agenda, so compare the rows
        // before grouping.
        let mut ungrouped = view.clone();
        for block in &mut ungrouped.blocks {
            block.groups.clear();
        }
        let native = engine
            .view(&ungrouped, start, today, false)
            .map(|sections| {
                let rows: String = sections
                    .iter()
                    .map(|section| {
                        agenda_csv(&section.entries, today, &index)
                            + &items_csv(&section.items, &section.kind, &index)
                    })
                    .collect();
                normalize_csv(&rows)
            });
        results.push(compare_rows(key, &view.desc, editor_rows.get(key), native));
    }
    results
}

fn compare_rows(
    key: &str,
    description: &str,
    editor: Option<&String>,
    native: std::result::Result<
        std::result::Result<Vec<Vec<String>>, csv::Error>,
        agd_core::AgdError,
    >,
) -> VerificationResult {
    let native = match native {
        Ok(Ok(rows)) => rows,
        Ok(Err(error)) => {
            return VerificationResult {
                status: ImportStatus::Unsupported,
                key: key.to_string(),
                description: description.to_string(),
                detail: format!("cannot normalize native rows: {error}"),
            };
        }
        Err(error) => {
            return VerificationResult {
                status: ImportStatus::Unsupported,
                key: key.to_string(),
                description: description.to_string(),
                detail: format!("native view failed: {error}"),
            };
        }
    };
    let Some(editor) = editor else {
        return VerificationResult {
            status: ImportStatus::Unsupported,
            key: key.to_string(),
            description: description.to_string(),
            detail: "editor endpoint returned no verification rows".to_string(),
        };
    };
    if let Some(error) = editor.strip_prefix("AGD_VERIFY_ERROR:") {
        return VerificationResult {
            status: ImportStatus::Unsupported,
            key: key.to_string(),
            description: description.to_string(),
            detail: format!("editor view failed: {error}"),
        };
    }
    match normalize_csv(editor) {
        Ok(editor) if editor == native => VerificationResult {
            status: ImportStatus::Exact,
            key: key.to_string(),
            description: description.to_string(),
            detail: format!("{} normalized rows match the editor", native.len()),
        },
        Ok(editor) => VerificationResult {
            status: ImportStatus::Difference,
            key: key.to_string(),
            description: description.to_string(),
            detail: format!(
                "row difference: editor returned {}, native returned {}",
                editor.len(),
                native.len()
            ),
        },
        Err(error) => VerificationResult {
            status: ImportStatus::Unsupported,
            key: key.to_string(),
            description: description.to_string(),
            detail: format!("cannot normalize editor rows: {error}"),
        },
    }
}

fn normalize_csv(value: &str) -> std::result::Result<Vec<Vec<String>>, csv::Error> {
    const KINDS: [&str; 9] = [
        "scheduled",
        "past-scheduled",
        "deadline",
        "upcoming-deadline",
        "timestamp",
        "block",
        "closed",
        "todo",
        "tagsmatch",
    ];
    let mut rows = Vec::new();
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(value.as_bytes());
    for record in reader.records() {
        let record = record?;
        if record.len() < 9 || !KINDS.contains(&record.get(2).unwrap_or_default()) {
            continue;
        }
        let mut row: Vec<String> = record.iter().map(str::to_string).collect();
        if row.len() > 10 {
            row[5] = row[10].clone();
        }
        row[6] = row[6].trim_end_matches('.').to_string();
        if row[2] == "past-scheduled" && row[7] == "Scheduled:" {
            row[2] = "scheduled".to_string();
        }
        if row[2] == "tagsmatch" {
            row[2] = "todo".to_string();
        }
        row.truncate(9);
        rows.push(row);
    }
    rows.sort_by(|left, right| {
        let rank = |kind: &str| match kind {
            "past-scheduled" | "scheduled" => 0,
            "deadline" => 1,
            "block" => 2,
            "timestamp" => 3,
            "upcoming-deadline" => 4,
            "closed" => 5,
            _ => 6,
        };
        left[5]
            .cmp(&right[5])
            .then_with(|| left[6].is_empty().cmp(&right[6].is_empty()))
            .then_with(|| left[6].cmp(&right[6]))
            .then_with(|| {
                left[8]
                    .as_str()
                    .if_empty("B")
                    .cmp(right[8].as_str().if_empty("B"))
            })
            .then_with(|| rank(&left[2]).cmp(&rank(&right[2])))
            .then_with(|| left[0].cmp(&right[0]))
            .then_with(|| left[1].cmp(&right[1]))
    });
    Ok(rows)
}

trait EmptyDefault {
    fn if_empty<'a>(&'a self, default: &'a str) -> &'a str;
}

impl EmptyDefault for str {
    fn if_empty<'a>(&'a self, default: &'a str) -> &'a str {
        if self.is_empty() { default } else { self }
    }
}

fn reader_file(name: &str, contents: &str) -> Result<tempfile::NamedTempFile> {
    let suffix = Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("reader");
    let mut reader = tempfile::Builder::new()
        .prefix("agd-import-")
        .suffix(&format!(".{suffix}"))
        .tempfile()
        .context("cannot create the editor config reader")?;
    reader
        .write_all(contents.as_bytes())
        .context("cannot write the editor config reader")?;
    reader
        .flush()
        .context("cannot write the editor config reader")?;
    Ok(reader)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_reader_that_never_finishes_is_stopped_with_an_error() {
        let started = Instant::now();
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 30"]);
        let error = run(&mut command, "test reader", Duration::from_millis(200)).unwrap_err();
        assert_eq!(
            error.to_string(),
            "test reader did not finish within 200ms and was stopped; an editor prompt or, with --verify, a slow agenda view can cause this"
        );
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[cfg(unix)]
    #[test]
    fn a_reader_prompt_reads_end_of_file() {
        let mut command = Command::new("sh");
        command.args(["-c", "read answer; echo \"answer:$answer\""]);
        let output = run(&mut command, "test reader", Duration::from_secs(10)).unwrap();
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "answer:\n");
    }

    #[test]
    fn config_diff_names_the_real_path_and_is_empty_when_unchanged() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("config.toml");
        let path = destination.display();
        assert_eq!(
            config_diff(&destination, "roots = [\"~/org\"]\n").unwrap(),
            format!("--- /dev/null\n+++ {path}\n@@ -0,0 +1 @@\n+roots = [\"~/org\"]\n")
        );
        fs::write(&destination, "roots = [\"~/org\"]\n").unwrap();
        assert_eq!(
            config_diff(&destination, "roots = [\"~/notes\"]\n").unwrap(),
            format!(
                "--- {path}\n+++ {path}\n@@ -1 +1 @@\n-roots = [\"~/org\"]\n+roots = [\"~/notes\"]\n"
            )
        );
        assert_eq!(
            config_diff(&destination, "roots = [\"~/org\"]\n").unwrap(),
            ""
        );
    }
}
