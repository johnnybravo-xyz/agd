# Changelog

All notable changes to the `agd` executable are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/) and the
project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `nerd_font = true` rounds every pill and chip with Nerd Font caps; off by
  default, since terminals without such a font show missing-glyph boxes.

### Changed

- Every popup (capture, edit, the schedule/deadline/priority/tags/filter
  prompts, refile, views, keys, peek and the raw diff) shares one design: a
  rounded accent frame with its title in the accent, two columns of
  padding, a blank row between sections, input in a rounded box, Org text
  in a rounded inset, and key hints as the last row. Chips are spaced
  apart, and the footer separates the location from the keys.

### Fixed

- Capture templates with `DEADLINE: %^{deadline}t` no longer write a second,
  broken `DEADLINE: t` line: date prompts take dates, and `due:`/`on:` merge
  into the template's planning line.

## [0.1.0] - 2026-10-14

First public release: an org-agenda for the terminal and for coding agents,
reading the Org files you already use with Emacs or nvim-orgmode.

### Terminal UI

- `agd` opens Today: SLIPPED (three rows, the rest wait in Triage),
  SCHEDULE, DUE SOON, DOING with WAIT ages, and INBOX with who captured each
  item. `v` swaps in the classic org-agenda day list; Week, Inbox and Agents
  are tabs, and `a` runs saved views like `C-c a`.
- Triage (`T`) takes overdue items, then the inbox, one card at a time, with
  how long each slipped and how often it was rescheduled; `u` undoes a step
  or the whole pass.
- Capture (`c`) takes one line with `due:fri on:+2d +tag #A` through your
  templates and previews the exact Org text before writing.
- `⏎` edits an entry's title, state, priority, tags and dates in a popup that
  previews the exact change; `ctrl-e` opens the entry in `$EDITOR` from
  anywhere.
- Agent review (`A`) lists every agent write with who made it and from which
  project, to accept or undo.
- `agd --pane` is a 32-column glance for a tmux or herdr split.
- Keys follow org-agenda; the theme follows the terminal's light or dark
  background, falls back to 16 colours, and honours `NO_COLOR`. Files are
  watched, so edits from your editor or an agent show up as they happen.

### Command line and agents

- `agd agenda` prints the agenda as org-agenda does, or as `--json` or
  `--csv`; `--view KEY` runs a saved view, `--match` a tags/TODO match,
  `--log` closed items, and `--status` a one-line summary for status bars.
- `agd capture KEY "text"` captures one line, or opens the popup with `-i`.
- `agd mcp` is a stdio MCP server with six tools and an `agenda://today`
  resource; every agent write is journaled with its client and project.
- `agd setup import <path>` reads your Emacs or Neovim config files once and
  writes `config.toml`, reporting each setting with its file and line and
  everything it leaves out with the reason.
- `agd setup install` adds the MCP server and skill to Claude Code, Codex or
  OpenCode; `agd setup check` reports parse drift, invalid UTF-8 and
  duplicate IDs.

### Configuration and packaging

- One flat `~/.config/agd/config.toml` (or `$XDG_CONFIG_HOME/agd`) on Linux
  and macOS; the journal lives in `~/.local/share/agd`.
- Release binaries for Linux and macOS (x86_64 and aarch64) and Windows
  (x86_64) with signed checksums, a Homebrew tap, and a Nix flake.

## [0.0.1] - 2026-10-13

- First release of `agd`, forked from
  [org-mcp-server](https://github.com/szaffarano/org-mcp-server).
