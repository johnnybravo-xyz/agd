//! Parsed org files: [`OrgDocument`] (exported as
//! [`IndexedDocument`](crate::IndexedDocument)) with its headlines, planning,
//! properties, timestamps and directives, and [`WorkspaceIndex`], every agenda
//! file under the roots with a stable ref for each heading.
//!
//! Parsing never changes bytes: a document keeps its source and byte spans
//! into it, and a file the Org parser would not reproduce exactly is refused
//! (`parse_failed`). A file's own `#+TODO`, `#+SEQ_TODO`, `#+PRIORITIES`,
//! `#+STARTUP` and `#+FILETAGS` win over configuration, and an `:ID:` that
//! occurs twice under the roots is refused for writes (`id_conflict`).

mod directive;
mod headline;
mod index;
mod org_document;
mod source;
mod timestamp;

pub use directive::{LogKind, LogSettings, PriorityRange, TodoKeyword};
pub(crate) use directive::{ignored_priorities_line, todo_keywords};
pub use headline::{HeadlineSpan, PropertySpan};
pub(crate) use headline::{is_org_tag, leading_priority_cookie, sha256_hex, split_trailing_tags};
pub use index::WorkspaceIndex;
pub use org_document::OrgDocument;
pub use source::ByteSpan;
pub use timestamp::{TimestampSpan, parse_timestamp};
