use std::path::Path;

use agd_core::{
    AgendaEngine, AgendaEntry, AgendaQuery, AgendaSpan, OccurrenceType, STATUS_SCHEMA, Settings,
    VIEWS_SCHEMA, ViewSection, WorkspaceIndex, WriteEngine, agenda_csv, agenda_row, agenda_text,
    agenda_title, item_row, items_csv, keyword_colors, load_settings, overdue_entries,
    parse_agenda_day,
};
use anyhow::Result;

use super::write_stdout;
use chrono::{Local, NaiveDate};
use clap::Args;

#[derive(Args, Debug)]
pub struct AgendaCommand {
    /// Days to show: day, week, month or a number of days; defaults to
    /// agenda_span from config.toml
    #[arg(long)]
    span: Option<String>,
    /// First day: a date (2026-10-13) or Org date-prompt syntax (fri, +2d,
    /// -1w, oct 30)
    #[arg(long, allow_hyphen_values = true)]
    from: Option<String>,
    /// Filter the dated agenda with an Emacs tags/todo match, e.g.
    /// 'work-someday/NEXT|TODO'. For a tags search like C-c a m, run a
    /// `tags` or `tags-todo` view
    #[arg(long = "match")]
    match_expression: Option<String>,
    /// Run a saved view by its dispatcher key, as org-agenda does: a [[view]]
    /// key, or Org's built-in n, which is always available. Without a key,
    /// the configured start_view
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "",
        value_name = "KEY",
        conflicts_with_all = ["span", "match_expression", "brief", "profile"]
    )]
    view: Option<String>,
    /// Use the files and match of a configured [[agent_profile]]
    #[arg(long)]
    profile: Option<String>,
    /// Also list CLOSED entries, like org-agenda log mode (l)
    #[arg(long)]
    log: bool,
    /// One `date title` line per entry, for hooks
    #[arg(long)]
    brief: bool,
    /// One line of counts for a tmux or herdr status bar; with --json,
    /// agd.status/v1: the counts and the overdue rows in Today's order
    #[arg(
        long,
        conflicts_with_all = [
            "csv", "brief", "view", "span", "from", "match_expression", "log", "profile",
        ]
    )]
    status: bool,
    /// The saved views, start_view first; with --json, also what a client
    /// draws them with: theme, nerd_font, notation and each TODO keyword's
    /// color
    #[arg(
        long,
        conflicts_with_all = [
            "csv", "brief", "view", "status", "span", "from", "match_expression", "log", "profile",
        ]
    )]
    views: bool,
    /// agd.agenda-entry/v1 objects
    #[arg(long, conflicts_with_all = ["csv", "brief"])]
    json: bool,
    /// Emacs org-batch-agenda-csv rows: the same eleven columns and values
    #[arg(long, conflicts_with = "brief")]
    csv: bool,
}

impl AgendaCommand {
    /// True when the output, and so any error, is JSON.
    pub fn json(&self) -> bool {
        self.json
    }

    pub fn execute(self, config: Option<String>, root: Option<String>) -> Result<()> {
        let mut settings = load_settings(
            config.as_deref().map(Path::new),
            root.as_deref().map(Path::new),
        )?
        .settings;
        if self.views {
            return Ok(write_stdout(&views_output(&settings, self.json)?)?);
        }
        let match_expression = match self.profile.as_deref() {
            Some(name) => settings.apply_agent_profile(name, self.match_expression)?,
            None => self.match_expression,
        };
        let index = load_index(settings.clone())?;
        let today = Local::now().date_naive();
        if self.status {
            return Ok(write_stdout(&status_output(
                &index, &settings, today, self.json,
            )?)?);
        }
        let start = match self.from {
            Some(value) => parse_agenda_day(&value, today)?,
            None => settings.agenda_start(today)?,
        };
        let color = !self.json && !self.csv && super::color_stdout();
        if let Some(key) = self.view.as_deref() {
            let view = if key.is_empty() {
                settings.find_start_view()?
            } else {
                settings.find_view(key)?
            };
            let sections = AgendaEngine::new(&index).view(view, start, today, self.log)?;
            let mut output = view_output(&sections, self.json, self.csv, today, &index)?;
            if color {
                let rows: Vec<RowFacts> = sections.iter().flat_map(section_rows).collect();
                output = colored(&output, &rows, today);
            }
            return Ok(write_stdout(&output)?);
        }
        let span = match self.span.as_deref() {
            Some(span) => span.parse::<AgendaSpan>()?,
            None => settings.agenda_span,
        };
        let (from, to) = span.bounds(start, settings.week_start);
        let entries = AgendaEngine::new(&index).agenda(&AgendaQuery {
            from,
            to,
            today,
            match_expression,
            log_mode: self.log,
        })?;
        let output = if self.json {
            format!("{}\n", serde_json::to_string_pretty(&entries)?)
        } else if self.csv {
            agenda_csv(&entries, today, &index)
        } else if self.brief {
            entries
                .iter()
                .map(|entry| format!("{} {}\n", entry.occurrence.date, agenda_title(&entry.item)))
                .collect()
        } else if color {
            let rows: Vec<RowFacts> = entries.iter().map(entry_row).collect();
            colored(&agenda_text(&entries, from, to, None), &rows, today)
        } else {
            agenda_text(&entries, from, to, None)
        };
        Ok(write_stdout(&output)?)
    }
}

