//! Saved views: `AgendaEngine::view` runs a view's blocks in order like a
//! block agenda and groups a block's rows as org-super-agenda does; this
//! module also checks a block's match (`validate_view_block`) and reads Org
//! effort durations (`duration_minutes`).
//!
//! The view settings themselves are config types. A row goes to the first
//! group that takes it, and rows no group takes come last as "Other items".

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

use chrono::{NaiveDate, NaiveDateTime};
use regex::Regex;
use serde::Serialize;

use crate::{
    AgdError, AgendaEntry, ErrorCode, HeadlineSpan, Item, Settings, VIEW_SECTION_SCHEMA, ViewBlock,
    ViewGroup, ViewSettings,
};

use super::color::keyword_color;
use super::engine::{AgendaEngine, AgendaQuery};
use super::matcher::{TagTodoMatcher, matcher_now};

/// Checks a view block's match the way `AgendaEngine::view` will use it.
pub fn validate_view_block(block: &ViewBlock) -> Result<(), AgdError> {
    let Some(expression) = block.match_expression.as_deref() else {
        return Ok(());
    };
    let expression = if block.kind == "todo" {
        format!("/{expression}")
    } else {
        expression.to_string()
    };
    TagTodoMatcher::parse(&expression)?;
    for group in &block.groups {
        if let Some(expression) = &group.match_expression {
            TagTodoMatcher::parse(expression)?;
        }
    }
    Ok(())
}

/// One block of a saved view: agenda rows for an agenda block, otherwise a
/// list of items. `from`/`to` are set for agenda blocks only. A block with
/// groups has its rows in `groups` and none in `entries` or `items`.
#[derive(Clone, Debug, Serialize)]
pub struct ViewSection {
    /// VIEW_SECTION_SCHEMA.
    pub schema: String,
    /// The block's kind: agenda, alltodo, todo, tags or tags-todo.
    pub kind: String,
    pub header: String,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub entries: Vec<AgendaEntry>,
    pub items: Vec<Item>,
    /// Empty unless the block has groups.
    pub groups: Vec<ViewSectionGroup>,
    /// The header's color name (COLOR_NAMES): the block's configured color,
    /// or the one its rows' keywords share (see `keyword_color`).
    pub color: String,
}

/// A grouped block's rows under one group name, in display order.
#[derive(Clone, Debug, Serialize)]
pub struct ViewSectionGroup {
    pub name: String,
    pub entries: Vec<AgendaEntry>,
    pub items: Vec<Item>,
    /// As for a section: the group's configured color, or its rows' shared one.
    pub color: String,
}

