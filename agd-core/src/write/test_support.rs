use std::collections::BTreeMap;
use std::path::Path;

use crate::{PlanningTimestamp, Settings, WorkspaceIndex};

use super::edit::{Change, EditIntent};
use super::engine::WriteContext;

pub(super) fn settings(root: &Path) -> Settings {
    Settings {
        roots: vec![root.to_string_lossy().into_owned()],
        agenda_files: vec!["**/*.org".to_string()],
        ..Settings::default()
    }
}

pub(super) fn context() -> WriteContext {
    WriteContext {
        actor: "test".to_string(),
        project: "fixture".to_string(),
        now: None,
    }
}

/// Every path under `directory`, relative and sorted.
pub(super) fn tree(directory: &Path) -> Vec<String> {
    let mut paths: Vec<String> = ignore::WalkBuilder::new(directory)
        .hidden(false)
        .build()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let relative = entry.path().strip_prefix(directory).ok()?;
            (!relative.as_os_str().is_empty()).then(|| relative.display().to_string())
        })
        .collect();
    paths.sort();
    paths
}

pub(super) fn refs_by_title(settings: &Settings) -> BTreeMap<String, String> {
    WorkspaceIndex::load(settings.clone())
        .unwrap()
        .items()
        .map(|item| (item.title.clone(), item.item_ref.clone()))
        .collect()
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
