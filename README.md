# agd

[![CI](https://github.com/johnnybravo-xyz/agd/actions/workflows/ci.yml/badge.svg)](https://github.com/johnnybravo-xyz/agd/actions/workflows/ci.yml)
[![agd on crates.io](https://img.shields.io/crates/v/agd.svg)](https://crates.io/crates/agd)
[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](https://github.com/johnnybravo-xyz/agd/blob/main/LICENSE)

agd is an org-agenda for the terminal and for coding agents. It reads the
Org files and the Emacs or Neovim config you already have, shows them in a
keyboard-first TUI, and lets Claude Code, Codex and other MCP clients read
and write the same tasks. What is due this week, or a task for Thursday, is
a glance or one line away from the pane you are working in, without
opening an editor.

Your files stay yours: agd changes only the bytes it was asked to change,
checks its agenda and its writes against a pinned Emacs, and keeps every
agent write in a tab until you accept or undo it.

![agd: Today, Week, Triage, Capture and a saved view](https://raw.githubusercontent.com/johnnybravo-xyz/agd/main/docs/release/agd.gif)

New to Org? [Org](https://orgmode.org) keeps notes and tasks in plain text
files (`* TODO Renew passport`), and org-agenda is its view of what is
scheduled, due and open across them. agd is that view outside Emacs, and it
starts on an empty `~/org` with Org's defaults.

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

On Arch, `cd packaging/aur && makepkg -si` in a clone builds the release.
Tagged releases publish binaries for Linux, macOS and Windows with a
Sigstore-signed `SHA256SUMS`. Completions come from
`agd completions bash|zsh|fish|elvish|powershell`.

## Start

Bring your Emacs or Neovim setup once, then run `agd`:

```bash
agd setup import ~/.doom.d     # or ~/.config/emacs, ~/.emacs.d, ~/.config/nvim
agd setup check
agd
```

The import runs a clean `emacs -Q` or `nvim --clean` that reads your config
files without running your startup, and writes `~/.config/agd/config.toml`
with your org directory, agenda files, TODO keywords, logging, priorities,
capture templates and custom agenda commands. It names the file and line
each setting came from, and why it left anything out. Starting fresh needs
no config: `mkdir ~/org`, then `agd`.
[Getting started](https://github.com/johnnybravo-xyz/agd/blob/main/docs/getting-started.md)
walks through both, and a first day with agents.

## The TUI

- **Today** opens first, or the saved view named by `start_view`:
  OVERDUE, SCHEDULE, DUE SOON, DOING and INBOX, in the order you act on
  them. `v` swaps in org-agenda's classic day list.
- **Week**, **Inbox** and **Agents** are tabs, and `a` opens your saved
  views like `C-c a`.
- **Triage** (`T`) walks the overdue items, then the inbox, one card and one
  key per decision. `u` undoes a step, or the whole pass.
- **Capture** (`c`) takes one line and previews the exact Org text it will
  write.
- **Agents** lists every agent write, who made it and from which project,
  until you accept or undo it.
- `agd --pane` is a narrow glance for a split: overdue as a count, the day's
  times with `▸` on the next, and the inbox and Agents counts.

Keys follow org-agenda wherever it has one:

| Key               | Does                                                              | org-agenda          |
| ----------------- | ----------------------------------------------------------------- | ------------------- |
| `n` `j` / `p` `k` | next / previous row                                               | `n` / `p`           |
| `SPC`             | peek at the entry                                                 | `SPC`               |
| `⏎`               | edit title, state, priority, tags, dates and checklist in a popup |                     |
| `C-e`             | open the heading in `$EDITOR`                                     | `RET`               |
| `t` `x` `X`       | next state, done, cancel                                          | `t`                 |
| `s` `d`           | schedule, deadline                                                | `C-c C-s` `C-c C-d` |
| `,` `+` `-` `:`   | priority, tags                                                    | `,` `+` `-` `:`     |
| `R`               | refile                                                            | `C-c C-w`           |
| `m` `*` `U` `B`   | mark, mark all, unmark all, act on the marked rows                | `m` `*` `U` `B`     |
| `f` `b` `.`       | next day, previous day, today                                     | `f` `b` `.`         |
| `u`               | undo the last write                                               | `C-_`               |
| `?`               | every key on this screen, filtered as you type                    |                     |

Every key, screen by screen, is in
[KEYS.md](https://github.com/johnnybravo-xyz/agd/blob/main/docs/KEYS.md).
Color only carries meaning: lateness, deadlines, done, focus and agents.
`theme = "auto"` follows the terminal's background, and `NO_COLOR` is
respected.

## Capture

The same line works in `agd capture`, the TUI's `c` and Claude Code's
`/capture`:

```bash
agd capture "Renew passport due:fri #personal !A"
agd capture t "Dentist on:fri 14:00"
agd capture "Pay rent due:2026-11-01 +1m -3d #home"
```

The first word picks a template when it is one of your `[[capture]]` keys.
`due:` sets a deadline and `on:` schedules, with any date Org's date prompt
reads (`fri`, `+2d`, `oct 30`, `fri 9am`), then a repeater and a warning.
`#tag` and `!A` add a tag and a priority, or `:tag:` and `#A` with
`notation = "org"`; `#42` stays text. Text that starts with `-` goes after
`--`. In the TUI, `M-⏎` adds a checklist line under the task, `x` checks
one off in the edit popup, and lists show the progress, `1/3`.

## With agents

![Claude Code captures into agd, then agd shows what you, Claude and Codex added](https://raw.githubusercontent.com/johnnybravo-xyz/agd/main/docs/release/agd-story.gif)

In Claude Code, install agd as a plugin:

```bash
claude plugin marketplace add johnnybravo-xyz/agd
claude plugin install agd@johnnybravo-xyz
```

It brings the MCP server, the `agenda` skill and a mod (Claude Code 2.1.296
or later): `/agd` opens a pane beside the transcript with your start view
and saved views, `/capture t text` files a task without a Claude turn, and a
line above the prompt shows the day's counts. For Codex and OpenCode,
install the MCP server and skill in a project (`--dry-run` shows the diff):

```bash
agd setup install --directory . codex      # or opencode
```

`agd mcp` is a stdio MCP server with seven tools: `agenda`, `query`,
`describe`, `capture`, `edit`, `plan` and `set_state`. It never opens a
network port, and your agenda reaches a model only when it calls one of
them.

- Every agent write is journaled under the agent's name and project, and
  waits on the Agents tab until you accept or undo it.
- Agents add `REPO`, `BRANCH` and a `SOURCE` link to what they capture, so
  a task says where it came from.
- `agent_inbox = "t"` sends agent captures through template `t` without
  dates: agents can add work, but not plan your day.

## Panes and status bars

```bash
tmux split-window -h -l 36 'agd --pane'                 # a glance beside your agent

# ~/.tmux.conf
bind T display-popup -E -w 80 -h 22 'agd capture -i'    # prefix T: capture from anywhere
set -g status-right '#(agd agenda --status)'            # 1 overdue · 3 today · 9 inbox · 2 unreviewed
```

[herdr](https://herdr.dev) panes take `agd --pane` the same way. The pane
and the TUI watch your files, so changes from Emacs, Neovim or an agent show
up as they happen.

## Command line

```text
agd                    the TUI
agd --pane             the narrow pane
agd agenda             the agenda as org-agenda prints it, or --json, --csv, --status
agd capture            one line through a template, or the popup with -i
agd undo               undo your latest command-line change
agd mcp                the stdio MCP server for coding agents
agd setup import       your Emacs or Neovim org settings into config.toml
agd setup install      the MCP server and skill for Codex or OpenCode
agd setup check        parse drift, invalid UTF-8, duplicate IDs, interrupted writes
agd completions SHELL  shell completions
```

```bash
agd agenda --view n                                  # a saved view, like C-c a n
agd agenda --match 'work-someday/NEXT|TODO' --json
agd capture "Renew passport due:fri" --dry-run
```

Piped, `agd agenda` prints exactly Org's text. JSON items carry a stable
`ref`, and errors with `--json` are `agd.error/v1` objects with a code such
as `ref_conflict` or `invalid_date`.

## Configuration

One flat file, `~/.config/agd/config.toml` (or `$XDG_CONFIG_HOME/agd`) on
Linux and macOS alike. Missing keys take their defaults and unknown keys are
errors. Values reuse Org's names:

```toml
roots = ["~/org"]                 # the only directories agd reads or writes
agenda_files = ["**/*.org"]       # files, directories or globs under the roots

todo_keywords = ["TODO(t) NEXT(n) WAIT(w@/!) | DONE(d!) CANCELLED(c@)"]  # #+TODO: lines
todo_keyword_colors = { WAIT = "yellow", CANCELLED = "dim-red" }
agenda_span = "week"              # day, week, month or a number of days
log_done = "time"                 # off, time or note; also log_repeat, log_reschedule
log_into_drawer = true

editor = ["emacsclient", "-n", "+{line}:{col}", "{file}"]
notation = "agd"                  # #tag and !A; "org" for :tag: and #A
start_view = "w"                  # open on this view instead of Today
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
  { kind = "tags-todo", match = "work/NEXT|WAIT", header = "Next and waiting" },
  { kind = "alltodo", header = "Backlog", groups = [   # org-super-agenda groups
    { name = "Overdue", match = 'DEADLINE<"<today>"' },
    { name = "Quick", effort_max = "0:15" },
    { by = "category" },
  ] },
]
```

## Compatibility and safety

Agenda rows and their order, the text and CSV agendas, and every write are
compared byte for byte with a pinned Emacs 30.2 and Org 9.7. Repeaters and
habits, sorting strategies, tag inheritance, custom agenda command blocks
and logging match; regexp matches, diary sexps, skip functions and clocking
are reported and never evaluated.

- agd reads and writes only under `roots`, and never writes through a
  symlink.
- A write locks, re-reads the file, refuses a stale or ambiguous target or a
  file Emacs holds unsaved changes for, patches only its bytes, journals the
  inverse and renames atomically.
- Undo applies only where those bytes are unchanged; anything else is a
  conflict, never an overwrite.
- No network access at runtime.

## More

- [Getting started](https://github.com/johnnybravo-xyz/agd/blob/main/docs/getting-started.md),
  [KEYS.md](https://github.com/johnnybravo-xyz/agd/blob/main/docs/KEYS.md),
  [IMPORTING.md](https://github.com/johnnybravo-xyz/agd/blob/main/docs/IMPORTING.md),
  [COMPATIBILITY.md](https://github.com/johnnybravo-xyz/agd/blob/main/docs/COMPATIBILITY.md),
  [SAFETY.md](https://github.com/johnnybravo-xyz/agd/blob/main/docs/SAFETY.md)
- [DESIGN.md](https://github.com/johnnybravo-xyz/agd/blob/main/docs/DESIGN.md):
  the words, formats, keys and look every surface follows
- [ARCHITECTURE.md](https://github.com/johnnybravo-xyz/agd/blob/main/ARCHITECTURE.md),
  [CONTRIBUTING.md](https://github.com/johnnybravo-xyz/agd/blob/main/CONTRIBUTING.md),
  [SECURITY.md](https://github.com/johnnybravo-xyz/agd/blob/main/SECURITY.md)

`just check` runs fmt, clippy, the tests, the plugin checks and
`nix flake check`; `just oracles` compares agd with the pinned Emacs and
Neovim in Docker.

agd started as a fork of [`szaffarano/org-mcp-server`](https://github.com/szaffarano/org-mcp-server).
The combined work is GPL-3.0-or-later; upstream-derived code remains
available under its MIT license in
[`LICENSE-MIT`](https://github.com/johnnybravo-xyz/agd/blob/main/LICENSE-MIT).