impl<'a> AgendaEngine<'a> {
    /// Runs a saved view block by block. `start` is the first agenda day
    /// before span alignment, as for `agd agenda --from`.
    pub fn view(
        &self,
        view: &ViewSettings,
        start: NaiveDate,
        today: NaiveDate,
        log_mode: bool,
    ) -> Result<Vec<ViewSection>, AgdError> {
        let settings = self.index.settings();
        let mut sections = Vec::new();
        for block in &view.blocks {
            let match_expression = block.match_expression.clone();
            let section = match block.kind.as_str() {
                "agenda" => {
                    let span = block.span.unwrap_or(settings.agenda_span);
                    let (from, to) = span.bounds(start, settings.week_start);
                    ViewSection {
                        schema: VIEW_SECTION_SCHEMA.to_string(),
                        kind: block.kind.clone(),
                        header: block.header.clone().unwrap_or_default(),
                        from: Some(from),
                        to: Some(to),
                        entries: self.agenda(&AgendaQuery {
                            from,
                            to,
                            today,
                            match_expression,
                            log_mode,
                        })?,
                        items: Vec::new(),
                        groups: Vec::new(),
                        color: String::new(),
                    }
                }
                kind => {
                    let keywords = match_expression.as_deref().unwrap_or("");
                    let (items, default_header) = match kind {
                        "alltodo" => (
                            self.todo(None, today)?,
                            "Global list of TODO items of type: ALL".to_string(),
                        ),
                        // `(todo "DELEGATED")` lists that keyword even when it is
                        // a done state; only alltodo and tags-todo skip done.
                        "todo" if !keywords.is_empty() => (
                            self.tags(&format!("/{keywords}"), today)?,
                            format!("Global list of TODO items of type: {keywords}"),
                        ),
                        "todo" => (
                            self.todo(None, today)?,
                            "Global list of TODO items of type: ALL".to_string(),
                        ),
                        "tags" => (
                            self.tags(keywords, today)?,
                            format!("Headlines with TAGS match: {keywords}"),
                        ),
                        _ => (
                            self.todo(Some(keywords), today)?,
                            format!("Headlines with TAGS match: {keywords}"),
                        ),
                    };
                    ViewSection {
                        schema: VIEW_SECTION_SCHEMA.to_string(),
                        kind: block.kind.clone(),
                        header: block.header.clone().unwrap_or(default_header),
                        from: None,
                        to: None,
                        entries: Vec::new(),
                        items,
                        groups: Vec::new(),
                        color: String::new(),
                    }
                }
            };
            let mut section = self.group_section(section, &block.groups, today)?;
            section.color = match &block.color {
                Some(color) => color.clone(),
                None => {
                    let grouped = section.groups.iter().flat_map(|group| {
                        group
                            .entries
                            .iter()
                            .map(|entry| &entry.item)
                            .chain(&group.items)
                    });
                    self.shared_color(
                        section
                            .entries
                            .iter()
                            .map(|entry| &entry.item)
                            .chain(&section.items)
                            .chain(grouped),
                    )
                }
            };
            sections.push(section);
        }
        Ok(sections)
    }

    /// The color the items' keywords share, or blue when they differ or
    /// there are none.
    fn shared_color<'b>(&self, items: impl Iterator<Item = &'b Item>) -> String {
        let mut colors = items.map(|item| keyword_color(self.index, item));
        match colors.next() {
            Some(first) if colors.all(|color| color == first) => first,
            _ => "blue".to_string(),
        }
    }

    /// Moves a block's rows into its groups, if it has any.
    fn group_section(
        &self,
        mut section: ViewSection,
        groups: &[ViewGroup],
        today: NaiveDate,
    ) -> Result<ViewSection, AgdError> {
        if groups.is_empty() {
            return Ok(section);
        }
        let headlines: HashMap<(&str, usize), &HeadlineSpan> = self
            .headlines()
            .map(|headline| ((headline.item.file.as_str(), headline.item.line), headline))
            .collect();
        let context = GroupContext {
            headlines: &headlines,
            now: matcher_now(today),
            settings: self.index.settings(),
        };
        let entries = group_rows(
            std::mem::take(&mut section.entries),
            |entry| &entry.item,
            groups,
            &context,
        )?;
        let items = group_rows(
            std::mem::take(&mut section.items),
            |item| item,
            groups,
            &context,
        )?;
        section.groups = entries
            .into_iter()
            .map(|group| ViewSectionGroup {
                color: group.color.unwrap_or_else(|| {
                    self.shared_color(group.rows.iter().map(|entry| &entry.item))
                }),
                name: group.name,
                entries: group.rows,
                items: Vec::new(),
            })
            .chain(items.into_iter().map(|group| {
                ViewSectionGroup {
                    color: group
                        .color
                        .unwrap_or_else(|| self.shared_color(group.rows.iter())),
                    name: group.name,
                    entries: Vec::new(),
                    items: group.rows,
                }
            }))
            .collect();
        Ok(section)
    }
}

struct GroupContext<'a> {
    headlines: &'a HashMap<(&'a str, usize), &'a HeadlineSpan>,
    now: NaiveDateTime,
    settings: &'a Settings,
}

const OTHER_ITEMS: &str = "Other items";

