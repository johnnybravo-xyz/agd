use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use orgize::ParseConfig;
use regex::Regex;
use sha2::{Digest, Sha256};

use crate::{AgdError, ITEM_SCHEMA, Item, LogMode, Planning, PlanningTimestamp, RootSet, Settings};

static HEADLINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\*+)[ \t]+(.*)$").expect("headline regex"));
static TAGS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|[ \t]+)(:[\w@#%:]+:)[ \t]*$").expect("tags regex"));
static PRIORITY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[#([[:alnum:]])\](?:[ \t]+|$)").expect("priority regex"));
static PROPERTY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^:([[:alnum:]_-]+):(?:[ \t]*(.*))?$").expect("property regex"));
static LINK_TARGET_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[[^\]\n]*\]").expect("link target regex"));
static TIMESTAMP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"[<\[](\d{4}-\d{2}-\d{2})(?:[ \t]+[^\s\d+\->\]]+)?(?:[ \t]+(\d{1,2}:\d{2})(?:-(\d{1,2}:\d{2}))?)?([^>\]]*)[>\]]",
    )
    .expect("timestamp regex")
});

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ByteSpan {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug)]
pub struct SourceLine {
    pub number: usize,
    pub full: ByteSpan,
    pub content: ByteSpan,
}

#[derive(Clone, Debug)]
pub struct PropertySpan {
    pub key: String,
    pub value: String,
    pub line: ByteSpan,
    pub value_span: ByteSpan,
}

#[derive(Clone, Debug)]
pub struct TimestampSpan {
    pub timestamp: PlanningTimestamp,
    pub span: ByteSpan,
    pub active: bool,
}

#[derive(Clone, Debug)]
pub struct HeadlineSpan {
    pub item: Item,
    pub level: usize,
    pub line_span: ByteSpan,
    pub subtree_span: ByteSpan,
    pub section_span: ByteSpan,
    pub planning_span: Option<ByteSpan>,
    pub drawer_span: Option<ByteSpan>,
    pub body_span: ByteSpan,
    pub properties: Vec<PropertySpan>,
    pub timestamps: Vec<TimestampSpan>,
    pub digest: String,
}

#[derive(Clone, Debug)]
pub struct OrgDocument {
    path: PathBuf,
    source: String,
    lines: Vec<SourceLine>,
    headlines: Vec<HeadlineSpan>,
    todo_active: Vec<String>,
    todo_done: Vec<String>,
    todo_keywords: Vec<TodoKeyword>,
    logging: LogSettings,
    file_tags: Vec<String>,
    file_category: String,
}

/// How a state change or planning change is recorded, as in Org's `!` (time)
/// and `@` (note) markers and the org-log-* variables.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogKind {
    Time,
    Note,
}

/// One TODO keyword with the logging markers from `KEYWORD(key!/@)`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TodoKeyword {
    pub name: String,
    pub done: bool,
    /// Index of the sequence it belongs to; repeaters reset to its first keyword.
    pub sequence: usize,
    pub log_enter: Option<LogKind>,
    pub log_leave: Option<LogKind>,
}

/// Effective logging for one file: configuration overridden by `#+STARTUP`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogSettings {
    pub done: Option<LogKind>,
    pub repeat: Option<LogKind>,
    pub reschedule: Option<LogKind>,
    pub redeadline: Option<LogKind>,
    pub into_drawer: bool,
}

#[derive(Clone, Debug)]
pub struct WorkspaceIndex {
    roots: RootSet,
    settings: Settings,
    documents: BTreeMap<PathBuf, OrgDocument>,
    ids: BTreeMap<String, Vec<(PathBuf, usize)>>,
    // `:ID:` values found in org files under the roots that are not agenda
    // files, so an ID shared with an archive or notes file is still ambiguous.
    outside_ids: BTreeMap<String, Vec<PathBuf>>,
    skipped: BTreeMap<PathBuf, String>,
}

/// Reads and parses one agenda file. A non-UTF-8 file is returned as a
/// skip reason instead of an error so that one unreadable file never hides
/// the rest of the agenda.
fn load_document(
    path: &Path,
    settings: &Settings,
) -> Result<Result<OrgDocument, AgdError>, AgdError> {
    let bytes = std::fs::read(path).map_err(|error| {
        AgdError::with_source(
            "io",
            format!("cannot read {}: {error}", path.display()),
            error,
        )
    })?;
    match OrgDocument::parse(path.to_path_buf(), bytes, settings) {
        Ok(document) => Ok(Ok(document)),
        Err(error) if error.code() == "non_utf8" => Ok(Err(error)),
        Err(error) => Err(error),
    }
}

fn scan_property_ids(path: &Path) -> Vec<String> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let key = line.get(..4)?;
            if !key.eq_ignore_ascii_case(":ID:") {
                return None;
            }
            let value = line[4..].trim();
            (!value.is_empty()).then(|| value.to_string())
        })
        .collect()
}