/// What colours one printed row: its exact text, whether it is done, and
/// the leader Org printed for it (`Sched. 2x:`, `In   3 d.:`).
struct RowFacts {
    text: String,
    done: bool,
    late: bool,
    due: bool,
    leader: String,
}

fn entry_row(entry: &AgendaEntry) -> RowFacts {
    RowFacts {
        text: agenda_row(entry),
        done: entry.item.done,
        late: entry.occurrence.late,
        due: matches!(
            entry.occurrence.kind,
            OccurrenceType::Deadline | OccurrenceType::UpcomingDeadline
        ),
        leader: entry.occurrence.extra.clone(),
    }
}

fn item_facts(item: &agd_core::Item) -> RowFacts {
    RowFacts {
        text: item_row(item),
        done: item.done,
        late: false,
        due: false,
        leader: String::new(),
    }
}

/// Every row a view section prints, its groups' too.
fn section_rows(section: &ViewSection) -> Vec<RowFacts> {
    let mut rows: Vec<RowFacts> = section.entries.iter().map(entry_row).collect();
    rows.extend(section.items.iter().map(item_facts));
    for group in &section.groups {
        rows.extend(group.entries.iter().map(entry_row));
        rows.extend(group.items.iter().map(item_facts));
    }
    rows
}

const BOLD: &str = "\x1b[1m";
const FAINT: &str = "\x1b[2m";
const UNDERLINE: &str = "\x1b[4m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const RESET: &str = "\x1b[0m";

/// The org-agenda text with 16-colour codes for a terminal, its words
/// unchanged: day headers bold and today's underlined, categories and tags
/// faint, lateness red, deadlines yellow, done rows faint throughout.
/// Only for a terminal; piped output stays Org's own bytes.
fn colored(text: &str, rows: &[RowFacts], today: NaiveDate) -> String {
    let today_line = agenda_text(&[], today, today, Some(""))
        .lines()
        .nth(1)
        .unwrap_or_default()
        .to_string();
    let mut output = String::with_capacity(text.len() * 2);
    for line in text.split_inclusive('\n') {
        let (body, newline) = match line.strip_suffix('\n') {
            Some(body) => (body, "\n"),
            None => (line, ""),
        };
        let painted = match rows.iter().find(|row| row.text == body) {
            Some(row) => paint_row(body, row),
            None if body == today_line => format!("{BOLD}{UNDERLINE}{body}{RESET}"),
            None if is_day_line(body) => format!("{BOLD}{body}{RESET}"),
            None => body.to_string(),
        };
        output.push_str(&painted);
        output.push_str(newline);
    }
    output
}

/// `Friday     16 October 2026`: a weekday first, at the line's start.
fn is_day_line(line: &str) -> bool {
    const DAYS: [&str; 7] = [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ];
    DAYS.iter().any(|day| {
        line.strip_prefix(day)
            .is_some_and(|rest| rest.starts_with(' '))
    })
}

/// One row: `  category:` faint, its leader red when late and yellow when
/// due, its trailing `:tags:` faint; a done row is faint as a whole.
fn paint_row(line: &str, row: &RowFacts) -> String {
    if row.done {
        return format!("{FAINT}{line}{RESET}");
    }
    let category_end = line.find(':').map_or(0, |colon| colon + 1).min(line.len());
    let (indent, rest) = line.split_at(line.len() - line.trim_start().len());
    let category = &rest[..category_end.saturating_sub(indent.len())];
    let mut after = rest[category.len()..].to_string();
    let leader = row.leader.trim();
    if (row.late || row.due)
        && !leader.is_empty()
        && let Some(at) = after.find(leader)
    {
        let color = if row.late { RED } else { YELLOW };
        after = format!(
            "{}{color}{leader}{RESET}{}",
            &after[..at],
            &after[at + leader.len()..]
        );
    }
    let tags_at = after
        .rfind(' ')
        .filter(|at| {
            let tags = &after[at + 1..];
            tags.len() > 1 && tags.starts_with(':') && tags.ends_with(':')
        })
        .map(|at| at + 1);
    if let Some(at) = tags_at {
        after = format!("{}{FAINT}{}{RESET}", &after[..at], &after[at..]);
    }
    format!("{indent}{FAINT}{category}{RESET}{after}")
}

/// A view's output: its sections as JSON, their CSV rows, or the text.
fn view_output(
    sections: &[ViewSection],
    json: bool,
    csv: bool,
    today: NaiveDate,
    index: &WorkspaceIndex,
) -> Result<String> {
    if json {
        return Ok(format!("{}\n", serde_json::to_string_pretty(sections)?));
    }
    if !csv {
        return Ok(render_view(sections));
    }
    let mut rows = String::new();
    for section in sections {
        rows.push_str(&agenda_csv(&section.entries, today, index));
        rows.push_str(&items_csv(&section.items, &section.kind, index));
        for group in &section.groups {
            rows.push_str(&agenda_csv(&group.entries, today, index));
            rows.push_str(&items_csv(&group.items, &section.kind, index));
        }
    }
    Ok(rows)
}

/// Blocks separated by a rule of `=`, as in an Emacs block agenda. An agenda
/// block's header replaces its `Week-agenda (W42):` line, as
/// org-agenda-overriding-header does.
fn render_view(sections: &[ViewSection]) -> String {
    let mut blocks = Vec::new();
    for section in sections {
        let mut block = String::new();
        if !section.groups.is_empty() {
            // org-super-agenda: the block's groups replace its days or list,
            // each under its own indented header.
            if !section.header.is_empty() {
                block.push_str(&format!("{}\n", section.header));
            }
            for group in &section.groups {
                block.push_str(&format!(" {}\n", group.name));
                for entry in &group.entries {
                    block.push_str(&agenda_row(entry));
                    block.push('\n');
                }
                for item in &group.items {
                    block.push_str(&item_row(item));
                    block.push('\n');
                }
            }
        } else if let (Some(from), Some(to)) = (section.from, section.to) {
            let header = (!section.header.is_empty()).then_some(section.header.as_str());
            block.push_str(&agenda_text(&section.entries, from, to, header));
        } else {
            block.push_str(&format!("{}\n", section.header));
            for item in &section.items {
                block.push_str(&item_row(item));
                block.push('\n');
            }
        }
        blocks.push(block);
    }
    blocks.join(&format!("{}\n", "=".repeat(78)))
}

/// The saved views in config order, then the built-in ones, with
/// start_view moved first. The JSON carries the TUI's display settings and
/// palettes, so a client such as the Claude Code pane draws them the same
/// way.
fn views_output(settings: &Settings, json: bool) -> Result<String> {
    let mut views = settings.all_views();
    views.sort_by_key(|view| view.key != settings.start_view);
    if json {
        let rows: Vec<_> = views
            .iter()
            .map(|view| {
                serde_json::json!({
                    "key": view.key,
                    "desc": view.desc,
                    "start": view.key == settings.start_view,
                    "builtin": settings.is_builtin_view(&view.key),
                })
            })
            .collect();
        let index = load_index(settings.clone())?;
        let payload = serde_json::json!({
            "schema": VIEWS_SCHEMA,
            "views": rows,
            "theme": settings.theme,
            "nerd_font": settings.nerd_font,
            "notation": settings.notation,
            "keyword_colors": keyword_colors(&index),
            "palettes": crate::tui::palettes(),
        });
        return Ok(format!("{}\n", serde_json::to_string_pretty(&payload)?));
    }
    Ok(views
        .iter()
        .map(|view| {
            let start = if view.key == settings.start_view {
                "  (start)"
            } else {
                ""
            };
            let builtin = if settings.is_builtin_view(&view.key) {
                "  (built-in)"
            } else {
                ""
            };
            format!("{:<4}{}{start}{builtin}\n", view.key, view.desc)
        })
        .collect())
}

/// The status band, `1 overdue · 3 today · 9 inbox · 2 unreviewed`, as the
/// TUI's header and pane count it (AgendaEngine::summary, words::summary).
/// With --json, agd.status/v1: the same counts and the overdue rows in the
/// order Today and Triage list them (overdue_entries), for a client such as
/// the Claude Code pane.
fn status_output(
    index: &WorkspaceIndex,
    settings: &Settings,
    today: NaiveDate,
    json: bool,
) -> Result<String> {
    let journal = WriteEngine::with_default_journal(settings.clone())?.journal()?;
    let engine = AgendaEngine::new(index);
    let summary = engine.summary(today, &journal)?;
    if !json {
        return Ok(format!("{}\n", crate::words::summary(&summary)));
    }
    let entries = engine.agenda(&AgendaQuery {
        from: today,
        to: today,
        today,
        match_expression: None,
        log_mode: false,
    })?;
    let payload = serde_json::json!({
        "schema": STATUS_SCHEMA,
        "overdue": summary.overdue,
        "today": summary.today,
        "inbox": summary.inbox,
        "unreviewed": summary.unreviewed,
        "overdue_entries": overdue_entries(&entries),
    });
    Ok(format!("{}\n", serde_json::to_string_pretty(&payload)?))
}

// Reads keep working when one agenda file cannot be indexed; say which file
// was left out on stderr so stdout stays machine-readable.
pub fn load_index(settings: Settings) -> Result<WorkspaceIndex> {
    let index = WorkspaceIndex::load(settings)?;
    for (_, reason) in index.skipped_files() {
        eprintln!("warning: skipped {reason}");
    }
    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The text with every escape sequence taken out.
    fn plain(text: &str) -> String {
        let mut output = String::new();
        let mut rest = text;
        while let Some(start) = rest.find('\x1b') {
            output.push_str(&rest[..start]);
            let end = rest[start..]
                .find('m')
                .map_or(rest.len(), |m| start + m + 1);
            rest = &rest[end..];
        }
        output.push_str(rest);
        output
    }

    #[test]
    fn a_terminal_gets_colours_without_a_word_changed() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("work.org"),
            "#+TODO: TODO | DONE\n\
* TODO Late one :ops:\nSCHEDULED: <2026-10-14 Wed>\n\
* TODO Soon one\nDEADLINE: <2026-10-19 Mon>\n\
* DONE Finished\nSCHEDULED: <2026-10-16 Fri>\n",
        )
        .unwrap();
        let settings = Settings {
            roots: vec![directory.path().to_string_lossy().into_owned()],
            agenda_files: vec!["*.org".to_string()],
            ..Settings::default()
        };
        let index = WorkspaceIndex::load(settings).unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 10, 16).unwrap();
        let entries = AgendaEngine::new(&index)
            .agenda(&AgendaQuery {
                from: today,
                to: today,
                today,
                match_expression: None,
                log_mode: false,
            })
            .unwrap();
        let text = agenda_text(&entries, today, today, None);
        let rows: Vec<RowFacts> = entries.iter().map(entry_row).collect();
        let painted = colored(&text, &rows, today);
        assert_eq!(plain(&painted), text, "{painted}");
        assert!(
            painted.contains("\x1b[1m\x1b[4mFriday     16 October 2026"),
            "{painted}"
        );
        assert!(painted.contains("\x1b[31mSched. 2x:\x1b[0m"), "{painted}");
        assert!(painted.contains("\x1b[33mIn   3 d.:\x1b[0m"), "{painted}");
        assert!(painted.contains("\x1b[2m:ops:\x1b[0m"), "{painted}");
        assert!(painted.contains("\x1b[2mwork:\x1b[0m"), "{painted}");
        let done = painted
            .lines()
            .find(|line| line.contains("Finished"))
            .unwrap();
        assert!(done.starts_with("\x1b[2m"), "{done}");
    }
}