/// org-super-agenda's grouping: each group takes the remaining rows it
/// matches, in list order; groups show by order (equal non-zero orders by
/// name), the rest last as "Other items" (order 99), empty groups not at
/// all. Rows keep their order inside a group.
fn group_rows<T>(
    rows: Vec<T>,
    item_of: impl Fn(&T) -> &Item,
    groups: &[ViewGroup],
    context: &GroupContext<'_>,
) -> Result<Vec<RowGroup<T>>, AgdError> {
    let mut remaining = rows;
    let mut sections: Vec<RowGroup<T>> = Vec::new();
    for group in groups {
        if let Some(by) = &group.by {
            let mut keyed: BTreeMap<String, Vec<T>> = BTreeMap::new();
            let mut rest = Vec::new();
            for row in remaining {
                match group_key(by, item_of(&row)) {
                    Some(key) => keyed.entry(key).or_default().push(row),
                    None => rest.push(row),
                }
            }
            sections.extend(keyed.into_iter().map(|(name, rows)| RowGroup {
                name,
                order: group.order,
                color: group.color.clone(),
                rows,
            }));
            remaining = rest;
            continue;
        }
        let matcher = group
            .match_expression
            .as_deref()
            .map(TagTodoMatcher::parse)
            .transpose()?;
        let effort_bound = |bound: &Option<String>| -> Result<Option<f64>, AgdError> {
            bound
                .as_deref()
                .map(|text| {
                    duration_minutes(text).ok_or_else(|| {
                        AgdError::new(
                            ErrorCode::InvalidView,
                            format!("effort '{text}' is not a duration"),
                        )
                    })
                })
                .transpose()
        };
        let effort_max = effort_bound(&group.effort_max)?;
        let effort_min = effort_bound(&group.effort_min)?;
        let (matching, rest): (Vec<T>, Vec<T>) = remaining.into_iter().partition(|row| {
            let item = item_of(row);
            let Some(headline) = context.headlines.get(&(item.file.as_str(), item.line)) else {
                return matcher.is_none() && effort_max.is_none() && effort_min.is_none();
            };
            let effort = || {
                headline
                    .properties
                    .iter()
                    .find(|property| property.key.eq_ignore_ascii_case("Effort"))
                    .and_then(|property| duration_minutes(&property.value))
            };
            matcher
                .as_ref()
                .is_none_or(|matcher| matcher.matches(headline, context.now, context.settings))
                && effort_max.is_none_or(|max| effort().is_some_and(|effort| effort <= max))
                && effort_min.is_none_or(|min| effort().is_some_and(|effort| effort >= min))
        });
        remaining = rest;
        if !group.discard {
            let name = group
                .name
                .clone()
                .or_else(|| group.match_expression.clone())
                .unwrap_or_else(|| OTHER_ITEMS.to_string());
            sections.push(RowGroup {
                name,
                order: group.order,
                color: group.color.clone(),
                rows: matching,
            });
        }
    }
    sections.insert(
        0,
        RowGroup {
            name: OTHER_ITEMS.to_string(),
            order: 99,
            color: None,
            rows: remaining,
        },
    );
    sections.sort_by(|left, right| {
        left.order.cmp(&right.order).then_with(|| {
            if left.order == 0 {
                Ordering::Equal
            } else {
                left.name.cmp(&right.name)
            }
        })
    });
    sections.retain(|group| !group.rows.is_empty());
    Ok(sections)
}

struct RowGroup<T> {
    name: String,
    order: i64,
    color: Option<String>,
    rows: Vec<T>,
}

static DURATION_NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9]+(?:\.[0-9]*)?$").unwrap());
static DURATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^((?:[0-9]+(?:\.[0-9]*)?[ \t]*(?:min|h|d|w|m|y)[ \t]*)*)(?:([0-9]+):([0-9]{2})(?::([0-9]{2}))?)?$",
    )
    .unwrap()
});
static DURATION_UNIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([0-9]+(?:\.[0-9]*)?)[ \t]*(min|h|d|w|m|y)").unwrap());