impl OrgDocument {
    pub fn parse(path: PathBuf, bytes: Vec<u8>, settings: &Settings) -> Result<Self, AgdError> {
        let source = String::from_utf8(bytes).map_err(|error| {
            AgdError::with_source(
                "non_utf8",
                format!("org file is not UTF-8: {}", path.display()),
                error,
            )
        })?;
        let lines = source_lines(&source);
        let todo_keywords = todo_keywords(&source, settings);
        let todo_active: Vec<String> = todo_keywords
            .iter()
            .filter(|keyword| !keyword.done)
            .map(|keyword| keyword.name.clone())
            .collect();
        let todo_done: Vec<String> = todo_keywords
            .iter()
            .filter(|keyword| keyword.done)
            .map(|keyword| keyword.name.clone())
            .collect();
        let logging = log_settings(&source, settings);
        let file_tags = directive(&source, "FILETAGS")
            .map(parse_colon_tags)
            .unwrap_or_default();
        let file_category = directive(&source, "CATEGORY")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or("org")
                    .to_string()
            });

        let parse_config = ParseConfig {
            todo_keywords: (todo_active.clone(), todo_done.clone()),
            ..Default::default()
        };
        let parsed = parse_config.parse(&source);
        if parsed.to_org() != source {
            return Err(AgdError::new(
                "parse_drift",
                format!(
                    "orgize changed bytes during a no-op round trip: {}",
                    path.display()
                ),
            ));
        }

        let raw = scan_headlines(
            &path,
            &source,
            &lines,
            &todo_active,
            &todo_done,
            &file_tags,
            &file_category,
            settings,
        )?;
        Ok(Self {
            path,
            source,
            lines,
            headlines: raw,
            todo_active,
            todo_done,
            todo_keywords,
            logging,
            file_tags,
            file_category,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn lines(&self) -> &[SourceLine] {
        &self.lines
    }

    pub fn headlines(&self) -> &[HeadlineSpan] {
        &self.headlines
    }

    pub fn headline_at_line(&self, line: usize) -> Option<&HeadlineSpan> {
        self.headlines
            .iter()
            .find(|headline| headline.item.line == line)
    }

    pub fn todo_active(&self) -> &[String] {
        &self.todo_active
    }

    pub fn todo_done(&self) -> &[String] {
        &self.todo_done
    }

    pub fn todo_keywords(&self) -> &[TodoKeyword] {
        &self.todo_keywords
    }

    pub fn todo_keyword(&self, name: &str) -> Option<&TodoKeyword> {
        self.todo_keywords
            .iter()
            .find(|keyword| keyword.name == name)
    }

    pub fn logging(&self) -> LogSettings {
        self.logging
    }

    pub fn file_tags(&self) -> &[String] {
        &self.file_tags
    }

    pub fn file_category(&self) -> &str {
        &self.file_category
    }
}

impl WorkspaceIndex {
    pub fn load(settings: Settings) -> Result<Self, AgdError> {
        let roots = RootSet::new(&settings.roots, &settings.agenda_files)?;
        let mut documents = BTreeMap::new();
        let mut skipped = BTreeMap::new();
        for path in roots.agenda_files()? {
            match load_document(&path, &settings)? {
                Ok(document) => {
                    documents.insert(path, document);
                }
                Err(reason) => {
                    skipped.insert(path, reason.to_string());
                }
            }
        }
        let mut outside_ids: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
        for path in roots.all_org_files()? {
            if documents.contains_key(&path) || skipped.contains_key(&path) {
                continue;
            }
            for id in scan_property_ids(&path) {
                outside_ids.entry(id).or_default().push(path.clone());
            }
        }
        let mut index = Self {
            roots,
            settings,
            documents,
            ids: BTreeMap::new(),
            outside_ids,
            skipped,
        };
        index.rebuild_ids();
        Ok(index)
    }

    /// Agenda files left out of the index, with the reason (currently only
    /// non-UTF-8 content).
    pub fn skipped_files(&self) -> impl Iterator<Item = (&Path, &str)> {
        self.skipped
            .iter()
            .map(|(path, reason)| (path.as_path(), reason.as_str()))
    }

    pub fn roots(&self) -> &RootSet {
        &self.roots
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn refresh_path(&mut self, path: &Path) -> Result<(), AgdError> {
        if path.exists() {
            if path.extension().is_none_or(|extension| extension != "org") {
                return Ok(());
            }
            let path = self.roots.resolve_read(path)?;
            if !self.documents.contains_key(&path) && !self.skipped.contains_key(&path) {
                for locations in self.outside_ids.values_mut() {
                    locations.retain(|location| location != &path);
                }
                for id in scan_property_ids(&path) {
                    self.outside_ids.entry(id).or_default().push(path.clone());
                }
                self.outside_ids
                    .retain(|_, locations| !locations.is_empty());
                return Ok(());
            }
            match load_document(&path, &self.settings)? {
                Ok(document) => {
                    self.skipped.remove(&path);
                    self.documents.insert(path, document);
                }
                Err(reason) => {
                    self.documents.remove(&path);
                    self.skipped.insert(path, reason.to_string());
                }
            }
        } else {
            self.documents.remove(path);
            self.skipped.remove(path);
        }
        self.rebuild_ids();
        Ok(())
    }

    fn rebuild_ids(&mut self) {
        self.ids.clear();
        for (path, document) in &self.documents {
            for (index, headline) in document.headlines().iter().enumerate() {
                if let Some(id) = &headline.item.id {
                    self.ids
                        .entry(id.clone())
                        .or_default()
                        .push((path.clone(), index));
                }
            }
        }
    }

    pub fn documents(&self) -> impl Iterator<Item = &OrgDocument> {
        self.documents.values()
    }

    pub fn items(&self) -> impl Iterator<Item = &Item> {
        self.documents
            .values()
            .flat_map(|document| document.headlines().iter().map(|headline| &headline.item))
    }

    pub fn duplicate_ids(&self) -> impl Iterator<Item = (&str, &[(PathBuf, usize)])> {
        self.ids
            .iter()
            .filter(|(id, locations)| locations.len() + self.outside_id_count(id) > 1)
            .map(|(id, locations)| (id.as_str(), locations.as_slice()))
    }

    fn outside_id_count(&self, id: &str) -> usize {
        self.outside_ids.get(id).map_or(0, Vec::len)
    }

    fn refuse_duplicate_id(&self, headline: &HeadlineSpan) -> Result<(), AgdError> {
        let Some(id) = &headline.item.id else {
            return Ok(());
        };
        let count = self.ids.get(id).map_or(0, Vec::len) + self.outside_id_count(id);
        if count > 1 {
            return Err(AgdError::new(
                "duplicate_id",
                format!("ID '{id}' occurs {count} times under the configured roots"),
            ));
        }
        Ok(())
    }

    pub fn document(&self, path: &Path) -> Option<&OrgDocument> {
        self.documents.get(path)
    }

    pub fn resolve(&self, item_ref: &str) -> Result<(&OrgDocument, &HeadlineSpan), AgdError> {
        if !item_ref.contains("#sha256:") {
            let matches = self
                .ids
                .get(item_ref)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let outside = self.outside_id_count(item_ref);
            if matches.len() + outside > 1 {
                return Err(AgdError::new(
                    "duplicate_id",
                    format!(
                        "ID '{item_ref}' occurs {} times under the configured roots",
                        matches.len() + outside
                    ),
                ));
            }
            return match matches {
                [] if outside == 1 => Err(AgdError::new(
                    "stale_ref",
                    format!("ID '{item_ref}' is in a file outside the agenda files"),
                )),
                [] => Err(AgdError::new(
                    "stale_ref",
                    format!("no item has ID '{item_ref}'"),
                )),
                [(path, index)] => Ok((
                    self.documents.get(path).expect("indexed document"),
                    &self
                        .documents
                        .get(path)
                        .expect("indexed document")
                        .headlines()[*index],
                )),
                _ => unreachable!("duplicate IDs are rejected above"),
            };
        }

        let (location, digest) = item_ref
            .rsplit_once("#sha256:")
            .ok_or_else(|| AgdError::new("invalid_ref", format!("invalid item ref: {item_ref}")))?;
        if digest.len() != 32 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(AgdError::new(
                "invalid_ref",
                format!("invalid SHA-256 digest in item ref: {item_ref}"),
            ));
        }
        let (file, line) = location.rsplit_once(':').ok_or_else(|| {
            AgdError::new(
                "invalid_ref",
                format!("item ref has no line hint: {item_ref}"),
            )
        })?;
        let line = line.parse::<usize>().map_err(|_| {
            AgdError::new(
                "invalid_ref",
                format!("invalid line hint in item ref: {item_ref}"),
            )
        })?;
        let requested = Path::new(file);
        self.roots.resolve_read(requested)?;
        if let Some(reason) = self.skipped.get(requested) {
            return Err(AgdError::new("non_utf8", reason.clone()));
        }
        let document = self
            .documents
            .get(requested)
            .or_else(|| {
                requested
                    .canonicalize()
                    .ok()
                    .and_then(|canonical| self.documents.get(&canonical))
            })
            .ok_or_else(|| {
                AgdError::new(
                    "stale_ref",
                    format!(
                        "referenced file is not an agenda file: {}",
                        requested.display()
                    ),
                )
            })?;
        if let Some(headline) = document.headline_at_line(line)
            && headline.digest == digest
        {
            self.refuse_duplicate_id(headline)?;
            return Ok((document, headline));
        }
        let matches: Vec<&HeadlineSpan> = document
            .headlines()
            .iter()
            .filter(|headline| headline.digest == digest)
            .collect();
        match matches.as_slice() {
            [headline] => {
                self.refuse_duplicate_id(headline)?;
                Ok((document, headline))
            }
            [] => {
                let mut error = AgdError::new(
                    "stale_ref",
                    format!("headline bytes no longer match ref: {item_ref}"),
                );
                if let Some(current) = document.headline_at_line(line) {
                    error = error.with_current(current.item.clone());
                }
                Err(error)
            }
            _ => Err(AgdError::new(
                "stale_ref",
                format!("headline digest is ambiguous in {}", requested.display()),
            )),
        }
    }
}

#[derive(Clone, Debug)]
struct RawHeadline {
    level: usize,
    line_index: usize,
    line_span: ByteSpan,
    section_end: usize,
    subtree_end: usize,
    keyword: Option<String>,
    done: bool,
    priority: Option<String>,
    title: String,
    tags: Vec<String>,
    planning: Planning,
    planning_span: Option<ByteSpan>,
    drawer_span: Option<ByteSpan>,
    properties: Vec<PropertySpan>,
    timestamps: Vec<TimestampSpan>,
    digest: String,
}

struct ParsedMetadata {
    planning: Planning,
    planning_span: Option<ByteSpan>,
    drawer_span: Option<ByteSpan>,
    properties: Vec<PropertySpan>,
    body_start: usize,
}

#[allow(clippy::too_many_arguments)]
fn scan_headlines(
    path: &Path,
    source: &str,
    lines: &[SourceLine],
    todo_active: &[String],
    todo_done: &[String],
    file_tags: &[String],
    file_category: &str,
    settings: &Settings,
) -> Result<Vec<HeadlineSpan>, AgdError> {
    let mut raw = Vec::new();
    for (line_index, line) in lines.iter().enumerate() {
        let text = &source[line.content.start..line.content.end];
        let Some(captures) = HEADLINE_RE.captures(text) else {
            continue;
        };
        let stars = captures.get(1).expect("stars");
        let content = captures.get(2).expect("headline content");
        let (keyword, done, priority, title, tags) =
            parse_headline_content(content.as_str(), todo_active, todo_done);
        let ParsedMetadata {
            planning,
            planning_span,
            drawer_span,
            properties,
            body_start,
        } = parse_metadata(source, lines, line_index, raw.len())?;
        let digest = headline_digest(text.as_bytes());
        raw.push(RawHeadline {
            level: stars.as_str().len(),
            line_index,
            line_span: line.content,
            section_end: source.len(),
            subtree_end: source.len(),
            keyword,
            done,
            priority,
            title,
            tags,
            planning,
            planning_span,
            drawer_span,
            properties,
            timestamps: Vec::new(),
            digest,
        });
        let current = raw.last_mut().expect("pushed headline");
        current.section_end = body_start;
    }

    for index in 0..raw.len() {
        let next_any = raw
            .get(index + 1)
            .map_or(source.len(), |next| next.line_span.start);
        raw[index].section_end = next_any;
        raw[index].subtree_end = raw[index + 1..]
            .iter()
            .find(|next| next.level <= raw[index].level)
            .map_or(source.len(), |next| next.line_span.start);
        let timestamp_start = raw[index]
            .planning_span
            .map_or(raw[index].line_span.end, |span| span.end);
        // Org lists active timestamps on the headline line itself, e.g.
        // `* Birthday <1993-10-16 Sat +1y>`, but never the planning line.
        let mut timestamps =
            scan_timestamps(source, raw[index].line_span.start, raw[index].line_span.end);
        timestamps.extend(scan_timestamps(source, timestamp_start, next_any));
        raw[index].timestamps = timestamps;
    }

    let excluded: BTreeSet<&str> = settings
        .tag_inheritance_exclusions
        .iter()
        .map(String::as_str)
        .collect();
    let file_path = path.to_string_lossy().into_owned();
    let mut result = Vec::with_capacity(raw.len());
    let mut ancestors: Vec<(usize, String, Vec<String>, String)> = Vec::new();
    for headline in raw {
        while ancestors
            .last()
            .is_some_and(|ancestor| ancestor.0 >= headline.level)
        {
            ancestors.pop();
        }
        let mut inherited = BTreeSet::new();
        for tag in file_tags {
            if !excluded.contains(tag.as_str()) {
                inherited.insert(tag.clone());
            }
        }
        for (_, _, tags, _) in &ancestors {
            for tag in tags {
                if !excluded.contains(tag.as_str()) {
                    inherited.insert(tag.clone());
                }
            }
        }
        for own in &headline.tags {
            inherited.remove(own);
        }
        let own_category = headline
            .properties
            .iter()
            .find(|property| property.key.eq_ignore_ascii_case("CATEGORY"))
            .map(|property| property.value.clone());
        let category = own_category.unwrap_or_else(|| {
            ancestors
                .last()
                .map(|ancestor| ancestor.3.clone())
                .unwrap_or_else(|| file_category.to_string())
        });
        let id = headline
            .properties
            .iter()
            .find(|property| property.key.eq_ignore_ascii_case("ID"))
            .map(|property| property.value.clone());
        let mut olpath: Vec<String> = ancestors
            .iter()
            .map(|ancestor| ancestor.1.clone())
            .collect();
        olpath.push(headline.title.clone());
        let item_ref = id.clone().unwrap_or_else(|| {
            format!(
                "{}:{}#sha256:{}",
                path.display(),
                lines[headline.line_index].number,
                headline.digest
            )
        });
        let item = Item {
            schema: ITEM_SCHEMA.to_string(),
            item_ref,
            id,
            file: file_path.clone(),
            line: lines[headline.line_index].number,
            olpath,
            todo: headline.keyword.clone(),
            done: headline.done,
            priority: headline.priority.clone(),
            title: headline.title.clone(),
            tags: headline.tags.clone(),
            inherited_tags: inherited.into_iter().collect(),
            category: category.clone(),
            planning: headline.planning.clone(),
        };
        let metadata_end = headline
            .drawer_span
            .or(headline.planning_span)
            .map_or(headline.line_span.end, |span| span.end);
        let body_start = line_end_after(source, metadata_end);
        let body_span = ByteSpan {
            start: body_start.min(headline.section_end),
            end: headline.section_end,
        };
        ancestors.push((
            headline.level,
            headline.title.clone(),
            headline.tags.clone(),
            category,
        ));
        result.push(HeadlineSpan {
            item,
            level: headline.level,
            line_span: headline.line_span,
            subtree_span: ByteSpan {
                start: headline.line_span.start,
                end: headline.subtree_end,
            },
            section_span: ByteSpan {
                start: headline.line_span.start,
                end: headline.section_end,
            },
            planning_span: headline.planning_span,
            drawer_span: headline.drawer_span,
            body_span,
            properties: headline.properties,
            timestamps: headline.timestamps,
            digest: headline.digest,
        });
    }
    Ok(result)
}

fn parse_headline_content(
    input: &str,
    todo_active: &[String],
    todo_done: &[String],
) -> (Option<String>, bool, Option<String>, String, Vec<String>) {
    let mut content = input.trim_end();
    let tags = TAGS_RE
        .captures(content)
        .and_then(|captures| captures.get(1))
        .map(|matched| {
            content = content[..matched.start()].trim_end();
            parse_colon_tags(matched.as_str())
        })
        .unwrap_or_default();

    let first_end = content.find(char::is_whitespace).unwrap_or(content.len());
    let first = &content[..first_end];
    let (keyword, done) = if todo_active.iter().any(|word| word == first) {
        content = content[first_end..].trim_start();
        (Some(first.to_string()), false)
    } else if todo_done.iter().any(|word| word == first) {
        content = content[first_end..].trim_start();
        (Some(first.to_string()), true)
    } else {
        (None, false)
    };

    let priority = PRIORITY_RE.captures(content).and_then(|captures| {
        let full = captures.get(0)?;
        let value = captures.get(1)?.as_str().to_string();
        content = content[full.end()..].trim_start();
        Some(value)
    });
    (keyword, done, priority, content.to_string(), tags)
}

fn parse_metadata(
    source: &str,
    lines: &[SourceLine],
    headline_line: usize,
    _headline_count: usize,
) -> Result<ParsedMetadata, AgdError> {
    let mut planning = Planning::default();
    // Org reads planning only from the line right after the headline; a
    // second SCHEDULED/DEADLINE line or one after a blank line is plain text
    // with an ordinary timestamp.
    let mut planning_span: Option<ByteSpan> = None;
    let mut cursor = headline_line + 1;
    if let Some(line) = lines.get(cursor) {
        let trimmed = source[line.content.start..line.content.end].trim_start();
        if is_planning_line(trimmed) {
            planning_span = Some(line.full);
            planning.scheduled = labeled_timestamp(trimmed, "SCHEDULED:");
            planning.deadline = labeled_timestamp(trimmed, "DEADLINE:");
            planning.closed = labeled_timestamp(trimmed, "CLOSED:");
            cursor += 1;
        }
    }

    let mut properties = Vec::new();
    let mut drawer_span = None;
    if let Some(line) = lines.get(cursor) {
        let text = &source[line.content.start..line.content.end];
        let trimmed = text.trim_start();
        if trimmed == ":PROPERTIES:" {
            let start = line.full.start;
            cursor += 1;
            let mut found_end = false;
            while let Some(property_line) = lines.get(cursor) {
                let property_text = &source[property_line.content.start..property_line.content.end];
                let property_trimmed = property_text.trim_start();
                if property_trimmed == ":END:" {
                    drawer_span = Some(ByteSpan {
                        start,
                        end: property_line.full.end,
                    });
                    cursor += 1;
                    found_end = true;
                    break;
                }
                if let Some(captures) = PROPERTY_RE.captures(property_trimmed) {
                    let key = captures.get(1).expect("property key");
                    let value = captures.get(2);
                    properties.push(PropertySpan {
                        key: key.as_str().to_string(),
                        value: value.map_or("", |matched| matched.as_str()).to_string(),
                        line: property_line.full,
                        value_span: value.map_or(
                            ByteSpan {
                                start: property_line.content.end,
                                end: property_line.content.end,
                            },
                            |matched| ByteSpan {
                                start: property_line.content.start + property_text.len()
                                    - property_trimmed.len()
                                    + matched.start(),
                                end: property_line.content.start + property_text.len()
                                    - property_trimmed.len()
                                    + matched.end(),
                            },
                        ),
                    });
                }
                cursor += 1;
            }
            if !found_end {
                return Err(AgdError::new(
                    "parse_error",
                    format!(
                        "unterminated property drawer after line {}",
                        headline_line + 1
                    ),
                ));
            }
        }
    }
    let body_start = lines
        .get(cursor)
        .map_or(source.len(), |line| line.full.start);
    Ok(ParsedMetadata {
        planning,
        planning_span,
        drawer_span,
        properties,
        body_start,
    })
}

fn source_lines(source: &str) -> Vec<SourceLine> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (index, segment) in source.split_inclusive('\n').enumerate() {
        let end = start + segment.len();
        let content_end = if segment.ends_with("\r\n") {
            end - 2
        } else if segment.ends_with('\n') {
            end - 1
        } else {
            end
        };
        lines.push(SourceLine {
            number: index + 1,
            full: ByteSpan { start, end },
            content: ByteSpan {
                start,
                end: content_end,
            },
        });
        start = end;
    }
    if source.is_empty() {
        return lines;
    }
    if start < source.len() {
        lines.push(SourceLine {
            number: lines.len() + 1,
            full: ByteSpan {
                start,
                end: source.len(),
            },
            content: ByteSpan {
                start,
                end: source.len(),
            },
        });
    }
    lines
}

