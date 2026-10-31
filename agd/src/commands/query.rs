use std::path::Path;

use agd_core::{AgendaEngine, Item, WorkspaceIndex, item_row, load_settings};
use anyhow::Result;
use chrono::Local;
use clap::Args;
use serde_json::Value;

use super::{agenda::load_index, write_stdout};

#[derive(Args, Debug)]
pub struct QueryCommand {
    /// org-ql expression, such as '(and (todo) (deadline :to 7))'
    expression: String,
    /// Return at most this many items
    #[arg(long)]
    limit: Option<usize>,
    /// Include each item's full body; requires --json
    #[arg(long, requires = "json")]
    with_body: bool,
    /// Print agd.item/v1 objects
    #[arg(long)]
    pub json: bool,
}

impl QueryCommand {
    pub fn execute(self, config: Option<String>, root: Option<String>) -> Result<()> {
        let settings = load_settings(
            config.as_deref().map(Path::new),
            root.as_deref().map(Path::new),
        )?;
        let index = load_index(settings)?;
        let mut items =
            AgendaEngine::new(&index).query(&self.expression, Local::now().date_naive())?;
        if let Some(limit) = self.limit {
            items.truncate(limit);
        }
        let output = if self.json {
            json_output(&items, &index, self.with_body)?
        } else {
            items
                .iter()
                .map(|item| format!("{}\n", item_row(item)))
                .collect()
        };
        Ok(write_stdout(&output)?)
    }
}

fn json_output(items: &[Item], index: &WorkspaceIndex, with_body: bool) -> Result<String> {
    let mut value = serde_json::to_value(items)?;
    if with_body {
        let Value::Array(rows) = &mut value else {
            unreachable!("a list of items serializes as an array");
        };
        for (row, item) in rows.iter_mut().zip(items) {
            let Value::Object(row) = row else {
                continue;
            };
            let body = index
                .item_body(Path::new(&item.file), item.line)
                .unwrap_or_default();
            row.insert("body".to_string(), Value::String(body.to_string()));
        }
    }
    Ok(format!("{}\n", serde_json::to_string_pretty(&value)?))
}
