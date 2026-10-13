use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output};

use anyhow::{Context, Result, anyhow, bail};
use chrono::{Duration, Local, NaiveDate};
use org_core::{
    AgendaEngine, AgendaQuery, AgendaSpan, CommandProfile, ImportSnapshot, ImportStatus,
    ResolvedEditorConfig, Settings, WorkspaceIndex, agenda_csv, items_csv, parse_date_input,
};
use similar::TextDiff;

const EMACS_PROBE: &str = include_str!("../probes/emacs.el");
const NVIM_PROBE: &str = include_str!("../probes/nvim.lua");

#[derive(Clone, Debug)]
pub struct VerificationResult {
    pub status: ImportStatus,
    pub key: String,
    pub description: String,
    pub detail: String,
}
#[derive(Clone, Debug)]
pub struct ImportExecution {
    pub snapshot: ImportSnapshot,
    pub toml: String,
    pub diff: String,
    pub verification: Vec<VerificationResult>,
}

pub fn import_emacs(
    settings: &Settings,
    server: Option<&str>,
    batch_profile: Option<&str>,
    destination: &Path,
    write: bool,
    verify: bool,
) -> Result<ImportExecution> {
    let probe = probe_file("emacs.el", EMACS_PROBE)?;
    let resolved = if let Some(profile) = batch_profile {
        let profile = settings
            .imports
            .emacs
            .batch_profiles
            .get(profile)
            .ok_or_else(|| anyhow!("unknown Emacs batch profile: {profile}"))?;
        eprintln!(
            "warning: the batch profile runs your Emacs configuration: {} {}",
            profile.command,
            profile.args.join(" ")
        );
        run_emacs_batch(profile, probe.path(), verify)?
    } else {
        let server = server.or(settings.imports.emacs.server.as_deref());
        run_emacs_daemon(settings, server, probe.path(), verify).context(
            "no usable Emacs daemon; start the selected daemon or choose an explicit --batch-profile",
        )?
    };
    finish_import(resolved, settings, destination, write, verify)
}

pub fn import_nvim(
    settings: &Settings,
    profile_name: Option<&str>,
    destination: &Path,
    write: bool,
    verify: bool,
) -> Result<ImportExecution> {
    let probe = probe_file("nvim.lua", NVIM_PROBE)?;
    let profile = match profile_name {
        Some(name) => settings
            .imports
            .nvim
            .profiles
            .get(name)
            .cloned()
            .ok_or_else(|| anyhow!("unknown Neovim profile: {name}"))?,
        None => CommandProfile {
            command: settings
                .imports
                .nvim
                .command
                .clone()
                .unwrap_or_else(|| "nvim".to_string()),
            args: if settings.imports.nvim.args.is_empty() {
                vec!["--headless".to_string()]
            } else {
                settings.imports.nvim.args.clone()
            },
        },
    };
    let mut command = Command::new(&profile.command);
    command.args(&profile.args).arg("-l").arg(probe.path());
    command.env("AGD_IMPORT_SOURCE", profile_name.unwrap_or("default"));
    let output = run(&mut command, "Neovim import probe")?;
    let resolved = parse_process_json(&output, "Neovim")?;
    finish_import(resolved, settings, destination, write, verify)
}

fn run_emacs_daemon(
    settings: &Settings,
    server: Option<&str>,
    probe: &Path,
    verify: bool,
) -> Result<ResolvedEditorConfig> {
    let import = &settings.imports.emacs;
    let mut command = Command::new(
        import
            .command
            .clone()
            .unwrap_or_else(|| "emacsclient".to_string()),
    );
    command.args(&import.args);
    if let Some(server) = server {
        command.arg("--socket-name").arg(server);
    }
    let quoted_path = serde_json::to_string(&probe.to_string_lossy().as_ref())?;
    let verify_form = if verify {
        "(setq agd-import-verify t) "
    } else {
        ""
    };
    let expression =
        format!("(progn (load {quoted_path} nil nil t) {verify_form}(agd-import-probe))");
    command.arg("--eval").arg(expression);
    command.env("AGD_IMPORT_SOURCE", "daemon");
    let output = run(&mut command, "Emacs daemon probe")?;
    let json: String = serde_json::from_slice(&output.stdout)
        .context("Emacs daemon returned a non-string probe result")?;
    serde_json::from_str(&json).context("Emacs daemon returned an invalid probe payload")
}

fn run_emacs_batch(
    profile: &CommandProfile,
    probe: &Path,
    verify: bool,
) -> Result<ResolvedEditorConfig> {
    let mut command = Command::new(&profile.command);
    command
        .args(&profile.args)
        .arg("--batch")
        .arg("-l")
        .arg(probe);
    command.env("AGD_IMPORT_SOURCE", "batch-profile");
    command.env("AGD_PROBE_STDOUT", "1");
    if verify {
        command.env("AGD_IMPORT_VERIFY", "1");
    }
    let output = run(&mut command, "Emacs batch profile probe")?;
    parse_process_json(&output, "Emacs")
}

