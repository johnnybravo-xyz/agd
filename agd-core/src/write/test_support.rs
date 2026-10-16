//! Helpers shared by the write modules' tests: settings over a temporary root,
//! a write context, and small fixtures.

use std::collections::BTreeMap;
use std::path::Path;

use crate::{PlanningTimestamp, Settings, WorkspaceIndex};

use super::edit::{Change, EditIntent};
use super::engine::WriteContext;

pub(super) use crate::config::test_support::settings;

pub(super) fn context() -> WriteContext {
    WriteContext {
        actor: "test".to_string(),
        project: "fixture".to_string(),
        now: None,
    }
}

/// Every path under `directory`, relative, `/`-separated on every
/// platform, and sorted.
pub(super) fn tree(directory: &Path) -> Vec<String> {
    let mut paths: Vec<String> = ignore::WalkBuilder::new(directory)
        .hidden(false)
        .build()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let relative = entry.path().strip_prefix(directory).ok()?;
            (!relative.as_os_str().is_empty()).then(|| {
                relative
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/")
            })
        })
        .collect();
    paths.sort();
    paths
}

/// The first heading's ref under `settings`.
pub(super) fn first_ref(settings: &Settings) -> String {
    WorkspaceIndex::load(settings.clone())
        .unwrap()
        .items()
        .next()
        .unwrap()
        .item_ref
        .clone()
}

pub(super) fn refs_by_title(settings: &Settings) -> BTreeMap<String, String> {
    WorkspaceIndex::load(settings.clone())
        .unwrap()
        .items()
        .map(|item| (item.title.clone(), item.item_ref.clone()))
        .collect()
}

pub(super) fn state_change(keyword: &str) -> EditIntent {
    EditIntent {
        state: Change::Set(keyword.to_string()),
        ..EditIntent::default()
    }
}

pub(super) fn scheduled(date: &str) -> EditIntent {
    EditIntent {
        scheduled: Change::Set(PlanningTimestamp {
            date: date.to_string(),
            time: None,
            end_time: None,
            repeater: None,
            warning: None,
            raw: None,
        }),
        ..EditIntent::default()
    }
}