/// Minutes in an Org duration, as org-duration-to-minutes reads it: a
/// number of minutes, `H:MM` or `H:MM:SS`, unit sums such as `1h 30min` or
/// `2d` (`m` is a 30-day month, `y` 365.25 days), or units then `H:MM`.
pub(crate) fn duration_minutes(text: &str) -> Option<f64> {
    let text = text.trim();
    if DURATION_NUMBER.is_match(text) {
        return text.parse().ok();
    }
    let captures = DURATION.captures(text)?;
    let units = captures.get(1).map_or("", |units| units.as_str());
    if units.is_empty() && captures.get(2).is_none() {
        return None;
    }
    let mut minutes = 0.0;
    for unit in DURATION_UNIT.captures_iter(units) {
        let count: f64 = unit[1].parse().ok()?;
        minutes += count
            * match &unit[2] {
                "min" => 1.0,
                "h" => 60.0,
                "d" => 1_440.0,
                "w" => 10_080.0,
                "m" => 43_200.0,
                _ => 525_960.0,
            };
    }
    let clock = |index: usize| -> f64 {
        captures
            .get(index)
            .and_then(|part| part.as_str().parse::<f64>().ok())
            .unwrap_or(0.0)
    };
    Some(minutes + clock(2) * 60.0 + clock(3) + clock(4) / 60.0)
}

