# Changelog

All notable changes to the `agd` executable are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/) and the
project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

- `notation` chooses how tags and priorities are typed and shown in the
  capture line and every list: `"agd"` (the default) writes `#work` and
  `!A`, `"org"` writes `:work:` and `#A`. `#` followed by digits stays text
  (`Fix #42`), and the tags prompt adds and removes with `+tag` and `-tag` in
  both.
- `todo_keywords` takes one `#+TODO:` line per sequence, read like a file's
  own `#+TODO:` lines:
  `todo_keywords = ["TODO NEXT WAIT(w@/!) | DONE CANCELLED"]`. A line
  without `|` makes its last keyword the done state.
- `agent_inbox = "t"` sends every agent capture through that `[[capture]]`
  template without dates, so agent work reaches your day only through
  Triage; unset, agents choose the template.
- `someday_clears_dates` (on by default) makes Someday remove SCHEDULED and
  an overdue DEADLINE, so the task leaves the agenda.
- `log_done`, `log_repeat`, `log_reschedule` and `log_redeadline` take
  `off`, `time` or `note`.
- A `[[capture]]` template names an outline path with `olpath`, has exactly
  one of `template` and `template_file`, and one of org-capture's target
  shapes: `id`; `file`; `file` with `heading` (file+headline) or `olpath`
  (file+olp); `datetree` with `file` alone or with `olpath`.
  `template_file` is read at load, relative to the first root, with `~`
  expanded.
- `tag_alist` and `persistent_tags` are not config keys; import lists
  `org-tag-alist` and `org-tag-persistent-alist` as not used by agd.
- `agenda_span` and a view block's `span` take `"day"`, `"week"`, `"month"`
  or a number of days (`agenda_span = 10`); 14 days start on the week start,
  as in Org.
- `config.toml` is checked at load, with an error that names the key and
  the accepted values: `week_start` is `monday` or `sunday`; priorities are
  letters A–Z from highest through default to lowest (numeric priorities
  are refused); `agenda_start_day` reads as a day; `deadline_warning_days`
  is 0 or more; `theme` is `auto`, `dark` or `light`; `start_view` and
  `agent_inbox` name a view and a template that exist. A top-level setting
  written below a `[[capture]]`, `[[view]]` or `[[agent_profile]]` record
  is explained as such.
- With no config file and no `~/org`, agd says how to start: create `~/org`,
  run `agd setup import`, or read another directory with `-r DIR`.
- `theme = "auto"` asks the terminal for its background color (OSC 11) and
  picks the light or dark palette.
- `start_view = "KEY"` opens `agd` on that saved view, as the first tab;
  number keys follow the tab order.
- `todo_keyword_colors`, like org-todo-keyword-faces, colors keywords by
  name, such as `{ WAIT = "yellow", CANCELLED = "dim-red" }`, from blue,
  green, yellow, orange, red, dim-red, purple, pink, teal and grey. A view
  block or group takes a `color`, else the color its rows share.
- `nerd_font = true` rounds every pill and chip with Nerd Font glyphs; off
  by default, since terminals without such a font show missing-glyph boxes.
- View blocks take org-super-agenda `groups`: each row goes to the first
  group whose match it passes, `discard` drops rows, `by` groups by
  category, parent, todo or priority, `effort_max`/`effort_min` compare
  `:Effort:` as a duration (`0:10` is ten minutes), and `order` places
  groups, with the rest under "Other items".
- Tags/TODO matches in views and `--match` take property terms with Org's
  semantics, checked against Emacs: `TODO="DONE"`, `PRIORITY="A"`,
  `LEVEL=2`, `CATEGORY="home"`, `Effort<15`, `CLOSED>="<-3w>"`,
  `DEADLINE<="<+1w>"`, `-STATUS="review"`. Regexp terms (`{...}`) are
  refused with `invalid_match`.
- Org's built-in `n` view, "Agenda and all TODOs", is always available
  beside your `[[view]]`s; a `[[view]]` with key `n` takes its place.
- The inbox is where your capture templates file entries: the open entries
  directly under each template's target heading, or at the top level of a
  file a template targets as a whole (datetree targets aside).
- Dates read as Org's date prompt reads them, the same in the CLI, the TUI,
  the capture line and MCP: `today`, `tomorrow`, `.`, a weekday, `+2tue`,
  `-wed`, `+3` (days), `+2d`, `-1w`, `+2m`, `+1y`, `14` (the next 14th),
  `oct 30`, `2026-10-30`, a time alone (`14:00`, `3pm`) or after a day
  (`fri 9am`), then a repeater and a warning (`fri +1w -3d`). A weekday
  names the next one, so `fri` on a Friday is a week ahead, as in Org. A
  date agd cannot read is an error that lists every accepted form.
