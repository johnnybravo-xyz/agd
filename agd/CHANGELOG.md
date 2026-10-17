# agd changelog

This file records user-visible changes to the `agd` executable. It follows
[Keep a Changelog](https://keepachangelog.com/en/1.0.0/) and
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `agd setup install --detected` now configures supported clients whose user
  config directories already exist. The setup guide now covers shared
  Codex/desktop configuration and Paseo's provider-owned MCP setup.
- Pi and oh-my-pi now receive `/capture`, matching Claude Code. `agd setup
  install pi` or `agd setup install omp` adds an extension that files tasks
  through `agd capture` without a model turn.
- `agd capture --json` now includes `summary`, the same concise result shown on
  a terminal.

### Changed

- `agd setup install` now installs agent support once at user scope. The
  `agenda` skill goes to `~/.agents/skills`, which Codex, OpenCode, Pi,
  oh-my-pi, and Gemini CLI can share. Named `codex`, `opencode`, `pi`, and `omp`
  harnesses receive the MCP server in their user configuration.
- Per-project installation and `--directory` were removed. Existing project
  copies under `.agents/skills/agenda` and project-local `agd` server entries
  may be deleted.

## [0.2.0] - 2026-10-16

### Added

- Checklist capture and editing. `M-Enter`, or `S-Enter` in terminals that
  distinguish it, adds `- [ ]` items during capture. The edit popup toggles
  items with `x`, updates `[/]` and `[%]` cookies like Org, and exposes progress
  such as `1/3` in lists, panes, and JSON.
- `agd undo` for the latest command-line change.
- `agd completions <shell>` for bash, zsh, fish, elvish, and PowerShell. Arch
  and Nix packages install the generated completions.
- `notation`, selecting `#work` and `!A` or Org's `:work:` and `#A` for tags and
  priorities.
- `start_view`, which opens the TUI on a saved view. Org's built-in `n` view
  remains available.
- org-super-agenda-style `groups`, property matches such as `TODO="DONE"`,
  `Effort<15`, and `CLOSED>="<-1w>"`, and a block `color` for saved views.
- `todo_keyword_colors`, automatic terminal-background detection through
  `theme = "auto"`, and `nerd_font` presentation.
- `agent_inbox`, which routes agent captures through one template and removes
  dates.
- Org-style marking and bulk actions through `m`, `*`, `U`, and `B`, committed
  as one change.
- `H` and `L` navigation for State and Priority in the edit popup.
- Searchable `?` help and a generated
  [keyboard reference](https://github.com/johnnybravo-xyz/agd/blob/main/docs/KEYS.md).
- JSON status output, JSON capture output with the entry location,
  and `agd setup check --resolve` for interrupted writes.
- MCP `describe`, exposing capture templates, TODO keywords, priority ranges,
  saved views, and accepted date forms.
- The Claude Code mod: `/agd` opens a pane, `/capture` files without a model
  turn, and a status band shows current counts.

### Changed

- `todo_keywords` now accepts `#+TODO:` sequence syntax. `agenda_span` accepts
  `day`, `week`, `month`, or a number of days. Logging settings accept `off`,
  `time`, or `note`.
- `tag_alist`, `persistent_tags`, and numeric priorities were removed.
  Configuration now validates at load time and names the invalid key and
  accepted values. Version 0.1.0 configurations may require another import or a
  small manual edit.
- Today now begins with OVERDUE, ordered by missed deadlines before past
  scheduled items. Every interface calculates overdue, today, and inbox counts
  consistently.
- Dates now use Org-style prompt syntax everywhere: `fri`, `+2d`, `oct 30`,
  `fri 9am`, followed by optional repeaters and warnings.
- List movement now uses `j`/`k` as aliases for `n`/`p`; `q` quits and `Esc`
  returns consistently. Pickers use `C-n`/`C-p`; text fields use Emacs
  minibuffer keys.
- Triage now suggests someday after nine reschedules, configurable through
  `reschedule_nudge`.
- MCP `capture` now uses `template_key`, `set_state` uses `todo`, `plan` accepts
  `none` to clear a date, and `agenda` returns at most 100 rows unless narrowed.
- Agent journal actors now include the MCP client name, such as `claude-code`
  or `codex`.
- Error codes now follow one naming scheme. `stale_ref` became `ref_conflict`,
  `duplicate_id` became `id_conflict`, `editor_dirty` became
  `editor_conflict`, and `no_change` became `invalid_edit`.
- Lock files moved to `agd`'s data directory instead of living beside Org
  files.
- `agd setup install` now supports Codex and OpenCode; Claude Code installation
  moved to the plugin.

### Fixed

- Completing habits with `.+2d/3d` repeaters.
- Editing one property without rewriting the rest of its drawer.
- Capture placement and blank lines for every supported org-capture target.
- Import order for capture templates and org-super-agenda groups, property
  matches, and missing agenda files.
- Peek now abbreviates paths below the home directory with `~`.
- Fast typing and paste handling in capture.
- `agd agenda | head` now exits quietly when the pipe closes.
- Windows now honors `XDG_CONFIG_HOME` and `XDG_DATA_HOME`, and messages show
  paths relative to their configured root.

## [0.1.0] - 2026-10-14

The first public release provided an Org agenda for terminals and coding agents,
using existing Emacs or nvim-orgmode files.

### Terminal UI

- Today opened with SLIPPED, SCHEDULE, DUE SOON, DOING, and INBOX sections.
  Classic org-agenda layout was available through `v`; Week, Inbox, and Agents
  were tabs; `a` ran saved views.
- Triage reviewed overdue and inbox work one card at a time and could undo a
  step or the complete pass.
- Capture accepted `due:fri on:+2d +tag #A`, used configured templates, and
  previewed the exact Org write.
- The edit popup changed title, state, priority, tags, and dates with an exact
  preview. `C-e` opened the heading in `$EDITOR`.
- Agent review grouped writes by client and project for acceptance or undo.
- `agd --pane` provided a compact tmux or herdr view.
- Keys followed org-agenda; the theme supported light, dark, 16-color, and
  `NO_COLOR` environments. File watching surfaced external changes.

### Command line and agents

- `agd agenda` emitted Org-compatible text, JSON, or CSV, with saved views,
  tags/TODO matches, log mode, and status output.
- `agd capture KEY "text"` captured one line or opened an interactive popup.
- `agd mcp` exposed six stdio tools and `agenda://today`, with agent and project
  provenance in the journal.
- `agd setup import` read Emacs or Neovim configuration and reported imported
  and omitted settings with source locations.
- `agd setup install` configured Claude Code, Codex, or OpenCode.
- `agd setup check` reported parse drift, invalid UTF-8, and duplicate IDs.

### Configuration and packaging

- Configuration used one flat file under `~/.config/agd`, with XDG support.
- The journal lived under `~/.local/share/agd`.
- Releases included signed Linux, macOS, and Windows binaries, a Homebrew tap,
  and a Nix flake.

## [0.0.1] - 2026-10-13

- First `agd` release, forked from
  [org-mcp-server](https://github.com/szaffarano/org-mcp-server).

[Unreleased]: https://github.com/johnnybravo-xyz/agd/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/johnnybravo-xyz/agd/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/johnnybravo-xyz/agd/compare/agd-v0.0.1...v0.1.0
[0.0.1]: https://github.com/johnnybravo-xyz/agd/releases/tag/agd-v0.0.1
