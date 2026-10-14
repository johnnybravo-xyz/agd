# Changelog

All notable changes to the `agd-core` library are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/) and the
project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- `Settings` is flat and matches `config.toml` key for key; `captures`,
  `views` and `agent_profiles` are lists looked up with `capture_template`,
  `view` and `agent_profile`, and `editor_command` replaces
  `EditorSettings::args_for`. `RootSet::new` takes roots and agenda patterns.
  `apply_import` applies resolved editor settings over a `Settings` and
  `render_config` writes them; `ImportSnapshot`, its schemas and the
  per-group settings structs are removed.
- `default_config_path` and `WriteEngine::with_default_journal` follow the XDG
  base directories on every Unix, macOS included: `$XDG_CONFIG_HOME/agd` or
  `~/.config/agd`, and `$XDG_DATA_HOME/agd` or `~/.local/share/agd`. Windows
  keeps its platform directories. `paths::config_dir` and `paths::data_dir`
  expose them.

- Undo reverts its own bytes even when the file changed elsewhere: journaled
  patches keep their surrounding lines and are relocated by them; a change to
  the edited lines is still `undo_conflict`. Older entries keep the
  whole-file rule.
- Remove the unused `agenda.time_grid` setting.
- Import org-agenda custom commands from Emacs and nvim-orgmode as views,
  including block agendas and descriptionless commands; views with matches agd
  cannot run are left out with a diagnostic. `ImportSnapshot::commands` is
  replaced by `settings.views` plus diagnostics.
- `todo` view blocks with keywords list done-state keywords too, as Emacs does.
- `agd-core` enables chrono's `serde` feature itself instead of relying on
  workspace feature unification.
- Remove the upstream `OrgMode`, `OrgConfig`, `LoggingConfig` and
  `OrgModeError` API, including its line-reassembly writes; `load_settings`
  now returns `AgdError` with code `config`. Every caller uses the byte-span
  `WriteEngine`, `WorkspaceIndex` and `AgendaEngine`.
- Merge import snapshots from their default path (`imports/emacs.toml`,
  `imports/nvim.toml`), so `agd setup import` takes effect without extra
  configuration.
- Rename agendum to Agenda: `AgendumError` is now `AgdError`, schema strings
  use the `agd.` prefix, and default paths use `agd` directories.

### Added

- `CaptureDraft::properties` adds entries to the template's property drawer
  or opens one after the headline.
- `AgendaEngine::inbox` lists unfinished inbox headings for every surface.
- `[tui] theme` (auto, dark, light), `triage.someday_clears_scheduled` and
  `capture_prompts` for the TUI's capture popup.
- `WriteContext::now` pins the clock for timestamps written into Org text.
- `agenda.log_reschedule` and `agenda.log_redeadline`, imported from Emacs.
- Honor `agenda.sorting_strategy` with org-agenda's keys (`time`, `urgency`,
  `priority`, `category`, `alpha`, `todo-state`, each `-up`/`-down`, plus
  `category-keep` and `habit-*`); the default is Emacs's own. Imports read the
  agenda strategy and map Org < 9.7 `priority-*` to `urgency-*`.
- Saved views (`[views.KEY]`): org-agenda custom commands as blocks of
  `agenda`, `todo`, `alltodo`, `tags` and `tags-todo`, run by
  `AgendaEngine::view`. Org's built-in `n` view ships by default.
- `AgendaEngine::tags` and `items_csv` for tags blocks and TODO-list CSV rows.
- Tags/TODO matches accept Emacs forms: bare tags (`work`), `&`, `|`
  alternatives, `/-WAIT` and `/!`; property and regexp matches fail with a
  clear `invalid_match`.
- `EditorSettings::args_for` substitutes `{file}`, `{line}` and `{col}`.
- `todo.ids = "add-on-write"` gives a heading without `:ID:` a UUID v4 ID on
  its first edit, state change or planning change, as Emacs org-id does.
- Support the `%u`, `%T` and `%%` capture template escapes.

### Fixed

- Journal captures with the new item's ref and its canonical path, so
  provenance and Agent review can find what an agent captured.
- Read planning only from the line right after the headline, as Org does;
  a second SCHEDULED/DEADLINE line is an ordinary timestamp. Captures write
  SCHEDULED and DEADLINE on one planning line instead of two.
- Align tags to `org-tags-column` (`tags.column`, imported from Emacs and
  nvim-orgmode) whenever a headline is rewritten, as Emacs does, instead of
  collapsing them to one space.
- Write logs the way Emacs does: `State "DONE" from "TODO" [time]` entries
  for `!`/`@` keyword markers and `org-log-repeat`, `CLOSING NOTE` for
  `org-log-done` note, "Rescheduled from"/"New deadline from" for
  `org-log-reschedule`/`org-log-redeadline`, at the top of an existing
  LOGBOOK or inline per `org-log-into-drawer`; `#+STARTUP` log options
  override the configuration per file. Notes previously always opened a new
  drawer.
- Repeating tasks return to the head of their keyword's sequence, set
  `LAST_REPEAT` only when repeats are logged, and new dates keep the old
  repeater and warning cookie.
- Read every `#+TODO` line of a file; a sequence without `|` makes its last
  keyword the done state.
- Skip Emacs `.#name` lock symlinks while discovering agenda files; an
  unsaved Emacs buffer no longer makes every read fail with `walk_error`.
- Order agenda rows as org-agenda does by default (time-up, urgency-down,
  category-keep): urgency from priority, days overdue and days late, ties in
  file order with deadlines before scheduled before timestamps. TODO lists
  sort by priority, then file order, instead of by category name.
- An explicit configuration path that does not exist is an error instead of
  silently using defaults.
- Parse Org syntax seen in real setups: habit repeaters (`.+2d/3d`), Unicode
  tags and TODO keywords, non-English weekday names (`<2026-10-14 周三 10:30>`),
  and multi-day ranges that use the end time on their last day.
- Ignore timestamps in src/example/export/comment blocks, fixed-width lines and
  link targets, and do not treat `: SCHEDULED: …` as a planning line.
- Agenda errors name the `file:line` of the heading that caused them.
- Match org-agenda for planning: overdue repeating deadlines show on today
  counted from the base date, scheduled repeats show only on the base date and
  after today, deadline warnings use the base date, and leaders use Org's
  `Sched.%2dx:` / `%2d d. ago:` widths.
- List active timestamps written on the headline line (`* Birthday <… +1y>`)
  and drop them from the agenda title, as org-agenda does.
- List CLOSED entries only with `AgendaQuery::log_mode`, like org-agenda log
  mode; they previously appeared in every agenda.
- Print dry-run diff headers with the plain file path instead of `a//abs`.
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
