# Changelog

All notable changes to the `agd` executable are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/) and the
project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

- A Claude Code plugin (`claude plugin marketplace add johnnybravo-xyz/agd`,
  then `claude plugin install agd@johnnybravo-xyz`) with the MCP server, the
  `agenda` skill and a mod: `/agd` opens a pane with your `start_view` or
  today's agenda and a tab per saved view on its own key (`d: Dashboard`),
  `/capture KEY text` files a task without a Claude turn, and a line above
  the prompt shows the day's counts. The pane draws rows as the TUI does, in
  its palette (agd's `theme`, or with `auto` Claude Code's), with your
  `todo_keyword_colors` and `nerd_font` chips. The mod only runs `agd`; it
  needs Claude Code 2.1.296 or later.
- `agd setup install` covers Codex and OpenCode, writing their MCP entry and
  the `agenda` skill to `.agents/skills/agenda/`, and leaves another skill of
  that name untouched.
- `start_view = "KEY"` opens `agd` on that saved view, such as your
  dashboard, as the first tab; number keys follow the tab order.
  `agd agenda --view` without a key runs it, and `agd agenda --views` lists
  the views; with `--json`, also the theme, `nerd_font` and each TODO
  keyword's color, for a client to draw them as the TUI does.
- View blocks take org-super-agenda groups: each row goes to the first group
  whose match it passes, `discard` drops rows, `by` groups by category,
  parent, todo or priority, `effort_max`/`effort_min` compare `:Effort:` as a
  duration (`0:10` is ten minutes, unlike Org's `Effort<15`), and `order`
  places groups, with the rest under "Other items". Import reads a block's
  org-super-agenda groups and reports the selectors it cannot translate.
- Saved views run and import property matches, such as a dashboard's
  `TODO="DONE"+CLOSED>="<-3w>"` or `TODO="NEXT"+Effort<15`.
- `todo_keyword_colors`, like org-todo-keyword-faces, colors keywords by name,
  such as `{ WAIT = "yellow", CANCELLED = "dim-red" }`; the colors are blue,
  green, yellow, orange, red, dim-red, purple, pink, teal and grey. A view
  section takes the color its rows share, or the `color` on its block or
  group.
- Sections stand apart as in the design: a view's blocks and groups, like
  Today's sections, are uppercase with a row count and a blank line above,
  and empty sections and days are muted. The Agenda name is a filled badge
  and the current tab is bold and underlined.
- Capture opens on a list of templates with their keys, destinations and
  what a line takes (`due:fri` deadline, `on:+2d` scheduled, `+work` tag,
  `#A` priority), picked by key or ↑/↓ and ⏎. A date prompt the line answers
  with `due:` or `on:` is not asked, template date prompts take dates and
  merge into one planning line, and parsed dates show their month, and their
  year when it is not this one: `deadline Thu 5 Nov`.
- Every popup (capture, edit, the schedule/deadline/priority/tags/filter
  prompts, refile, views, keys, peek and the raw diff) shares one design: a
  rounded accent frame with its title in the accent, two columns of padding,
  a blank row between sections, input in a rounded box, Org text in a
  rounded inset, and key hints as the last row.
- Past-due work is OVERDUE everywhere: Today's first section, the pane's
  badge, Triage's cards and `agd agenda --status` (`3 overdue · 7 today`).
- `X` cancels the selected task in the lists and the pane, with the file's
  own CANCELLED (or CANCELED) keyword.
- `nerd_font = true` rounds every pill and chip with Nerd Font caps; off by
  default, since terminals without such a font show missing-glyph boxes.
- Import keeps capture templates in the order an Emacs config lists them;
  nvim-orgmode's, which a Lua table keeps in no order, come by key. It also
  reads prefix-key entries in `org-agenda-custom-commands`, such as
  `("g" . "Searches")`, and `--stdout` prints only the config, with the
  report on stderr.
- `p` on the Agents tab peeks at the task the selected change touched.
- MCP writes are journaled under the agent's own name, such as
  `mcp:claude-code`, so the Agents tab says which agent made each change.
- Pasting or typing fast into capture or the edit popup keeps up: waiting
  keys are handled together, and the preview is built once per redraw.
- A view whose blocks repeat a header, such as the same day in two agenda
  blocks, draws each row once, and a lead that fills its six columns
  (`in 14d`) keeps a space before the TODO keyword.

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