/// TODO keywords from every `#+TODO:`/`#+SEQ_TODO:`/`#+TYP_TODO:` line, or
/// the configured `todo_keywords` lines, which hold the same text. A
/// sequence without `|` makes its last keyword the done state, as in Org.
fn todo_keywords(source: &str, settings: &Settings) -> Vec<TodoKeyword> {
    let in_file: Vec<&str> = directives(source, &["TODO", "SEQ_TODO", "TYP_TODO"]).collect();
    let lines = if in_file.is_empty() {
        settings.todo_keywords.iter().map(String::as_str).collect()
    } else {
        in_file
    };
    let mut keywords = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let sequence: Vec<&str> = line.split_whitespace().collect();
        let separator = sequence.iter().position(|word| *word == "|");
        let last = sequence.len().saturating_sub(1);
        for (position, raw) in sequence.iter().enumerate() {
            if *raw == "|" {
                continue;
            }
            let (name, markers) = raw.split_once('(').map_or((*raw, ""), |(name, rest)| {
                (name, rest.trim_end_matches(')'))
            });
            if name.is_empty() {
                continue;
            }
            let (enter, leave) = markers.split_once('/').unwrap_or((markers, ""));
            keywords.push(TodoKeyword {
                name: name.to_string(),
                done: separator.map_or(position == last, |separator| position > separator),
                sequence: index,
                log_enter: log_marker(enter),
                log_leave: log_marker(leave),
            });
        }
    }
    keywords
}

