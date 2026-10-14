# Changelog

All notable changes to the `agd-core` library are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/) and the
project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- `CapturePrompt::planning` names the planning keyword a date prompt fills,
  and `answered_by` tells whether a capture line's `due:`/`on:` already
  gives it.

### Fixed

- Capture templates support Org's date prompts (`%^{name}t`, `T`, `u`, `U`
  and a bare `%^t`): the answer takes the same input as `due:` and renders
  as a timestamp. A template's own planning line, such as
  `DEADLINE: %^{deadline}t`, merges with the `due:`/`on:` dates instead of
  producing a second planning line and a literal `t`.

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
