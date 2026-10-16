# Changelog

All notable changes to the `agd-core` library are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/) and the
project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

- The library is `agd_core`, published as the `agd-core` crate from the
  `agd-core/` directory.
- Items are public at the crate root (`agd_core::Settings`,
  `agd_core::WriteEngine`); the modules are private, so each item has one
  path.
- `ErrorCode` lists every agd.error/v1 code, named by one rule
  (`invalid_<thing>`, `unknown_<thing>`, `<thing>_not_found`,
  `<thing>_conflict`, `<thing>_failed`). `AgdError::new` and
  `AgdError::with_source` take an `ErrorCode`, `AgdError::code` returns
  it, `ErrorCode::as_str` and `Display` give its string, and
  `AgdError::with_context` puts context before the message.
- Every schema name is a constant: `ITEM_SCHEMA`, `AGENDA_ENTRY_SCHEMA`,
  `MUTATION_SCHEMA`, `ERROR_SCHEMA`, `VIEW_SECTION_SCHEMA`, `VIEWS_SCHEMA`,
  `CHECK_SCHEMA`, `JOURNAL_SCHEMA`, `UNDO_SCHEMA`, `REFILE_SCHEMA` and
  `DESCRIBE_SCHEMA`.
- `Settings::todo_keywords` holds `#+TODO:` lines (`Vec<String>`),
  `agenda_span` is an `AgendaSpan`, `week_start` a `WeekStart`, and the
  `log_*` settings are `LogMode` (`Off`, `Time`, `Note`). `Settings` has
  `agent_inbox`, `someday_clears_dates`, `todo_keyword_colors`,
  `nerd_font`, `start_view` and `notation`, and no `tag_alist`,
  `persistent_tags`, `agent_capture_to`, `agent_inbox_template` or
  `someday_clears_scheduled`.
- `CaptureTemplate::olpath` names an outline path. A template's `template`
  is empty when `template_file` holds the text, and `load_settings` reads
  that file into `template`.
- `ViewBlock::span` is an `AgendaSpan`, and `ViewBlock` takes `color` and
  `groups` (`ViewGroup`, org-super-agenda groups with `effort_max` and
  `effort_min`). `AgendaEngine::view` returns `ViewSection`s with `schema`,
  `kind`, `color` and `groups` (`ViewSectionGroup`).
- `AgendaSpan` is the one span type: `Days(u32)` holds spans other than a
  day, week or month, `AgendaSpan::days` reads a count as Org does,
  `FromStr` reads command-line and MCP input, serde reads a TOML string or
  integer, and `bounds` takes a `WeekStart`.
- `Settings` looks things up with `view` and `all_views` (your views, then
  the built-in `n` unless one of yours has its key), `is_builtin_view`,
  `find_view`, `find_start_view`, `find_capture_template`,
  `default_capture_template`, `find_agent_profile`, `apply_agent_profile`
  and `agenda_start`; each
  `find_*` returns an `unknown_*` error that lists the configured names.
  `capture_line_template` is the one rule that names a capture line's
  template, and `parse_agenda_day` reads `agenda_start_day` and `--from`.
- `AgendaEngine::todo` and `AgendaEngine::tags` take the agenda day, which
  relative property times count from. `AgendaEngine::inbox` returns the
  open entries where capture templates file them.
- `AgendaEngine::summary` returns an `AgendaSummary` (overdue, today, inbox,
  unreviewed), and `overdue_entries` lists the overdue rows, missed
  deadlines first, each item once. `STATUS_SCHEMA` names `agd.status/v1`.
- `Settings::default()` colors WAIT yellow and CANCELLED dim red in
  `todo_keyword_colors`.
- `agenda_text` prints rows as `org-batch-agenda` does; `agenda_csv` takes
  the entries, the day and the index, and `items_csv` the items, the block
  kind and the index, to print Emacs's eleven CSV columns; `agenda_row` and
  `item_row` give one row's text.
- `keyword_color` names a TODO keyword's color as agd draws it, from
  `Settings::todo_keyword_colors` or its state, `keyword_colors` names every
  keyword the agenda files use, and `COLOR_NAMES` lists the colors.
- `Occurrence` carries `end_time`, `days` and `late`; every time in the
  model reads `HH:MM`, with the file's text in `raw`. `Mutation::repeated`
  (`Repeat`) reports a repeating task marked done that moved to its next
  date.