fn log_marker(marker: &str) -> Option<LogKind> {
    if marker.contains('@') {
        Some(LogKind::Note)
    } else if marker.contains('!') {
        Some(LogKind::Time)
    } else {
        None
    }
}

fn log_value(mode: LogMode) -> Option<LogKind> {
    match mode {
        LogMode::Off => None,
        LogMode::Time => Some(LogKind::Time),
        LogMode::Note => Some(LogKind::Note),
    }
}

/// org-log-* settings for one file, with `#+STARTUP:` options such as
/// `logrepeat`, `lognotereschedule` or `nologdrawer` applied on top.
fn log_settings(source: &str, settings: &Settings) -> LogSettings {
    let agenda = settings;
    let mut logging = LogSettings {
        done: log_value(agenda.log_done),
        repeat: log_value(agenda.log_repeat),
        reschedule: log_value(agenda.log_reschedule),
        redeadline: log_value(agenda.log_redeadline),
        into_drawer: agenda.log_into_drawer,
    };
    for option in directives(source, &["STARTUP"]).flat_map(str::split_whitespace) {
        let option = option.to_ascii_lowercase();
        let (kind, name) = if let Some(name) = option.strip_prefix("nolog") {
            (None, name)
        } else if let Some(name) = option.strip_prefix("lognote") {
            (Some(LogKind::Note), name)
        } else if let Some(name) = option.strip_prefix("log") {
            (Some(LogKind::Time), name)
        } else {
            continue;
        };
        match name {
            "done" => logging.done = kind,
            "repeat" => logging.repeat = kind,
            "reschedule" => logging.reschedule = kind,
            "redeadline" => logging.redeadline = kind,
            "drawer" => logging.into_drawer = kind.is_some(),
            _ => {}
        }
    }
    logging
}

