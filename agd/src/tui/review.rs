use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, ListState, Paragraph, Wrap};

use org_core::{JournalRecord, JournalStatus};

use super::App;
use super::theme::{Theme, centered};

/// What an agent change did, in the fewest words that let you judge it.
enum Change {
    Captured {
        heading: String,
        target: String,
    },
    State {
        title: String,
        from: String,
        to: String,
        added: Vec<String>,
    },
    Dates {
        from: String,
        to: String,
    },
    Refiled {
        title: String,
    },
    Edited {
        lines: Vec<String>,
    },
}

fn classify(record: &JournalRecord) -> Change {
    let removed: Vec<&str> = changed(record, '-');
    let added: Vec<&str> = changed(record, '+');
    let headline = |lines: &[&str]| {
        lines
            .iter()
            .find(|line| line.starts_with('*'))
            .map(|line| line.to_string())
    };
    if record.transaction.is_some() {
        return Change::Refiled {
            title: headline(&added)
                .or_else(|| headline(&removed))
                .unwrap_or_default(),
        };
    }
    match (headline(&removed), headline(&added)) {
        (None, Some(heading)) => {
            let target = record
                .file
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_string();
            Change::Captured { heading, target }
        }
        (Some(old), Some(new)) => {
            let (old_word, new_word) = (second_word(&old), second_word(&new));
            if old_word != new_word {
                Change::State {
                    title: title_after_keyword(&new),
                    from: old_word,
                    to: new_word,
                    added: added
                        .iter()
                        .filter(|line| !line.starts_with('*'))
                        .map(|line| line.to_string())
                        .collect(),
                }
            } else {
                Change::Edited {
                    lines: added.iter().map(|line| line.to_string()).collect(),
                }
            }
        }
        _ => {
            let date = |lines: &[&str]| lines.iter().find_map(|line| compact_date(line));
            match (date(&removed), date(&added)) {
                (from, Some(to)) => Change::Dates {
                    from: from.unwrap_or_else(|| "—".to_string()),
                    to,
                },
                _ => Change::Edited {
                    lines: added.iter().map(|line| line.to_string()).collect(),
                },
            }
        }
    }
}

/// `SCHEDULED: <2026-10-15 Thu 15:00>` reads as `15 Oct 15:00`.
fn compact_date(line: &str) -> Option<String> {
    let inner = line.split_once('<')?.1.split_once('>')?.0;
    let mut parts = inner.split_whitespace();
    let date = parts.next()?.parse::<chrono::NaiveDate>().ok()?;
    let _weekday = parts.next();
    let time = parts.next().filter(|part| part.contains(':'));
    Some(match time {
        Some(time) => format!("{} {time}", date.format("%-d %b")),
        None => date.format("%-d %b").to_string(),
    })
}

fn changed(record: &JournalRecord, sign: char) -> Vec<&str> {
    let header = format!("{sign}{sign}{sign}");
    record
        .diff
        .lines()
        .filter(|line| line.starts_with(sign) && !line.starts_with(&header))
        .map(|line| line[1..].trim())
        .filter(|line| !line.is_empty())
        .collect()
}

fn second_word(headline: &str) -> String {
    headline
        .split_whitespace()
        .nth(1)
        .filter(|word| word.chars().all(|c| c.is_ascii_uppercase()))
        .unwrap_or("")
        .to_string()
}

fn title_after_keyword(headline: &str) -> String {
    let words: Vec<&str> = headline.split_whitespace().collect();
    let skip = if words
        .get(1)
        .is_some_and(|word| word.chars().all(|c| c.is_ascii_uppercase()))
    {
        2
    } else {
        1
    };
    words[skip.min(words.len())..].join(" ")
}

/// Line of the change in its file, for opening the editor there.
pub(super) fn changed_line(record: &JournalRecord) -> usize {
    record
        .diff
        .lines()
        .find_map(|line| {
            let hunk = line.strip_prefix("@@ +")?;
            hunk.split([',', ' ']).next()?.parse().ok()
        })
        .or_else(|| {
            record.diff.lines().find_map(|line| {
                let hunk = line.strip_prefix("@@ -")?;
                let plus = hunk.split_once(" +")?.1;
                plus.split([',', ' ']).next()?.parse().ok()
            })
        })
        .unwrap_or(1)
}

pub(super) fn draw(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = &app.theme;
    let unreviewed = app
        .journals
        .iter()
        .filter(|record| record.status == JournalStatus::Committed)
        .count();
    let mut title = format!("{unreviewed} unreviewed");
    if let Some(agent) = &app.agent_filter {
        title.push_str(&format!(" · {agent}"));
    }
    let block = theme.panel(title);
    if app.journals.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "No agent changes yet. MCP writes appear here with who made them.",
                theme.muted(),
            ))
            .block(block),
            area,
        );
        return;
    }
    let width = block.inner(area).width.saturating_sub(2) as usize;
    let items: Vec<ListItem<'_>> = app
        .journals
        .iter()
        .map(|record| ListItem::new(card(theme, record, width, app)))
        .collect();
    let mut state = ListState::default().with_selected(Some(app.selected));
    frame.render_stateful_widget(
        List::new(items)
            .block(block)
            .highlight_symbol(Span::styled("▌", theme.fg(theme.accent)))
            .highlight_spacing(ratatui::widgets::HighlightSpacing::Always)
            .highlight_style(theme.selected()),
        area,
        &mut state,
    );
    if app.review_diff
        && let Some(record) = app.journals.get(app.selected)
    {
        let popup = centered(
            area,
            area.width.saturating_sub(6),
            area.height.saturating_sub(4),
        );
        frame.render_widget(Clear, popup);
        let lines: Vec<Line<'_>> = record
            .diff
            .lines()
            .map(|line| {
                let style = if line.starts_with('+') {
                    theme.fg(theme.done)
                } else if line.starts_with('-') {
                    theme.fg(theme.slipped)
                } else {
                    theme.muted()
                };
                Line::from(Span::styled(line.to_string(), style))
            })
            .collect();
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(theme.panel("Raw diff · d close")),
            popup,
        );
    }
}