- `agd capture` takes the line with or without quotes. Its first word is a
  template key when it names a template and more text follows; otherwise
  the whole line goes through the first template. Text that starts with `-`
  goes after `--`. On a terminal it says where the entry went
  (`inbox.org:2  captured · Buy milk`); piped, it prints the item's ref.
- The capture line takes `due:` and `on:` with any date the date prompt
  reads, a time after it (`due:fri 14:00`), then a repeater and a warning
  (`on:sat +1w`, `due:+2d -1d`), plus tags and a priority in the configured
  notation; only a letter cookie sets the priority.
- Capture places entries as org-capture does: at the end of a `file`
  target, under a `file+headline` heading (created at the end of the file
  when missing), a `file+olp` path, a datetree or an `id` heading, with
  org-capture's blank lines. The body goes at `%i`, or after the heading's
  own lines when the template has none; tags align to `tags_column`; a new
  file gets the default mode.
- `agd undo` reverts your latest command-line change that is still in
  effect, never a TUI or agent change, and only where the file still holds
  what it wrote: `inbox.org:2  undid capture · Buy milk`. `--json` prints
  agd.undo/v1.
- `agd completions <shell>` prints completions for bash, zsh, fish, elvish
  and powershell.
- `agd agenda` prints the text agenda as `org-batch-agenda` does, and
  `--csv` prints Emacs's eleven `org-batch-agenda-csv` columns and values,
  with a field's commas written as `;`. Both are compared with pinned Emacs
  byte for byte.
- `agd agenda --status` prints
  `1 overdue · 3 today · 9 inbox · 2 unreviewed`, leaving out zero counts;
  a deadline's early warning is not counted as today.
- `agd agenda --view` without a key runs `start_view`, `--views` lists the
  saved views (the start view first, built-in ones marked), `--from` takes
  any date-prompt answer such as `-1w`, and `--span` takes a number of days.
- JSON: an agenda row's `occurrence` carries `end_time`, `days` (days late
  on an overdue row, minus the days left on an upcoming deadline) and
  `late`; every time reads `HH:MM`, with the file's text in `raw`; a write
  that completes a repeating task carries `repeated` (`done_state`,
  `next_date`); `--view KEY --json` prints agd.view-section/v1 sections
  with `kind`, `color` and `groups` (always present); `--views --json`
  prints agd.views/v1 with the theme, `nerd_font`, each TODO keyword's
  color and the palettes, for a client to draw views as the TUI does.
- On a terminal, `agd agenda` uses 16 colors and an error is one sentence
  after a bold red `error:`; piped output and `NO_COLOR` get plain text.
  Messages name things in single quotes and list the valid choices
  (`unknown view 'x'; the views are 'n'`).
- A command run with `--json` prints every error, usage errors included,
  as agd.error/v1 on stderr.
- A command refuses a flag it would ignore, such as `--status` with
  `--json` or `--view` with `--span`, as a usage error (exit 2).
- `agd agenda | head` and other closed pipes end quietly.
- Error codes follow one rule, `invalid_<thing>`, `unknown_<thing>`,
  `<thing>_not_found`, `<thing>_conflict` or `<thing>_failed`, with one code
  per failure in CLI JSON, MCP and `agd setup check`. A code from 0.1.0
  maps as:

  | 0.1.0 | Code |
  | --- | --- |
  | `ambiguous_path` | `path_conflict` |
  | `config` | `invalid_config` |
  | `duplicate_id` | `id_conflict` |
  | `editor_dirty` | `editor_conflict` |
  | `import` | `import_failed` |
  | `invalid_edit` for a cleared title | `invalid_title` |
  | `io` | `io_failed` |
  | `journal_corrupt` | `invalid_journal` |
  | `journal_error` | `journal_failed` |
  | `journal_recovery_conflict` | `journal_conflict` |
  | `missing_prompt` | `invalid_prompt` |
  | `no_change` | `invalid_edit` |
  | `non_utf8` | `invalid_utf8` |
  | `not_found` | `file_not_found` |
  | `nothing_to_undo` | `invalid_journal_state` (`journal_not_found` from `agd undo`) |
  | `outside_roots`, `path_traversal`, `symlink_write_refused` | `invalid_path` |
  | `parse_drift`, `parse_error` | `parse_failed` |
  | `stale_ref` | `ref_conflict` |
  | `unsupported_import_shape` | `invalid_import` |
  | `unsupported_target`, `unsupported_template` | `invalid_template` |
  | `walk_error` | `walk_failed` |
  | check: `read_error` | `io_failed` (a file), `walk_failed` (a directory) |
  | check: `byte_drift`, `invalid_org` | `parse_failed` |
  | check: `unknown_todo_keyword` | `invalid_todo` |

  The other codes include `unknown_view`, `unknown_profile`,
  `invalid_arguments` (a command line or tool call that does not parse),
  `invalid_batch`, `invalid_id_change`, `command_failed` and
  `encode_failed`.