fn directives<'a>(source: &'a str, names: &'a [&str]) -> impl Iterator<Item = &'a str> {
    source.lines().filter_map(move |line| {
        let (key, value) = line.strip_prefix("#+")?.split_once(':')?;
        names
            .iter()
            .any(|name| key.eq_ignore_ascii_case(name))
            .then_some(value.trim())
    })
}

fn directive<'a>(source: &'a str, name: &str) -> Option<&'a str> {
    source.lines().find_map(|line| {
        let (key, value) = line.strip_prefix("#+")?.split_once(':')?;
        key.eq_ignore_ascii_case(name).then_some(value.trim())
    })
}

fn parse_colon_tags(value: &str) -> Vec<String> {
    value
        .trim()
        .trim_matches(':')
        .split(':')
        .filter(|tag| !tag.is_empty())
        .map(str::to_string)
        .collect()
}

// A planning line starts with one of the keywords; `: SCHEDULED: <…>` is
// fixed-width text, not planning.
fn is_planning_line(line: &str) -> bool {
    let line = line.trim_start();
    ["SCHEDULED:", "DEADLINE:", "CLOSED:"]
        .iter()
        .any(|label| line.starts_with(label))
}

fn labeled_timestamp(line: &str, label: &str) -> Option<PlanningTimestamp> {
    let after = line.split_once(label)?.1;
    parse_timestamp(after)
}

