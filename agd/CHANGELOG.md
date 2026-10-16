# Changelog

All notable changes to the `agd` executable are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/) and the
project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Checklists. In capture, `M-⏎` (or `S-⏎` where the terminal reports it)
  adds a `- [ ] step` line under the task. In the edit popup, `x` checks a
  step off, and the heading's `[/]` or `[%]` cookie is counted as Org
  counts it. Lists, both panes and JSON items show the progress, `1/3`.
- `agd undo` reverts your latest command-line change, and
  `agd completions <shell>` prints shell completions, which the Arch and
  Nix packages install.
- `notation` picks how tags and priorities are typed and shown: `#work`
  and `!A` by default, or Org's `:work:` and `#A`.
- `start_view` opens agd on a saved view; Org's built-in `n` view is
  always there.
- Saved views take org-super-agenda `groups`, property matches
  (`TODO="DONE"`, `Effort<15`, `CLOSED>="<-1w>"`) and a `color` per block.
- `todo_keyword_colors` colors keywords as org-todo-keyword-faces does,
  `theme = "auto"` follows the terminal's background, and `nerd_font`
  rounds the pills.
- `agent_inbox` sends every agent capture through one template, without
  dates.
- Marks and bulk actions follow org-agenda (`m`, `*`, `U`, `B`) and write
  as one change.
- In the edit popup, `H` and `L` step through State and Priority as `←`
  and `→` do.
- `?` filters the screen's actions as you type, and
  [docs/KEYS.md](https://github.com/johnnybravo-xyz/agd/blob/main/docs/KEYS.md)
  lists every key.
- `agd agenda --status --json`, `agd capture --json` with the entry's
  location, and `agd setup check --resolve` for interrupted writes.
- MCP `describe` lists the templates, keywords, priorities, views and date
  forms an agent may use.
- The Claude Code plugin's mod: `/agd` opens a pane with your views,
  `/capture` files a task without a Claude turn, and a line above the
  prompt shows the day's counts.

### Changed

- `todo_keywords` takes `#+TODO:` lines, `agenda_span` takes `day`,
  `week`, `month` or a number of days, and the `log_*` settings take
  `off`, `time` or `note`; `tag_alist`, `persistent_tags` and numeric
  priorities are gone. `config.toml` is checked at load, with errors that
  name the key and its accepted values, so a 0.1.0 config may need
  `agd setup import` again or a small edit.
- Today's first section is OVERDUE, missed deadlines first, and every
  surface counts overdue, today and inbox the same way.
- Dates read as Org's date prompt reads them everywhere: `fri`, `+2d`,
  `oct 30`, `fri 9am`, then a repeater and a warning.
- `j`/`k` move like `n`/`p`, `q` quits and `esc` goes back on every
  screen; pickers move with `C-n`/`C-p`, and text fields take Emacs
  minibuffer keys.
- Triage suggests someday after nine reschedules (`reschedule_nudge`), not
  three.
- MCP `capture` takes `template_key`, `set_state` takes `todo`, `plan`
  takes `none` to clear a date, and `agenda` returns at most `limit` rows
  (100). Agent writes are journaled under the agent's name, such as
  `claude-code` or `codex`.
- Error codes follow one rule (`invalid_<thing>`, `unknown_<thing>`,
  `<thing>_not_found`, `<thing>_conflict`, `<thing>_failed`): `stale_ref`
  is now `ref_conflict`, `duplicate_id` `id_conflict`, `editor_dirty`
  `editor_conflict` and `no_change` `invalid_edit`.
- Lock files live in agd's data directory, never beside your org files.
- `agd setup install` covers Codex and OpenCode; Claude Code gets agd from
  its plugin.

### Fixed

- Completing a habit with a `.+2d/3d` repeater, and editing one property
  without rewriting the drawer.
- Capture places entries where org-capture does, with its blank lines, for
  every target shape.
- `agd setup import` reads org-super-agenda groups, property matches and
  capture templates in the editor's order, and skips agenda files Emacs
  cannot find.
- Peek shows a link under your home directory with `~`.
- Fast typing and pasting into capture keep up, and `agd agenda | head`
  ends quietly.
- On Windows, `XDG_CONFIG_HOME` and `XDG_DATA_HOME` are honored when set,
  and messages name files relative to their root.

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

[Unreleased]: https://github.com/johnnybravo-xyz/agd/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/johnnybravo-xyz/agd/compare/agd-v0.0.1...v0.1.0
[0.0.1]: https://github.com/johnnybravo-xyz/agd/releases/tag/agd-v0.0.1