- Lock files live in agd's data directory (`$XDG_DATA_HOME/agd/locks`),
  never beside your org files. `--dry-run` creates nothing: no directory,
  lock, journal entry or file.
- Reading the journal changes nothing. A write that meets an interrupted
  earlier write whose file changed since is refused with `journal_conflict`
  until `agd setup check --resolve` settles it, keeping the file exactly as
  it is on disk.
- `agd setup check` reports with the error codes, and also reports
  interrupted writes and a `#+PRIORITIES` line agd cannot use (that file
  keeps the configured range).
- Writes refuse a duplicate `:ID:` and a change to an existing `:ID:`, and
  check titles, tags, properties, bodies and notes so the entry writes back
  as one heading. Planning keywords go where org-add-planning-info puts
  them, setting a date removes CLOSED, completing a habit reads its
  repeater as the agenda does, and untouched property lines keep their
  bytes.
- Deadline warnings read `-Nh`, `-Nd`, `-Nw`, `-Nm` and `-Ny` as
  org-get-wdays does; `#+PRIORITIES` applies per file to sorting and the
  priority keys; inherited tags list file tags first, in Org's order.
- Symlinks that leave the roots are skipped and reported by
  `agd setup check`; files created after agd starts join the agenda when
  they match `agenda_files`; headings that share an `:ID:` get distinct refs.
- org-ql queries follow org-ql: `(todo)` is any not-done keyword,
  `(todo "NEXT" "WAIT")` any of those, and `deadline :to N` and
  `scheduled :to N` include past dates; an unknown predicate is an
  `invalid_query` error that lists the supported ones.
- Keys follow org-agenda in the lists: `n`/`j` and `p`/`k` move, `SPC`
  peeks, `.` goes to today, `r`/`g` re-read every file, `R` or `C-c C-w`
  refiles, `S` sends a task to someday, `X` cancels with the file's last
  done keyword, and `y` copies the link org-store-link would make (by
  `:ID:`, else file and heading).
- Marks follow org-agenda: `m` marks a row, `*` marks every row shown, `U`
  unmarks all, and `B` opens a menu for the marked rows: `.` today, `>`
  tomorrow, `w` in a week, `s` or `d` a date you type, `S` someday, `x`
  done, `X` cancel. Marked rows are written as one transaction, all or
  nothing.
- Triage (`T`) opens as a tab and works through the queue it started with,
  moving on after every decision: `.` today, `>` tomorrow, `w` in a week,
  `s` a date you type, `d` move a missed deadline, `x` done, `N` NEXT, `S`
  someday, `X` cancel, `R` or `C-c C-w` refile, `u` undo, `n`/`p` the next
  or previous card, `C-e` $EDITOR.
- The Agents tab (`A`): `a` accepts the marked changes, else the one under
  the cursor; `m`, `*` and `U` mark; `u` undoes a change whose bytes are
  unchanged; `SPC` peeks at the change's diff above its task; `⏎` edits
  the changed task and `C-e` opens it in $EDITOR; `<` shows one agent's
  changes at a time. Each card names the agent, the heading's file and
  line, and for a capture its task.