pub fn parse_timestamp(input: &str) -> Option<PlanningTimestamp> {
    let captures = TIMESTAMP_RE.captures(input)?;
    let suffix = captures.get(4).map_or("", |matched| matched.as_str());
    let repeater = suffix
        .split_whitespace()
        .find(|token| token.starts_with('+') || token.starts_with(".+"))
        .map(str::to_string);
    let warning = suffix
        .split_whitespace()
        .find(|token| token.starts_with('-'))
        .map(str::to_string);
    Some(PlanningTimestamp {
        date: captures.get(1)?.as_str().to_string(),
        time: captures.get(2).map(|matched| matched.as_str().to_string()),
        end_time: captures.get(3).map(|matched| matched.as_str().to_string()),
        repeater,
        warning,
        raw: captures.get(0).map(|matched| matched.as_str().to_string()),
    })
}

// Org ignores timestamps in src/example/export/comment blocks, in
// fixed-width lines (`: text`) and inside link targets.
fn scan_timestamps(source: &str, start: usize, end: usize) -> Vec<TimestampSpan> {
    if start >= end || end > source.len() {
        return Vec::new();
    }
    let mut found = Vec::new();
    let mut in_verbatim_block = false;
    let mut line_start = start;
    for line in source[start..end].split_inclusive('\n') {
        let offset = line_start;
        line_start += line.len();
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        if in_verbatim_block {
            in_verbatim_block = !lower.starts_with("#+end_");
            continue;
        }
        if [
            "#+begin_src",
            "#+begin_example",
            "#+begin_export",
            "#+begin_comment",
        ]
        .iter()
        .any(|opening| lower.starts_with(opening))
        {
            in_verbatim_block = true;
            continue;
        }
        if trimmed == ":" || trimmed.starts_with(": ") {
            continue;
        }
        let masked = LINK_TARGET_RE.replace_all(line, |captures: &regex::Captures| {
            " ".repeat(captures[0].len())
        });
        for captures in TIMESTAMP_RE.captures_iter(&masked) {
            let Some(matched) = captures.get(0) else {
                continue;
            };
            let Some(timestamp) = parse_timestamp(matched.as_str()) else {
                continue;
            };
            found.push(TimestampSpan {
                timestamp,
                span: ByteSpan {
                    start: offset + matched.start(),
                    end: offset + matched.end(),
                },
                active: matched.as_str().starts_with('<'),
            });
        }
    }
    found
}