fn run(command: &mut Command, label: &str) -> Result<Output> {
    tracing::info!(?command, "starting editor configuration probe");
    let output = command
        .output()
        .with_context(|| format!("cannot start {label}"))?;
    if !output.status.success() {
        bail!(
            "{label} failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output)
}

fn parse_process_json(output: &Output, editor: &str) -> Result<ResolvedEditorConfig> {
    parse_marked_json(&output.stdout, editor).or_else(|_| parse_marked_json(&output.stderr, editor))
}

fn parse_marked_json(bytes: &[u8], editor: &str) -> Result<ResolvedEditorConfig> {
    let output = String::from_utf8(bytes.to_vec())
        .with_context(|| format!("{editor} probe output is not UTF-8"))?;
    let marker = "AGD_JSON:";
    let payload = output
        .lines()
        .find_map(|line| line.strip_prefix(marker))
        .ok_or_else(|| anyhow!("{editor} probe produced no {marker} payload"))?;
    serde_json::from_str(payload)
        .with_context(|| format!("{editor} probe returned an unsupported payload shape"))
}

fn finish_import(
    mut resolved: ResolvedEditorConfig,
    settings: &Settings,
    destination: &Path,
    write: bool,
    verify: bool,
) -> Result<ImportExecution> {
    let host = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unknown".to_string());
    let editor_rows = std::mem::take(&mut resolved.verification);
    let snapshot = ImportSnapshot::from_resolved(resolved, settings, host)?;
    let verification = verify_snapshot(&snapshot, settings, &editor_rows, verify);
    let toml = snapshot.to_toml()?;
    let previous = fs::read_to_string(destination).unwrap_or_default();
    let diff = TextDiff::from_lines(&previous, &toml)
        .unified_diff()
        .header(
            &format!("a/{}", destination.display()),
            &format!("b/{}", destination.display()),
        )
        .to_string();
    if write {
        snapshot.write(destination)?;
    }
    Ok(ImportExecution {
        snapshot,
        toml,
        diff,
        verification,
    })
}
fn verify_snapshot(
    snapshot: &ImportSnapshot,
    approved: &Settings,
    editor_rows: &std::collections::BTreeMap<String, String>,
    verify: bool,
) -> Vec<VerificationResult> {
    if !verify {
        return Vec::new();
    }
    let mut settings = snapshot.settings.clone();
    settings.files.roots = approved.files.roots.clone();
    let index = match WorkspaceIndex::load(settings.clone()) {
        Ok(index) => index,
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
    let today = Local::now().date_naive();
    let start = verification_start(&settings, today);
    let span = match settings.agenda.span.as_str() {
        "day" => AgendaSpan::Day,
        "month" => AgendaSpan::Month,
        _ => AgendaSpan::Week,
    };
    let (from, to) = span.bounds(start, &settings.agenda.week_start);
    let mut results = Vec::new();
    let native_default = engine
        .agenda(&AgendaQuery {
            from,
            to,
            today,
            match_expression: None,
            log_mode: false,
        })
        .map(|entries| normalize_csv(&agenda_csv(&entries)));
    if snapshot.metadata.editor == "emacs" {
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

    for (key, view) in &snapshot.settings.views {
        if snapshot.metadata.editor != "emacs" {
            results.push(VerificationResult {
                status: ImportStatus::Difference,
                key: key.clone(),
                description: view.desc.clone(),
                detail: "view imported; nvim-orgmode has no stable row-export API".to_string(),
            });
            continue;
        }
        let native = engine.view(view, start, today, false).map(|sections| {
            let rows: String = sections
                .iter()
                .map(|section| agenda_csv(&section.entries) + &items_csv(&section.items))
                .collect();
            normalize_csv(&rows)
        });
        results.push(compare_rows(key, &view.desc, editor_rows.get(key), native));
    }
    for group in &snapshot.groups {
        results.push(VerificationResult {
            status: if group.enabled {
                ImportStatus::Difference
            } else {
                ImportStatus::Unsupported
            },
            key: format!("group:{}", group.key),
            description: group.description.clone(),
            detail: if group.enabled {
                "declarative group imported; grouping does not change underlying agenda rows"
                    .to_string()
            } else {
                group
                    .diagnostics
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "unsupported group".to_string())
            },
        });
    }
    results
}

fn compare_rows(
    key: &str,
    description: &str,
    editor: Option<&String>,
    native: std::result::Result<
        std::result::Result<Vec<Vec<String>>, csv::Error>,
        org_core::AgdError,
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

fn verification_start(settings: &Settings, today: NaiveDate) -> NaiveDate {
    let Some(value) = settings.agenda.start_day.as_deref() else {
        return today;
    };
    if let Some(days) = value
        .strip_prefix('-')
        .and_then(|value| value.strip_suffix('d'))
        .and_then(|value| value.parse::<i64>().ok())
    {
        today - Duration::days(days)
    } else {
        parse_date_input(value, today)
            .ok()
            .and_then(|timestamp| timestamp.date.parse().ok())
            .unwrap_or(today)
    }
}

fn probe_file(name: &str, contents: &str) -> Result<tempfile::NamedTempFile> {
    let suffix = Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("probe");
    let mut probe = tempfile::Builder::new()
        .prefix("agd-import-")
        .suffix(&format!(".{suffix}"))
        .tempfile()
        .context("cannot create editor probe file")?;
    probe
        .write_all(contents.as_bytes())
        .context("cannot write editor probe file")?;
    probe.flush().context("cannot flush editor probe file")?;
    Ok(probe)
}
