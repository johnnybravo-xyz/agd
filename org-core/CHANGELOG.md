# Changelog

All notable changes to the `agd-core` library are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/) and the
project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Rename agendum to Agenda: `AgendumError` is now `AgdError`, schema strings
  use the `agd.` prefix, and default paths use `agd` directories.

### Added

- `todo.ids = "add-on-write"` gives a heading without `:ID:` a UUID v4 ID on
  its first edit, state change or planning change, as Emacs org-id does.
- Support the `%u`, `%T` and `%%` capture template escapes.

### Fixed

- Skip non-UTF-8 agenda files for reads (`WorkspaceIndex::skipped_files`) and
  refuse writes to them with `non_utf8`, instead of failing every command.
- Treat an `:ID:` that also appears in a non-agenda org file under the roots as
  `duplicate_id`, for ID refs and for hash refs to that heading.
- Apply `agents.capture_to = "inbox"` only to MCP captures, route them to the
  configured `agents.inbox_template`, and drop their planning; validate both
  settings at load time.
- Insert capture titles, bodies and prompt answers literally instead of
  expanding `%U`, `%i` or `%t` typed inside them.

## [0.0.2] - 2026-10-13

### Added

- Source-preserving `OrgDocument` parser that honors in-file `#+TODO` and
  `#+SEQ_TODO` and indexes headline source spans.
- Typed item refs, planning occurrences, inherited tags and categories, and
  deterministic agenda ordering with repeaters, ranges and deadline warnings.
- Tags/TODO matching, an org-ql subset and Emacs-compatible agenda CSV rows.
- Byte-span mutations for TODO state, planning, metadata, capture and refile.
- Advisory locks, Emacs dirty-lock detection, fresh ref validation,
  crash-consistent journals, atomic replacement and guarded undo.
- Rooted discovery, symlink read containment, symlink write refusal and
  machine-readable error responses.
- Versioned import snapshots that cannot broaden configured roots.

## [0.0.1] - 2026-10-13

- First release of `agd-core`, forked from
  [org-mcp-server](https://github.com/szaffarano/org-mcp-server).