fn headline_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn line_end_after(source: &str, offset: usize) -> usize {
    if offset >= source.len() {
        return source.len();
    }
    if source.as_bytes().get(offset) == Some(&b'\n') {
        offset + 1
    } else {
        offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(root: &Path) -> Settings {
        Settings {
            roots: vec![root.to_string_lossy().into_owned()],
            ..Settings::default()
        }
    }

    #[test]
    fn indexes_refs_inheritance_and_in_file_todo_without_drift() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("work.org");
        let source = "#+TODO: OPEN(o) | SHUT(s!)\r\n#+FILETAGS: :work:\r\n* Parent :project:\r\n** OPEN [#A] Task :report:\r\nSCHEDULED: <2026-10-13 Tue +1w> DEADLINE: <2026-10-16 Fri -5d>\r\n:PROPERTIES:\r\n:ID: exact-id\r\n:CATEGORY: reporting\r\n:END:\r\nbody unchanged\r\n";
        std::fs::write(&path, source).unwrap();
        let document = OrgDocument::parse(
            path.clone(),
            std::fs::read(&path).unwrap(),
            &settings(temp.path()),
        )
        .unwrap();
        assert_eq!(document.source(), source);
        let item = &document.headlines()[1].item;
        assert_eq!(item.item_ref, "exact-id");
        assert_eq!(item.todo.as_deref(), Some("OPEN"));
        assert_eq!(item.priority.as_deref(), Some("A"));
        assert_eq!(item.inherited_tags, vec!["project", "work"]);
        assert_eq!(item.category, "reporting");
        assert_eq!(
            item.planning.deadline.as_ref().unwrap().warning.as_deref(),
            Some("-5d")
        );
    }

    #[test]
    fn digest_ref_relocates_uniquely_and_reports_stale_current() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tasks.org");
        std::fs::write(&path, "* TODO One\n* TODO Two\n").unwrap();
        let index = WorkspaceIndex::load(settings(temp.path())).unwrap();
        let item_ref = index.items().nth(1).unwrap().item_ref.clone();
        assert_eq!(index.resolve(&item_ref).unwrap().1.item.title, "Two");

        std::fs::write(&path, "intro\n* TODO One\n* TODO Two\n").unwrap();
        let relocated = WorkspaceIndex::load(settings(temp.path())).unwrap();
        assert_eq!(relocated.resolve(&item_ref).unwrap().1.item.line, 3);

        std::fs::write(&path, "intro\n* TODO One\n* TODO Changed\n").unwrap();
        let stale = WorkspaceIndex::load(settings(temp.path())).unwrap();
        let error = stale.resolve(&item_ref).unwrap_err();
        assert_eq!(error.code(), "stale_ref");
    }

    #[test]
    fn non_utf8_agenda_file_is_skipped_for_reads_and_refused_for_writes() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("good.org"), "* TODO Readable\n").unwrap();
        let bad = temp.path().join("bad.org");
        std::fs::write(&bad, b"* TODO Latin-1 \xe9\n").unwrap();
        let index = WorkspaceIndex::load(settings(temp.path())).unwrap();
        assert_eq!(index.items().count(), 1);
        let skipped: Vec<_> = index.skipped_files().collect();
        assert_eq!(skipped.len(), 1);
        assert!(skipped[0].0.ends_with("bad.org"));
        let canonical = bad.canonicalize().unwrap();
        let bad_ref = format!(
            "{}:1#sha256:00000000000000000000000000000000",
            canonical.display()
        );
        assert_eq!(index.resolve(&bad_ref).unwrap_err().code(), "non_utf8");
    }

    #[test]
    fn ids_in_non_agenda_files_make_targets_ambiguous() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("tasks.org"),
            "* TODO One\n:PROPERTIES:\n:ID: shared\n:END:\n",
        )
        .unwrap();
        std::fs::create_dir(temp.path().join("archive")).unwrap();
        std::fs::write(
            temp.path().join("archive/notes.org"),
            "* Notes\n:PROPERTIES:\n:id:   shared\n:END:\n",
        )
        .unwrap();
        let mut settings = settings(temp.path());
        settings.agenda_files = vec!["tasks.org".to_string()];
        let index = WorkspaceIndex::load(settings).unwrap();
        assert_eq!(index.resolve("shared").unwrap_err().code(), "duplicate_id");
        let item_ref = index.items().next().unwrap().item_ref.clone();
        assert_eq!(index.resolve(&item_ref).unwrap_err().code(), "duplicate_id");
        assert_eq!(index.duplicate_ids().count(), 1);
    }

    #[test]
    fn duplicate_ids_are_not_selectable() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("tasks.org"),
            "* TODO One\n:PROPERTIES:\n:ID: same\n:END:\n* TODO Two\n:PROPERTIES:\n:ID: same\n:END:\n",
        )
        .unwrap();
        let index = WorkspaceIndex::load(settings(temp.path())).unwrap();
        assert_eq!(index.duplicate_ids().count(), 1);
        assert_eq!(index.resolve("same").unwrap_err().code(), "duplicate_id");
    }
}
