# Agenda (`agd`)

[![CI](https://github.com/johnnybravo-xyz/agd/actions/workflows/ci.yml/badge.svg)](https://github.com/johnnybravo-xyz/agd/actions/workflows/ci.yml)
[![agd on crates.io](https://img.shields.io/crates/v/agd.svg)](https://crates.io/crates/agd)
[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](https://github.com/johnnybravo-xyz/agd/blob/main/LICENSE)

An org-agenda for the terminal and for coding agents. `agd` reads the Org
files and settings you already use with Emacs or nvim-orgmode, shows them in a
keyboard-first TUI, and lets agents read and write the same tasks over MCP,
without changing a byte it was not asked to change.

New to Org? [Org](https://orgmode.org) keeps notes and tasks in plain text
files (`* TODO Renew passport`), and org-agenda is its view of what is
scheduled, due and open across those files. agd is that view outside Emacs.

![agd: Today, Week, the Agents tab, Triage, Capture and saved views](https://raw.githubusercontent.com/johnnybravo-xyz/agd/main/docs/release/agd.gif)

- **Your setup keeps working.** `#+TODO` lines, keyword logging markers,
  `#+STARTUP` options, custom agenda commands, capture templates, sorting
  strategy and tag columns are honored, not reinterpreted. Agenda rows, their
  order and every write are checked against a pinned Emacs.
- **Built for the five moments you use an agenda:** a glance in a split pane,
  the morning plan in Today, an end-of-day Triage, a one-line capture from
  anywhere, and the Agents tab for what coding agents changed.
- **Safe with your data.** Writes patch only the bytes they mean to, refuse
  stale or ambiguous targets, respect Emacs lock files, and can be undone.

Try it on sample data without touching your files (needs `python3`):

```bash
git clone https://github.com/johnnybravo-xyz/agd.git && cd agd
cargo build --release && AGD=target/release/agd docs/demo/demo.sh
```

## Install

```bash
cargo install agd --locked                 # Rust 1.89+
brew install johnnybravo-xyz/agd/agd
nix run github:johnnybravo-xyz/agd
```

On Arch, build the release with the recipe in
[`packaging/aur`](https://github.com/johnnybravo-xyz/agd/tree/main/packaging/aur)
until the AUR package is published:

```bash
git clone https://github.com/johnnybravo-xyz/agd && cd agd/packaging/aur && makepkg -si
```

Tagged releases publish checksummed binaries for Linux, macOS and Windows with
a Sigstore bundle for `SHA256SUMS`.

Shell completions:

```bash
agd completions zsh > "${fpath[1]}/_agd"
agd completions bash > ~/.local/share/bash-completion/completions/agd
agd completions fish > ~/.config/fish/completions/agd.fish
```

## First run

Point agd at your editor config once, then start:

```bash
agd setup import ~/.doom.d               # or ~/.config/emacs, ~/.emacs.d, ~/.config/nvim
agd setup check
agd
```

The import runs a clean `emacs -Q` or `nvim --clean` (the editor must be on
your `PATH`) that reads your config files without running your startup, and
writes `~/.config/agd/config.toml` with your org directory, agenda files, TODO
keywords, logging, priorities, capture templates and custom agenda commands.
It reports each setting with the file and line it came from, and anything it
left out with the reason. From then on `config.toml` is yours to edit.
[IMPORTING.md](https://github.com/johnnybravo-xyz/agd/blob/main/docs/IMPORTING.md)
lists what imports and what does not.

New to Org? Skip the import: `mkdir ~/org`, and agd reads it with Org's
defaults. [Getting started](https://github.com/johnnybravo-xyz/agd/blob/main/docs/getting-started.md)
walks through a first day.

## The TUI

`agd` opens **Today**, or the saved view named by `start_view`, which then
sits first in the tabs. The other tabs are **Week**, **Inbox** and
**Agents**; `T` opens **Triage**, `c` captures, and `a` opens your saved views
like `C-c a`.

| Screen | What it is for |
| --- | --- |
| Today | Sections in the order you act: OVERDUE (three rows, the rest behind `+N more`), SCHEDULE by clock time, DUE SOON with countdowns, DOING with WAIT ages, INBOX with who captured each entry. `v` swaps in the classic org-agenda day list. |
| Triage | Overdue, then the inbox, one card at a time, one key per decision. Cards show how long an item has been overdue and how often it was rescheduled, and suggest someday after three reschedules. `u` undoes a step, or the whole pass at the end. |
| Capture | One line through your capture templates; the preview is the exact Org text that will be written. |
| Agents | Every agent write with who made it, from which project, as its smallest meaningful change, until you accept or undo it. |
| Pane (`agd --pane`) | A narrow glance: overdue as a count, timed items by clock with `▸` on the next one, done items dimmed, inbox and Agents counts. A terminal under 60 columns shows it on its own. |

The inbox is where your capture templates file new entries. Keys follow
org-agenda wherever it has one. The essentials:

| Key | Does | org-agenda |
| --- | --- | --- |
| `n` `j` / `p` `k` | next / previous row | `n` / `p` |
| `SPC` | peek at the entry | `SPC` |
| `⏎` | edit title, state, priority, tags and dates in a popup that previews the change | |
| `C-e` | open the heading in `$EDITOR` (`C-o` in a popup) | `RET` |
| `t` `x` `X` | next TODO state, done, cancel | `t` |
| `s` `d` | schedule, deadline | `C-c C-s` `C-c C-d` |
| `,` `+` `-` `:` | priority, tags | `,` `+` `-` `:` |
| `R` `C-c C-w` | refile | `C-c C-w` |
| `m` `*` `U` `B` | mark, mark all, unmark all, act on the marked rows | `m` `*` `U` `B` |
| `f` `b` `.` | next day, previous day, today | `f` `b` `.` |
| `r` `g` | re-read every file | `r` `g` |
| `u` | undo the last write | `C-_` |
| `?` | every key, filtered as you type | |

In Triage: `.` today, `>` tomorrow, `w` in a week, `s` a date, `d` move a
missed deadline, `x` done, `N` NEXT, `S` someday, `X` cancel, `R` refile,
`u` undo. On the Agents tab: `a` accepts (and writes nothing), `m` `*` `U`
mark, `u` undoes a change whose bytes are unchanged, `SPC` shows its diff, `<`
shows one agent's changes. Every key, screen by screen:
[KEYS.md](https://github.com/johnnybravo-xyz/agd/blob/main/docs/KEYS.md).

Colors carry state only: lateness, deadlines, done, focus and one hue per
agent. `theme = "auto"` asks the terminal for its background (`dark` or
`light` sets it); terminals without truecolor get 16 colors, and `NO_COLOR`
turns color off.

## Capture

The same line works in `agd capture`, the TUI's `c` and Claude Code's
`/capture`:

```bash
agd capture "Renew passport due:fri #personal !A"
agd capture t "Dentist due:fri 14:00"
agd capture "Pay rent due:2026-11-01 +1m -3d #home"
agd capture -- "-v ignores the config file"
```

- The first word is a template key when it names one of your
  `[[capture]]` templates; otherwise the line goes through the first one.
- `due:DATE` sets a deadline and `on:DATE` schedules. A date is Org's date
  prompt: `fri`, `+2d`, `oct 30`, `2026-10-30`, then an optional time
  (`fri 14:00`, `9am`), repeater (`+1m`) and warning (`-3d`).
- `#tag` adds a tag and `!A` sets the priority; with `notation = "org"`
  they are `:tag:` and `#A`. `#` followed by digits stays text: `Fix #42`.
- Put `--` before text that starts with `-`.

Priorities are letters (`highest_priority = "A"`); numeric priorities are
not supported.

## tmux and herdr

```bash
tmux split-window -h -l 36 'agd --pane'                 # a glance beside your agent

# ~/.tmux.conf
bind T display-popup -E -w 80 -h 22 'agd capture -i'    # prefix T: capture from anywhere
set -g status-right '#(agd agenda --status)'            # 1 overdue · 3 today · 9 inbox · 2 unreviewed
set -g set-clipboard on                                 # lets y copy a link to your clipboard
```

For your own status bar or script, `agd agenda --status --json` gives the
same counts and the overdue rows as `agd.status/v1`.

[herdr](https://herdr.dev) is a terminal workspace manager for AI coding
agents; give `agd --pane` a pane there the same way. The pane and the TUI
watch your files, so changes from Emacs, nvim or an agent show up as they
happen. Click a pane row to select it, and click it again to open the full
TUI there.

## Agents

`agd mcp` is a stdio MCP server with seven tools (`agenda`, `query`,
`describe`, `capture`, `edit`, `plan`, `set_state`) and an `agenda://today`
resource. It never opens a network port. `capture` takes a `template_key`
(the first template when left out) and `set_state` a `todo` keyword; a failed
call is marked `isError` and answers `agd.error/v1`.

In Claude Code, install agd as a plugin. It brings the MCP server, the
`agenda` skill, and a mod (Claude Code 2.1.296 or later) that adds `/agd`, a
pane beside the transcript with your `start_view` (or today's agenda) and a
tab per saved view; `/capture t text`, which files a task at once without a
Claude turn; and a line above the prompt with the day's counts. It runs the
`agd` on your `PATH`, which reads your `config.toml`, so the pane draws like
the TUI.

```bash
claude plugin marketplace add johnnybravo-xyz/agd
claude plugin install agd@johnnybravo-xyz
```

For Codex and OpenCode, install the MCP server and skill in a project (the
skill goes to `.agents/skills/agenda/`, which both read; `--dry-run` shows
the diff first):

```bash
agd setup install --directory . codex       # or opencode
```

- Every agent write is journaled with the client's name and project, and
  waits on the Agents tab until you accept or undo it.
- Agents pass `properties` such as `REPO`, `BRANCH` and `SOURCE` (an Org link
  back to the file and line) with a capture, so a task says where it came from.
- `agent_inbox = "t"` sends every agent capture through template `t` without
  dates, so agents can add work but not plan your day.
- Your agenda reaches a model only when it calls a tool.

## Command line

```text
agd                    the TUI
agd --pane             the narrow pane
agd agenda             the agenda as org-agenda prints it, or --json, --csv, --status
agd capture            one line through a template, or the popup with -i
agd undo               undo your latest change from the command line
agd mcp                the stdio MCP server for coding agents
agd setup import       your Emacs or Neovim org settings into config.toml
agd setup install      the MCP server and skill for Codex or OpenCode
agd setup check        parse drift, invalid UTF-8, duplicate IDs, interrupted writes
agd completions SHELL  completions for bash, zsh, fish, elvish or powershell
```

```bash
agd agenda --span day                        # org-agenda's day layout
agd agenda --view n                          # a saved view, like C-c a n
agd agenda --match 'work-someday/NEXT|TODO' --json
agd agenda --csv                             # org-batch-agenda-csv's eleven columns
agd capture "Renew passport due:fri #personal !A" --dry-run
agd undo
agd setup check --resolve                    # settle interrupted writes, keeping the files as they are
```

On a terminal the agenda is in color; piped, it is exactly Org's text. JSON
items carry a stable `ref`: the heading's `:ID:` when it is unique, otherwise
its file, line and a digest of the heading. Errors print as `error:` and a
sentence; with `--json` they are `agd.error/v1` objects with a stable code
such as `ref_conflict`, `id_conflict`, `editor_conflict` or `invalid_date`.

## Configuration

One flat file: `~/.config/agd/config.toml` (or `$XDG_CONFIG_HOME/agd`) on
Linux and macOS alike; `--config` picks another. Every setting is a top-level
key; only capture templates, views and agent profiles are lists of records.
Missing keys take their defaults; unknown keys are errors. The journal lives
in `~/.local/share/agd` (or `$XDG_DATA_HOME/agd`).

```toml
roots = ["~/org"]                 # the only directories agd reads or writes
agenda_files = ["**/*.org"]       # files, directories or globs under the roots; empty = every .org file

todo_keywords = ["TODO(t) NEXT(n) WAIT(w@/!) | DONE(d!) CANCELLED(c@)"]  # one #+TODO: line per sequence
todo_keyword_colors = { WAIT = "yellow", CANCELLED = "dim-red" }      # org-todo-keyword-faces
tags_column = -77                 # org-tags-column

agenda_span = "week"              # day, week, month or a number of days
deadline_warning_days = 14
log_done = "time"                 # off, time or note; also log_repeat, log_reschedule
log_into_drawer = true            # and log_redeadline; #+STARTUP options win per file

editor = ["emacsclient", "-n", "+{line}:{col}", "{file}"]  # run directly, no shell
theme = "auto"                    # asks the terminal for its background; or dark, light
notation = "agd"                  # #tag and !A; "org" for :tag: and #A
start_view = "w"                  # open on this view; leave it out to open on Today
agent_inbox = "t"                 # agents capture through this template, without dates

[[capture]]
key = "t"
desc = "Todo"
file = "inbox.org"
heading = "Inbox"
template = "* TODO %?\n:PROPERTIES:\n:CREATED: %U\n:END:"

[[view]]                          # an org-agenda custom command
key = "w"
desc = "Work"
blocks = [
  { kind = "agenda", span = "day" },
  { kind = "tags-todo", match = "work/NEXT|WAIT", header = "Next and waiting", color = "orange" },
  { kind = "tags", match = 'TODO="DONE"+CLOSED>="<-1w>"', header = "Done this week" },
  { kind = "alltodo", header = "Backlog", groups = [   # org-super-agenda groups
    { match = "/HOLD", discard = true },
    { name = "Overdue", match = 'DEADLINE<"<today>"' },
    { name = "Quick", effort_max = "0:15" },
    { by = "category" },
  ] },
]
```

Org's built-in `n` view ("Agenda and all TODOs") stays beside your own
views; a `[[view]]` with key `n` replaces it.

## Compatibility

Agenda rows and their order, and the text and CSV agendas, are compared
byte for byte with a pinned Emacs 30.2 and its Org 9.7; writes are compared
byte for byte with what the same Emacs writes. Full tables are in
[COMPATIBILITY.md](https://github.com/johnnybravo-xyz/agd/blob/main/docs/COMPATIBILITY.md).

| Area | Status |
| --- | --- |
| Scheduled, deadlines, warnings, past-scheduled and overdue reminders | Exact |
| `+`, `++`, `.+` and habit repeaters, ranges, heading timestamps | Exact |
| Default and configured `org-agenda-sorting-strategy` row order | Exact |
| Tag inheritance, `#+CATEGORY`, `#+FILETAGS`, Unicode tags and weekdays | Exact |
| Custom agenda commands: agenda, todo, alltodo, tags, tags-todo blocks, property matches | Exact rows; other options reported |
| State changes, CLOSED, repeats, LOGBOOK notes, reschedule logs, tag alignment | Byte-identical to Emacs |
| Regexp matches, diary sexps, skip functions, clocking | Not supported; reported, never evaluated |

## Safety

- agd reads and writes only under `roots`. Symlinks that leave the roots are
  skipped and reported, and writes never go through a symlink.
- A write takes a lock in agd's data directory, never beside your files,
  refuses a file Emacs holds unsaved changes for, re-reads the file, patches
  only its bytes, keeps the file's mode and line endings, journals the inverse
  patch with fsync, and renames atomically. `--dry-run` creates nothing.
- Undo applies the inverse only where those bytes are unchanged; anything
  else is a conflict, never an overwrite.
- No network access at runtime. Editor import is an explicit local command.

Details: [SAFETY.md](https://github.com/johnnybravo-xyz/agd/blob/main/docs/SAFETY.md).

## Documentation

- [Getting started](https://github.com/johnnybravo-xyz/agd/blob/main/docs/getting-started.md): a first day with agd, from an empty `~/org` or your Emacs setup
- [KEYS.md](https://github.com/johnnybravo-xyz/agd/blob/main/docs/KEYS.md): every key, screen by screen
- [IMPORTING.md](https://github.com/johnnybravo-xyz/agd/blob/main/docs/IMPORTING.md), [COMPATIBILITY.md](https://github.com/johnnybravo-xyz/agd/blob/main/docs/COMPATIBILITY.md), [SAFETY.md](https://github.com/johnnybravo-xyz/agd/blob/main/docs/SAFETY.md)
- [DESIGN.md](https://github.com/johnnybravo-xyz/agd/blob/main/docs/DESIGN.md): the words, formats, keys and look agd follows
- [ARCHITECTURE.md](https://github.com/johnnybravo-xyz/agd/blob/main/ARCHITECTURE.md), [CONTRIBUTING.md](https://github.com/johnnybravo-xyz/agd/blob/main/CONTRIBUTING.md), [SECURITY.md](https://github.com/johnnybravo-xyz/agd/blob/main/SECURITY.md)

## Development

Development commands are in the `justfile`, release tasks in the `Makefile`
(`make` lists them in order). Both run cargo through `scripts/cargo`, which on
macOS outside `nix develop` links with Apple's clang and SDK.

```bash
just check        # fmt, clippy, tests, the Claude Code plugin, nix flake check
just run agenda --status
just install      # release build to ~/.local/bin
just oracles      # compare with the pinned Emacs and Neovim (needs Docker)
```

[CONTRIBUTING.md](https://github.com/johnnybravo-xyz/agd/blob/main/CONTRIBUTING.md)
has the rest, and `vhs docs/release/agd.tape` records the GIF above.

## Origin and license

A fork of [`szaffarano/org-mcp-server`](https://github.com/szaffarano/org-mcp-server).
The combined work is GPL-3.0-or-later; upstream-derived code remains available
under its MIT license in
[`LICENSE-MIT`](https://github.com/johnnybravo-xyz/agd/blob/main/LICENSE-MIT).