- `?` is a palette: typing filters the screen's actions, `⏎` runs the chosen
  one, `esc` clears, then closes.
  [docs/KEYS.md](https://github.com/johnnybravo-xyz/agd/blob/main/docs/KEYS.md) lists
  every key, generated from the key table the TUI runs on.
- `q` quits and `esc` goes back on every screen. `C-n`/`C-p` and
  `C-j`/`C-k` move in pickers, every text input takes Emacs minibuffer keys
  (`C-a`, `C-e`, `C-d`, `C-w`, `M-DEL`, `C-u`), and `C-o` opens $EDITOR from
  a popup. `C-e` opens $EDITOR from the lists, Triage, the Agents tab and
  the pane.
- Today's sections are OVERDUE, SCHEDULE, DUE SOON, DOING and INBOX.
  OVERDUE puts missed deadlines first and folds the rest behind a `+N more`
  row; overdue rows give their age (`12d`), while the classic view keeps
  Org's `Sched.12x:`; past times are muted with `▸` on the next one; done
  rows come last and the soonest deadline first. Past-due work is OVERDUE
  on every surface.
- Week opens with the cursor on today, and a list's title names the day
  `f` and `b` move to. An empty list says why: no `.org` files yet, nothing
  matches the filter, or all clear, with tomorrow's count.
- Prompts and the edit popup preview the exact change and refuse a
  priority outside the file's range; a state change the file logs with `@`
  asks for the note first; `x` on a repeating task says when it is next;
  an edit field's text is selected on focus. `/` filters as you type, and
  `esc` restores the filter it started with.
- `u` undoes the latest change made in this session and names it; a change
  edited since is a conflict that says what to do.
- Capture opens on a list of templates with their keys, destinations and
  what a line takes, picked by key or ↑/↓ and `⏎`. A date prompt the line
  answers with `due:` or `on:` is not asked, template date prompts
  (`%^{name}t`, `T`, `u`, `U`) merge into one planning line, and parsed
  dates name their month, and their year when it is not this one.
- Every popup (capture, edit, the prompts, refile, views, keys, peek and the
  diff) shares one design: a rounded accent frame, input in a rounded box,
  Org text in a rounded inset and key hints on the last row. Peek mutes
  inherited tags and shows link text.
- Rows that do not fit end in `…` and keep their tags; files are named
  relative to their root; `~` stands for home only at a whole path
  component; the footer offers at most four hints for the selected row.
- In 16 colors every color role stays readable on the cursor row, and an
  empty `NO_COLOR` keeps color, as no-color.org specifies.
- The pane (`agd --pane`) takes `x`, `X`, `u`, `c` and `C-e`; `⏎` opens the
  full view and `T` Triage at 60 columns or more, and below that it says to
  widen.
- Typing or pasting fast into capture or the edit popup keeps up, and a
  view whose blocks repeat a day draws each row once.
- `agd mcp` has seven tools: `agenda`, `query`, `describe`, `capture`,
  `edit`, `plan` and `set_state`. `describe` (agd.describe/v1) lists the
  capture templates and their prompts, TODO keywords, priorities, views,
  tags and the date forms an agent may write.
- MCP `capture` takes `template_key`, optional, the first template by
  default (the `agent_inbox` template when set), and says when the agent
  inbox changed the template or dropped dates; `set_state` takes `todo`;
  `plan` takes `none` to remove a date.
- MCP `agenda` takes `view`, `profile`, `log`, `limit` (100 by default) and
  `with_body`, and defaults to `agenda_span` and `agenda_start_day` as
  `agd agenda` does; `query` takes `limit` and `with_body`.
- MCP tools refuse unknown arguments and describe every argument in their
  schema; a failed call sets `isError` and answers agd.error/v1. `agenda`,
  `query` and `describe` carry `readOnlyHint`, and `agenda://today` is
  `application/json`.
- MCP writes are journaled under the client's name from each request,
  such as `mcp:claude-code`, with Codex's `codex-mcp-client` as
  `mcp:codex`, so the Agents tab says which agent made each change.
- A Claude Code plugin (`claude plugin marketplace add johnnybravo-xyz/agd`,
  then `claude plugin install agd@johnnybravo-xyz`) brings the MCP server,
  the `agenda` skill and a mod: `/agd` opens a pane with your `start_view`
  or today's agenda and a tab per saved view on its own key, `/capture`
  files a capture line without a Claude turn, and a band above the prompt
  shows the day's counts (the `show_band` option hides it). The pane draws
  rows as the TUI does, in its palettes, keyword colors and `nerd_font`
  chips, and refreshes after agd's write tools. The mod only runs `agd`;
  it needs Claude Code 2.1.296 or later.
- `agd setup install codex|opencode` writes the MCP entry and the `agenda`
  skill to `.agents/skills/agenda/`, leaves another skill of that name
  untouched, edits the project's config in place (keeping comments and key
  order), refuses a differing agd entry or a file it cannot edit safely
  before writing anything, and prints the diff with `--dry-run`. Claude
  Code gets agd from its plugin.
- `agd setup import` reads a block's org-super-agenda groups (reporting the
  selectors it cannot translate), property matches, prefix-key entries such
  as `("g" . "Searches")`, and capture templates in the editor's order,
  each file relative to its root. It writes Emacs's `nil` log settings as
  `off`, skips agenda files Emacs cannot find, stops a reader still running
  after 120 s, reports `org-agenda-skip-function-global` as unsupported,
  and says when the config is already up to date. `-r DIR` sets the
  imported config's root, and `--stdout` prints only the config, with the
  report on stderr.
- Releases publish `SHA256SUMS` with a Sigstore bundle, and
  `packaging/aur` builds the release on Arch with `makepkg`. The Arch and
  Nix packages install bash, zsh and fish completions.

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