- `read_date` reads the date at the start of some words, `parse_date_input`
  a whole answer, and `parse_date_change` also `none`.
- `capture_prompts` returns `CapturePrompt`s: each prompt's name, whether it
  takes a date, the planning keyword it fills (`planning`), and
  `answered_by` tells whether a capture line's `due:`/`on:` gives it.
- `WriteEngine::edit_batch` applies `ItemEdit`s as one journal transaction
  (`BatchMutation`), all or nothing. `WriteEngine::undo` takes a journal
  entry or transaction id. `WriteEngine::journal` lists the records newest
  first, and `journal_conflicts` and `resolve_conflicts` find and settle
  interrupted writes.
- `JournalRecord::operation` (`JournalOperation`) says what a write did,
  with `operation_label`, `agent` and `actor_name`; `JournalStatus::Resolved`
  marks a settled conflict. `agent_actor` and `agent_name` turn MCP client
  names into journal actors and back.
- `RootSet::agenda_files` and `RootSet::all_org_files` return `OrgFiles`:
  the files, and the paths left out with their `SkipReason`.
  `WorkspaceIndex::id_in_use` tells whether any heading under the roots
  has an `:ID:`, and `IndexedDocument::priorities` returns the file's
  `PriorityRange`.
- `check_settings` reads files through `WorkspaceIndex`, and its
  `CheckProblem` codes are `ErrorCode` strings; `journal_problems` reports
  interrupted writes.
- `ResolvedEditorConfig::capture` is a list in the editor's order, each
  `ImportedCapture` with its `key`, and `import_root` gives the root an
  import writes for a directory.

## [0.1.0] - 2026-10-14

First public release: the parsing, agenda and write engine behind `agd`.

### Parsing

- `OrgDocument` parses without taking ownership of the file: headline source
  spans, every `#+TODO`/`#+SEQ_TODO` line, `#+STARTUP` logging options,
  planning only on the line after the headline, habit repeaters, ranges,
  Unicode tags and keywords, and non-English weekday names. Timestamps in
  blocks, fixed-width lines and link targets are ignored.
- `WorkspaceIndex` discovers agenda files under the roots, skips Emacs lock
  symlinks and non-UTF-8 files, and reports duplicate `:ID:`s across the
  roots.

### Agenda

- `AgendaEngine` reproduces org-agenda's rows and order for the supported
  subset, checked against a pinned Emacs: scheduled and deadline expansion
  with warnings and repeaters, past-scheduled and overdue reminders,
  headline timestamps, ranges, log mode, and `org-agenda-sorting-strategy`.
- Saved views run org-agenda custom command blocks (agenda, todo, alltodo,
  tags, tags-todo); tags/TODO matches accept Emacs forms and reject property
  and regexp terms with `invalid_match`.

### Writes

- `WriteEngine` changes state, planning, priority, tags, title and properties,
  captures and refiles by byte-span patches: it re-reads under a lock,
  refuses stale or ambiguous refs and unsaved Emacs buffers, journals the
  inverse patch, and replaces the file atomically. Undo relocates its patch
  by context and is a conflict, never an overwrite.
- Writes log the way Emacs does: `!`/`@` keyword markers, CLOSED,
  LAST_REPEAT, reschedule and redeadline notes, LOGBOOK or inline per
  `org-log-into-drawer`, and tags realigned to `org-tags-column`. Repeating
  tasks return to their sequence's first keyword.

### Settings and import

- `Settings` is the flat `config.toml`; `load_settings` reads it from
  `~/.config/agd` (XDG on every Unix) and `render_config` writes it.
- `find_config_sources` finds the Emacs or Neovim config files to read, and
  `apply_import` applies an editor's resolved settings, deriving roots and
  root-relative agenda patterns on a first import.

## [0.0.1] - 2026-10-13

- First release of `agd-core`, forked from
  [org-mcp-server](https://github.com/szaffarano/org-mcp-server).

[Unreleased]: https://github.com/johnnybravo-xyz/agd/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/johnnybravo-xyz/agd/compare/agd-v0.0.1...v0.1.0
[0.0.1]: https://github.com/johnnybravo-xyz/agd/releases/tag/agd-v0.0.1
