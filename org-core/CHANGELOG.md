# Changelog

All notable changes to the `agd-core` library are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/) and the
project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

- Tags/TODO matches take property terms with Org's semantics, checked
  against Emacs: `TODO="DONE"`, `PRIORITY="A"` (the default priority when a
  heading has none), `LEVEL`, `CATEGORY`, planning dates and drawer
  properties; string, number (`Effort<15`) and time (`CLOSED>="<-3w>"`,
  `DEADLINE<="<+1w>"`) comparisons, `-` negation and `OP*`. Regexp terms
  fail with `invalid_match`. `AgendaEngine::todo` and `AgendaEngine::tags`
  take the agenda day, which relative times count from.
- `ViewBlock::groups` (`ViewGroup`) groups a view block's rows as
  org-super-agenda does, including `effort_max`/`effort_min`, and
  `AgendaEngine::view` returns them in `ViewSection::groups`.
- `keyword_color` names a TODO keyword's color as agd draws it, from
  `Settings::todo_keyword_colors` or its state, and every `ViewSection` and
  `ViewSectionGroup` carries its resolved `color` (`COLOR_NAMES`).
- Capture templates support Org's date prompts (`%^{name}t`, `T`, `u`, `U`
  and a bare `%^t`): the answer takes the same input as `due:` and renders as
  a timestamp, and a template's own planning line, such as
  `DEADLINE: %^{deadline}t`, merges with the `due:`/`on:` dates.
  `CapturePrompt::planning` names the keyword a prompt fills, and
  `answered_by` tells whether a capture line's `due:`/`on:` gives it.
- `ResolvedEditorConfig::capture` is a list in the editor's order, each
  `ImportedCapture` with its `key`.

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
