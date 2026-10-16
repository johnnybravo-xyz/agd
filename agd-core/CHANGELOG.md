# Changelog

All notable changes to the `agd-core` library are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/) and the
project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `checklist` reads an entry's checklist, `Item::checklist` carries its
  `Progress`, and `EditIntent::checklist` checks boxes off as
  `org-toggle-checkbox` does, cookies included
  (`JournalOperation::Checklist`).
- `AgendaEngine::summary` and `overdue_entries` give the counts and
  overdue rows every surface shows; `AgendaEngine::inbox` lists the
  entries capture templates filed.
- View blocks take org-super-agenda `groups` and property matches.
- `WriteEngine::edit_batch` writes several edits as one journal
  transaction; `journal`, `journal_conflicts` and `resolve_conflicts` read
  and settle the journal.
- `JournalRecord::operation` names what a write did, and agent actors
  carry the MCP client's name.
- `agenda_text` and `agenda_csv` print what `org-batch-agenda` and
  `org-batch-agenda-csv` print.
- `parse_date_input` reads Org's date prompt, and `capture_prompts`
  describes a template's prompts.

### Changed

- The library is `agd_core`; items are exported at the crate root and the
  modules are private.
- `ErrorCode` lists every agd.error/v1 code under one naming rule, and
  `AgdError::new` takes an `ErrorCode`. Schema names are constants, such as
  `ITEM_SCHEMA` and `STATUS_SCHEMA`.
- `Settings` follows `config.toml`: `todo_keywords` as `#+TODO:` lines,
  `AgendaSpan`, `WeekStart` and `LogMode`, plus `notation`, `start_view`,
  `agent_inbox`, `todo_keyword_colors`, `nerd_font` and
  `someday_clears_dates`; `reschedule_nudge` defaults to 9, and
  `tag_alist` and `persistent_tags` are gone. `load_settings` returns
  `Settings`.

### Fixed

- Habit repeaters on completion, property edits that keep untouched lines,
  and capture placement as org-capture places entries.

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