/// The group header for a `by` group, as org-super-agenda's auto groups
/// name them; None leaves the row for later groups.
fn group_key(by: &str, item: &Item) -> Option<String> {
    match by {
        "category" => Some(format!("Category: {}", item.category)),
        "todo" => item.todo.as_ref().map(|todo| format!("To-do: {todo}")),
        "priority" => item
            .priority
            .as_ref()
            .map(|priority| format!("Priority: {priority}")),
        _ => item
            .olpath
            .len()
            .checked_sub(2)
            .map(|parent| item.olpath[parent].clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AgendaSpan;
    use crate::agenda::test_support::fixture;

    #[test]
    fn views_run_blocks_in_order_like_a_block_agenda() {
        let (_temp, index) = fixture(
            "#+TODO: TODO NEXT | DONE\n* TODO Today :work:\nSCHEDULED: <2026-10-13 Tue>\n* NEXT Later :home:\n* DONE Old :work:\n* Note :work:\n",
        );
        let view = ViewSettings {
            key: "m".to_string(),
            desc: "Mixed".to_string(),
            blocks: vec![
                ViewBlock {
                    kind: "agenda".to_string(),
                    match_expression: None,
                    span: Some(AgendaSpan::Day),
                    header: Some("Today".to_string()),
                    color: None,
                    groups: Vec::new(),
                },
                ViewBlock {
                    kind: "todo".to_string(),
                    match_expression: Some("NEXT|DONE".to_string()),
                    span: None,
                    header: None,
                    color: None,
                    groups: Vec::new(),
                },
                ViewBlock {
                    kind: "tags".to_string(),
                    match_expression: Some("work".to_string()),
                    span: None,
                    header: None,
                    color: None,
                    groups: Vec::new(),
                },
            ],
        };
        let today = NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        let sections = AgendaEngine::new(&index)
            .view(&view, today, today, false)
            .unwrap();
        assert_eq!(sections[0].header, "Today");
        assert_eq!(sections[0].entries.len(), 1);
        let titles = |section: &ViewSection| -> Vec<String> {
            section
                .items
                .iter()
                .map(|item| item.title.clone())
                .collect()
        };
        assert_eq!(
            sections[1].header,
            "Global list of TODO items of type: NEXT|DONE"
        );
        assert_eq!(titles(&sections[1]), ["Later", "Old"]);
        assert_eq!(titles(&sections[2]), ["Today", "Old", "Note"]);
    }

    #[test]
    fn durations_read_as_org_duration_to_minutes_does() {
        // Expected values printed by org-duration-to-minutes in Emacs 30.2.
        for (text, minutes) in [
            ("0:10", 10.0),
            ("1:30", 90.0),
            ("10", 10.0),
            ("90", 90.0),
            ("1h", 60.0),
            ("1h 30min", 90.0),
            ("2d", 2880.0),
            ("1:05:30", 65.5),
            (" 0:10 ", 10.0),
            ("0.5", 0.5),
            ("1.5h", 90.0),
        ] {
            assert_eq!(duration_minutes(text), Some(minutes), "{text}");
        }
        assert_eq!(duration_minutes("abc"), None);
        assert_eq!(duration_minutes(""), None);
    }

    #[test]
    fn effort_groups_compare_durations_inclusively() {
        let (_temp, index) = fixture(
            "* TODO Ten as a clock\n:PROPERTIES:\n:Effort:   0:10\n:END:\n* TODO Fifteen as minutes\n:PROPERTIES:\n:Effort:   15\n:END:\n* TODO Ninety as a clock\n:PROPERTIES:\n:Effort:   1:30\n:END:\n* TODO No estimate\n",
        );
        let view = ViewSettings {
            key: "e".to_string(),
            desc: "Effort".to_string(),
            blocks: vec![ViewBlock {
                kind: "alltodo".to_string(),
                match_expression: None,
                span: None,
                header: None,
                color: None,
                groups: vec![
                    ViewGroup {
                        name: Some("Quick".to_string()),
                        effort_max: Some("0:15".to_string()),
                        ..ViewGroup::default()
                    },
                    ViewGroup {
                        name: Some("Long".to_string()),
                        effort_min: Some("1h".to_string()),
                        ..ViewGroup::default()
                    },
                ],
            }],
        };
        let today = NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        let sections = AgendaEngine::new(&index)
            .view(&view, today, today, false)
            .unwrap();
        let groups: Vec<(String, Vec<String>)> = sections[0]
            .groups
            .iter()
            .map(|group| {
                let titles = group.items.iter().map(|item| item.title.clone()).collect();
                (group.name.clone(), titles)
            })
            .collect();
        let expected = [
            ("Quick", vec!["Ten as a clock", "Fifteen as minutes"]),
            ("Long", vec!["Ninety as a clock"]),
            ("Other items", vec!["No estimate"]),
        ]
        .map(|(name, titles)| {
            (
                name.to_string(),
                titles.into_iter().map(str::to_string).collect::<Vec<_>>(),
            )
        });
        assert_eq!(groups, expected);
    }

    #[test]
    fn view_groups_take_rows_first_match_first_and_show_by_order() {
        let (_temp, index) = fixture(
            "#+TODO: TODO NEXT WAIT HOLD | DONE\n* TODO Alone :work:\n* NEXT Decided :home:\n* HOLD Parked\n* WAIT Waiting\n* Project\n** TODO Child\n** TODO Hot child :urgent:\n",
        );
        let group = |name: &str, expression: &str, order: i64| ViewGroup {
            name: Some(name.to_string()),
            match_expression: Some(expression.to_string()),
            order,
            ..ViewGroup::default()
        };
        let view = ViewSettings {
            key: "g".to_string(),
            desc: "Grouped".to_string(),
            blocks: vec![ViewBlock {
                kind: "alltodo".to_string(),
                match_expression: None,
                span: None,
                header: Some("Backlog".to_string()),
                color: None,
                groups: vec![
                    ViewGroup {
                        match_expression: Some("/HOLD".to_string()),
                        discard: true,
                        ..ViewGroup::default()
                    },
                    // Matched before the others, shown after them.
                    group("Waiting", "/WAIT", 9),
                    group("Next", "/NEXT", -1),
                    group("Urgent", "urgent", 0),
                    group("Nothing", "absent", 0),
                    ViewGroup {
                        by: Some("parent".to_string()),
                        ..ViewGroup::default()
                    },
                ],
            }],
        };
        let today = NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        let sections = AgendaEngine::new(&index)
            .view(&view, today, today, false)
            .unwrap();
        let groups: Vec<(String, Vec<String>)> = sections[0]
            .groups
            .iter()
            .map(|group| {
                let titles = group.items.iter().map(|item| item.title.clone()).collect();
                (group.name.clone(), titles)
            })
            .collect();
        let expected = [
            ("Next", vec!["Decided"]),
            ("Urgent", vec!["Hot child"]),
            ("Project", vec!["Child"]),
            ("Waiting", vec!["Waiting"]),
            ("Other items", vec!["Alone"]),
        ]
        .map(|(name, titles)| {
            (
                name.to_string(),
                titles.into_iter().map(str::to_string).collect::<Vec<_>>(),
            )
        });
        assert_eq!(groups, expected);
        assert!(sections[0].items.is_empty());
    }
}
