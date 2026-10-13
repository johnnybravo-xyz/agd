# Changelog

All notable changes to the `agd` executable are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/) and the
project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `agd agenda --log` lists CLOSED entries, like org-agenda log mode.
- `agd import emacs --server NAME` selects a running daemon by socket name.
- `agd import emacs --batch-profile` prints the command it runs before
  executing your Emacs configuration.

### Changed

- Trim the CLI to `agd` (Today; `--pane`), `agenda`, `capture`, `mcp` and
  `setup import|install|check`. People write through the TUI and agents
  through MCP; `todo`, `q`, `state`, `schedule`, `deadline`, `edit`, `open`,
  `refile`, `undo`, `config`, `tui`, `pane`, `list`, `read`, `outline`,
  `heading`, `element-by-id` and `search` are gone.
- `agd agenda` prints an org-agenda style buffer and takes `--match`.
- `agd capture <key> "<text>"` parses the TUI tokens (`due:`, `on:`, `+tag`,
  `#A`) instead of separate date, tag and priority flags.
- Rename the product from agendum to Agenda. Identifiers use `agd`: the
  config and data directories (`~/.config/agd`, `~/.local/share/agd`), JSON
  schemas (`agd.item/v1` and siblings), the installed `agd` skill, and
  `AGD_*` environment variables.
- Replace the oversized keyboard-help overlay with a compact, structured panel
  and higher-contrast selection styling.

### Fixed

- Substitute `{col}` in `editor.args` for `agd open` and the TUI.
- Keep CLI reads working when an agenda file is not UTF-8 and warn about the
  skipped file on stderr.
- Keep the selected task in view when cycling its TODO state, including when
  the updated state reorders agenda rows.
- Probe the installed nvim-orgmode version by capability instead of rejecting
  every release except 0.7.5.

## [0.0.2] - 2026-10-13

### Added

- `agd check` with human and JSON reports for unreadable input, invalid UTF-8,
  parser byte drift and duplicate heading IDs.
- Native day/week/month agenda, TODO matcher, org-ql subset and
  Emacs-compatible CSV output.
- Transactional capture, state, planning, edit, refile and guarded undo with
  exact dry-run diffs and stable JSON errors.
- Today, triage, capture, Agent review and pane terminal workflows with file
  watching and `NO_COLOR` support.
- Versioned Emacs and nvim-orgmode imports with explicit profiles and
  verification reports.
- Claude Code, Codex and OpenCode installers that add no session hook by
  default.
- cargo-dist release binaries, Homebrew and AUR packaging, signed checksums
  and dependency policy gates.

### Changed

- The stdio MCP server ships inside `agd` as `agd mcp`, with six write-aware
  tools and one Today resource; every write journals client and project.
- `agd config init` writes the strict native schema shared by every surface.

## [0.0.1] - 2026-10-13

- First release of `agd`, forked from
  [org-mcp-server](https://github.com/szaffarano/org-mcp-server).