fn card(theme: &Theme, record: &JournalRecord, width: usize, app: &App) -> Vec<Line<'static>> {
    let reviewed = record.status != JournalStatus::Committed;
    let agent = record
        .actor
        .strip_prefix("mcp:")
        .unwrap_or(&record.actor)
        .to_string();
    let agent_color = if reviewed {
        theme.muted
    } else {
        theme.agent(&agent)
    };
    let time = record.timestamp.get(11..16).unwrap_or_default().to_string();
    let mut project = shorten_project(&record.project);
    if project.chars().count() > 22 {
        project = format!(
            "…{}",
            project
                .chars()
                .rev()
                .take(21)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<String>()
        );
    }
    let change = classify(record);
    let kind = match &change {
        Change::Captured { .. } => ("captured", theme.done),
        Change::State { .. } => ("state", theme.due),
        Change::Dates { .. } => ("date", theme.accent),
        Change::Refiled { .. } => ("refiled", theme.accent),
        Change::Edited { .. } => ("edited", theme.muted),
    };
    let pill = if reviewed {
        theme.pill("reviewed", theme.muted)
    } else {
        theme.pill(kind.0, kind.1)
    };
    let mut head = vec![
        Span::styled("● ", theme.fg(agent_color)),
        Span::styled(format!("{agent:<14}"), theme.strong(agent_color)),
        Span::styled(format!("{project:<23} "), theme.muted()),
        Span::styled(time, theme.muted()),
    ];
    let used: usize = head.iter().map(Span::width).sum();
    head.push(Span::raw(
        " ".repeat(width.saturating_sub(used + pill.width())),
    ));
    head.push(pill);
    let body_style = |style: Style| if reviewed { theme.muted() } else { style };
    let location = format!(
        "{:<14}",
        format!(
            "{}:{}",
            record.file.rsplit('/').next().unwrap_or_default(),
            changed_line(record)
        )
    );
    let mut lines = vec![Line::from(head)];
    match change {
        Change::Captured { heading, target } => {
            let parent = changed_item(app, record, &heading)
                .map(|item| item.olpath[..item.olpath.len().saturating_sub(1)].join(" › "))
                .unwrap_or_default();
            lines.push(Line::from(vec![
                Span::styled("  + ", body_style(theme.fg(theme.done))),
                Span::styled(heading, body_style(theme.fg(theme.text))),
            ]));
            lines.push(Line::from(Span::styled(
                format!("    → {target} › {parent}"),
                theme.muted(),
            )));
        }
        Change::State {
            title,
            from,
            to,
            added,
        } => {
            lines.push(Line::from(vec![
                Span::styled(format!("  {location}"), theme.muted()),
                Span::styled(format!("{title:<28}"), body_style(theme.fg(theme.text))),
                Span::styled(from, body_style(theme.fg(theme.due))),
                Span::styled(" → ", theme.muted()),
                Span::styled(to, body_style(theme.strong(theme.done))),
            ]));
            for line in added.into_iter().take(1) {
                lines.push(Line::from(vec![
                    Span::styled("  + ", body_style(theme.fg(theme.done))),
                    Span::styled(line, theme.muted()),
                ]));
            }
        }
        Change::Dates { from, to } => {
            let title = changed_item(app, record, "")
                .map(org_core::agenda_title)
                .unwrap_or_default();
            lines.push(Line::from(vec![
                Span::styled(format!("  {location}"), theme.muted()),
                Span::styled(format!("{title:<28}"), body_style(theme.fg(theme.text))),
                Span::styled(from, body_style(theme.fg(theme.muted))),
                Span::styled(" → ", theme.muted()),
                Span::styled(to, body_style(theme.fg(theme.accent))),
            ]));
        }
        Change::Refiled { title } => {
            lines.push(Line::from(vec![
                Span::styled("  ↪ ", theme.muted()),
                Span::styled(title, body_style(theme.fg(theme.text))),
            ]));
        }
        Change::Edited { lines: added } => {
            for line in added.into_iter().take(2) {
                lines.push(Line::from(vec![
                    Span::styled("  + ", body_style(theme.fg(theme.done))),
                    Span::styled(line, theme.muted()),
                ]));
            }
        }
    }
    lines.push(Line::default());
    lines
}

/// The agent's project as `~/src/app`, or its last two path parts.
fn shorten_project(path: &str) -> String {
    if let Ok(home) = std::env::var("HOME")
        && let Some(rest) = path.strip_prefix(&home)
    {
        return format!("~{rest}");
    }
    let parts: Vec<&str> = path.trim_end_matches('/').rsplit('/').take(2).collect();
    format!(
        "…/{}",
        parts.into_iter().rev().collect::<Vec<_>>().join("/")
    )
}

/// The changed heading in today's files: by ref, else by file and title.
fn changed_item<'a>(
    app: &'a App,
    record: &JournalRecord,
    title: &str,
) -> Option<&'a org_core::Item> {
    app.index
        .items()
        .find(|item| record.item_ref.as_deref() == Some(item.item_ref.as_str()))
        .or_else(|| {
            app.index.items().find(|item| {
                item.file == record.file && !title.is_empty() && title.contains(&item.title)
            })
        })
}
