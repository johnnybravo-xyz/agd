use std::fmt;

use serde::{Deserialize, Serialize};

pub const ITEM_SCHEMA: &str = "agd.item/v1";
pub const AGENDA_ENTRY_SCHEMA: &str = "agd.agenda-entry/v1";
pub const MUTATION_SCHEMA: &str = "agd.mutation/v1";
pub const ERROR_SCHEMA: &str = "agd.error/v1";
/// One block of `agd agenda --view KEY --json`.
pub const VIEW_SECTION_SCHEMA: &str = "agd.view-section/v1";
/// The payload of `agd agenda --views --json`.
pub const VIEWS_SCHEMA: &str = "agd.views/v1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Item {
    pub schema: String,
    #[serde(rename = "ref")]
    pub item_ref: String,
    pub id: Option<String>,
    pub file: String,
    pub line: usize,
    pub olpath: Vec<String>,
    pub todo: Option<String>,
    pub done: bool,
    pub priority: Option<String>,
    pub title: String,
    pub tags: Vec<String>,
    pub inherited_tags: Vec<String>,
    pub category: String,
    pub planning: Planning,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Planning {
    pub scheduled: Option<PlanningTimestamp>,
    pub deadline: Option<PlanningTimestamp>,
    pub closed: Option<PlanningTimestamp>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PlanningTimestamp {
    pub date: String,
    pub time: Option<String>,
    pub end_time: Option<String>,
    pub repeater: Option<String>,
    pub warning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgendaEntry {
    pub schema: String,
    pub item: Item,
    pub occurrence: Occurrence,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Occurrence {
    #[serde(rename = "type")]
    pub kind: OccurrenceType,
    pub date: String,
    pub time: Option<String>,
    /// The end of a time range such as `10:00-11:30`; null without one.
    #[serde(default)]
    pub end_time: Option<String>,
    pub extra: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OccurrenceType {
    Scheduled,
    PastScheduled,
    Deadline,
    UpcomingDeadline,
    Timestamp,
    Block,
    Closed,
    Todo,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Mutation {
    pub schema: String,
    pub ok: bool,
    pub item: Item,
    pub diff: String,
    pub journal_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ErrorResponse {
    pub schema: String,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<Item>,
}

#[derive(Debug)]
pub struct AgdError {
    response: Box<ErrorResponse>,
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl AgdError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            response: Box::new(ErrorResponse {
                schema: ERROR_SCHEMA.to_string(),
                code: code.into(),
                message: message.into(),
                current: None,
            }),
            source: None,
        }
    }

    pub fn with_current(mut self, current: Item) -> Self {
        self.response.current = Some(current);
        self
    }

    pub fn with_source(
        code: impl Into<String>,
        message: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            response: Box::new(ErrorResponse {
                schema: ERROR_SCHEMA.to_string(),
                code: code.into(),
                message: message.into(),
                current: None,
            }),
            source: Some(Box::new(source)),
        }
    }

    pub fn code(&self) -> &str {
        &self.response.code
    }

    pub fn response(&self) -> &ErrorResponse {
        &self.response
    }

    pub fn into_response(self) -> ErrorResponse {
        *self.response
    }
}

impl fmt::Display for AgdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.response.message)
    }
}

impl std::error::Error for AgdError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn std::error::Error + 'static))
    }
}

impl From<std::io::Error> for AgdError {
    fn from(error: std::io::Error) -> Self {
        Self::with_source("io", error.to_string(), error)
    }
}
