# agd-core changelog

This file records consumer-visible changes to the `agd-core` library. It
follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/) and
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-10-16

### Added

- Checklist parsing through `checklist`, checklist progress on
  `Item::checklist`, and `EditIntent::checklist` updates compatible with
  `org-toggle-checkbox`, including cookies and `JournalOperation::Checklist`.
- `AgendaEngine::summary` and `AgendaEngine::overdue_entries` for shared status
  counts and Today ordering, plus `AgendaEngine::inbox` for entries targeted by
  capture templates.
- org-super-agenda-style groups and property matches for saved-view blocks.
- `WriteEngine::edit_batch` for transactional multi-edit writes.
- `journal`, `journal_conflicts`, and `resolve_conflicts` for reading and
  settling interrupted operations.
- `JournalRecord::operation` for mutation classification and named MCP clients
  on agent actors.
- `agenda_text` and `agenda_csv`, matching `org-batch-agenda` and
  `org-batch-agenda-csv` for the supported subset.
- `parse_date_input` for Org-style date prompts and `capture_prompts` for
  describing template prompts.

### Changed

- The Rust library name is now `agd_core`. Public items are re-exported at the
  crate root and internal modules are private.
- `ErrorCode` now contains every `agd.error/v1` code under one naming scheme.
  `AgdError::new` accepts an `ErrorCode`, and schema names are constants such as
  `ITEM_SCHEMA` and `STATUS_SCHEMA`.
- `Settings` now mirrors the flat `config.toml` model:
  - `todo_keywords` uses `#+TODO:` sequence syntax;
  - `AgendaSpan`, `WeekStart`, and `LogMode` represent constrained values;
  - `notation`, `start_view`, `agent_inbox`, `todo_keyword_colors`,
    `nerd_font`, and `someday_clears_dates` were added;
  - `reschedule_nudge` now defaults to nine;
  - `tag_alist` and `persistent_tags` were removed.
- `load_settings` now returns `Settings` directly.

### Fixed

- Habit completion with repeaters.
- Property edits that previously rewrote untouched drawer lines.
- Capture placement and blank lines for supported org-capture targets.
- `config_dir` and `data_dir` now honor `XDG_CONFIG_HOME` and `XDG_DATA_HOME`
  on Windows.

## [0.1.0] - 2026-10-14

The first public library release provided the parsing, agenda, configuration,
and source-preserving write engine used by `agd`.

### Parsing and discovery

- `OrgDocument` retained source spans for headings, TODO keywords, priorities,
  tags, planning, properties, bodies, and subtrees.
- Parsing supported every `#+TODO` and `#+SEQ_TODO` declaration,
  `#+STARTUP` logging options, planning only on the line after a heading,
  habits, repeaters, ranges, Unicode tags and keywords, and localized weekday
  names.
- Timestamps inside blocks, fixed-width lines, and link targets were excluded.
- `WorkspaceIndex` discovered agenda files under configured roots, skipped
  Emacs lock symlinks and invalid UTF-8, and reported duplicate `:ID:` values
  across roots.

### Agenda

- `AgendaEngine` reproduced org-agenda rows and ordering for the fixture-backed
  subset: scheduled and deadline expansion, warnings, repeaters,
  past-scheduled carry-over, overdue reminders, heading timestamps, ranges, log
  mode, and sorting strategies.
- Saved views supported `agenda`, `todo`, `alltodo`, `tags`, and `tags-todo`
  blocks.
- Tags/TODO matches accepted the initial Emacs subset and rejected unsupported
  property and regexp terms with `invalid_match`.

### Writes

- `WriteEngine` changed state, planning, priority, tags, title, and properties;
  captured entries; and refiled subtrees through byte-range patches.
- Writes re-read under lock, rejected stale or ambiguous refs and unsaved Emacs
  buffers, journaled inverse patches, and replaced files atomically.
- Undo relocated patches by context and treated uncertainty as a conflict.
- State changes implemented `!` and `@` markers, `CLOSED`, `LAST_REPEAT`,
  reschedule and redeadline notes, inline or `LOGBOOK` logging, and
  `org-tags-column` alignment. Repeating tasks returned to the first open state
  in their sequence.

### Configuration and import

- `Settings` represented the flat `config.toml` file.
- `load_settings` used `~/.config/agd` with XDG support on Unix, and
  `render_config` generated the file.
- `find_config_sources` discovered Emacs and Neovim configuration files.
- `apply_import` converted resolved editor settings, deriving roots and
  root-relative agenda patterns on first import.

## [0.0.1] - 2026-10-13

- First `agd-core` release, forked from
  [org-mcp-server](https://github.com/szaffarano/org-mcp-server).

[Unreleased]: https://github.com/johnnybravo-xyz/agd/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/johnnybravo-xyz/agd/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/johnnybravo-xyz/agd/compare/agd-v0.0.1...v0.1.0
[0.0.1]: https://github.com/johnnybravo-xyz/agd/releases/tag/agd-v0.0.1
