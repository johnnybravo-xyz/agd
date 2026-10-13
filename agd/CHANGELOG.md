# Changelog

All notable changes to the `agd` executable are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/) and the
project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- MCP `capture` takes `properties` (e.g. `REPO`, `BRANCH`, `SOURCE` as an org
  link) recorded in the heading's `:PROPERTIES:` drawer; peek shows them and
  the installed skill asks agents to use them.
- `agd agenda --status` prints `3 slipped · 7 today · 4 inbox · 2 agents` for
  a tmux or herdr status bar.
- `agd agenda --view KEY` runs a saved view, like an org-agenda dispatcher
  key; block views print `=` rules between blocks.
- `agd agenda --log` lists CLOSED entries, like org-agenda log mode.
- `agd import emacs --server NAME` selects a running daemon by socket name.
- `agd import emacs --batch-profile` prints the command it runs before
  executing your Emacs configuration.

### Changed

- Rebuild the TUI to the design: a theme with light, dark, 16-colour and
  `NO_COLOR` variants; tabs for Today, Week, Inbox and Agents; a footer with
  the most-used keys and Emacs-style write confirmations.
- Today groups SLIPPED (capped at three, with badges), SCHEDULE, DUE SOON,
  DOING (with WAIT age) and INBOX (with who captured it), shows effort
  totals, and keeps section headers unselectable. `v` gives the classic
  org-agenda day list; Week shows every day as org-agenda does.
- Peek shows dates with the next repeat, effort, LOGBOOK count, why the row
  is listed, the body with link text and links; `y` copies a link through
  the terminal (OSC 52, tmux-safe).
- Triage is a card per item with age, reschedule count from LOGBOOK or the
  journal, the earned Someday nudge, key chips and a summary; Someday also
  clears an overdue deadline and skips already-demoted tasks.
- Capture shows parsed chips, the destination, the exact org preview and
  fields for `%^{prompt}`; `tab` changes template.
- Agent review shows cards per change (capture, state, date, refile),
  reviewed changes dimmed, `f` to filter by agent and `d` for the raw diff.
- The pane sorts by clock time with ▸ on the next item, collapses overdue
  to a count, and takes over automatically under 60 columns.
- New keys: `a` saved views, `tab`/`1`–`4` tabs, `+`/`-` priority, `l` log
  mode, `g` refresh, `y` copy link, `r` refile outside triage.
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

- Keep the TUI running when a write is refused (`stale_ref`, `editor_dirty`,
  a bad date) and show the reason on the status line.
- Keep the cursor on the same heading when files change or a write
  reorders rows; switching screens starts at the top.
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
