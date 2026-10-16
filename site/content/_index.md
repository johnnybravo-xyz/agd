+++
title = "agd"
template = "project.html"
description = "org-agenda for terminals and coding agents"
+++

<!-- Links and images are relative so the same content also serves from
     https://johnnybravo.xyz/projects/agd/. collect.sh copies agd.gif in. -->

agd reads the Org files and settings you already use with Emacs or
nvim-orgmode, shows them in a keyboard-first terminal UI, and lets coding
agents read and write the same tasks over MCP, without changing a byte it was
not asked to change.

New to Org? [Org](https://orgmode.org) keeps notes and tasks in plain text
files (`* TODO Renew passport`), and org-agenda is its view of what is
scheduled, due and open across those files. agd is that view outside Emacs.

![agd: Today, Week, the Agents tab, Triage, Capture and saved views](agd.gif)

- **21** agenda cases whose text and CSV match Emacs byte for byte
- **23** write cases byte-identical to what Emacs writes
- **0** bytes changed outside the edit you asked for
- **1** binary: the TUI, a narrow pane, a CLI and an MCP server

```sh
cargo install agd --locked
brew install johnnybravo-xyz/agd/agd
nix run github:johnnybravo-xyz/agd
```

[Source](https://github.com/johnnybravo-xyz/agd) ·
[Getting started](https://github.com/johnnybravo-xyz/agd/blob/main/docs/getting-started.md) ·
[Importing](docs/importing/) · [Compatibility](docs/compatibility/) ·
[Safety](docs/safety/) ·
[Keys](https://github.com/johnnybravo-xyz/agd/blob/main/docs/KEYS.md) ·
[Changelog](https://github.com/johnnybravo-xyz/agd/blob/main/agd/CHANGELOG.md)

## Five screens, one for each moment

You glance at today, plan the morning, sweep what is overdue, capture
mid-flow, and check what agents changed. Each has a screen, one key from
Today.

**Today** puts sections in the order you act on them: OVERDUE, SCHEDULE,
DUE SOON, DOING and INBOX. OVERDUE shows three rows, missed deadlines
first, and folds the rest behind `+N more`, so the front page never becomes
a guilt list. `SPC` peeks at an entry and says why it is listed; `v` swaps
in the classic org-agenda day list, with Org's own `Sched.12x:` leaders.

![Today with the peek split](peek.webp)

**Triage** (`T`) is the end-of-day sweep: overdue first, then the inbox, one
card at a time and one key per decision: `.` today, `>` tomorrow, `w` next
week, `s` a date, `d` move a missed deadline, `x` done, `N` NEXT, `S`
someday, `X` cancel, `R` refile.
Cards show how long an item has been overdue and how often it was
rescheduled, and suggest someday after three reschedules. `u` undoes a
step, or the whole pass.

![Triage](triage.webp)

**Capture** (`c`) takes one line, `Renew passport due:fri on:+2d #personal
!A`, through your own capture template and shows the exact Org text before
it writes. A letter picks the template, like `C-c c t`; `C-o` writes the
body in `$EDITOR`. The inbox is where your capture templates file new
entries.

![Capture](capture.webp)

**The Agents tab** lists every agent write with who made it, from which
project, as its smallest meaningful change: a new heading, `TODO → DONE`, a
date move. `a` accepts and writes nothing; `u` undoes it if those bytes are
unchanged; `SPC` shows its diff; `<` shows one agent's changes.

![The Agents tab](review.webp)

**The pane** (`agd --pane`) is a narrow glance beside your agent: timed
items by clock with `▸` on the next one, done items dimmed, overdue as a
count.

![The pane](pane.webp)

## Your setup keeps working

Point agd at your Emacs or Neovim config once; it reads the files, never your
editor's startup, and writes one flat `config.toml`. In-file syntax always
wins.

- `#+TODO` and `#+SEQ_TODO` lines, logging markers like `WAIT(w@/!)`, and
  `#+STARTUP` log options
- `org-agenda-custom-commands` and nvim-orgmode custom commands become saved
  views, block agendas and property matches included, with a block's
  org-super-agenda groups
- Capture templates with `%?` `%U` `%T` `%i` `%^{prompt}` `%^g` and date
  prompts
- `org-agenda-sorting-strategy`, priorities, deadline warnings and
  `org-tags-column`
- CLOSED, LAST_REPEAT and LOGBOOK lines written exactly as Emacs writes them

```sh
agd setup import ~/.doom.d         # or ~/.config/emacs, ~/.config/nvim
```

Every imported setting is listed with the file and line it came from, and
anything left out with the reason. See [importing](docs/importing/).

Every agenda case runs through a pinned Emacs 30.2 with Org 9.7 and through
agd, and the text agenda and CSV must match byte for byte. Every write is
applied by both and compared byte for byte. What is not supported (regexp
matches, diary sexps, skip functions, clocking) is reported, never guessed.
See [compatibility](docs/compatibility/).

![Week, with org-agenda's prefixes](week.webp)

![Saved views from your custom agenda commands](views.webp)

Keys follow org-agenda wherever it has one: `n` `p` to move, `SPC` to peek,
`t` state, `s` and `d` schedule and deadline, `,` `+` `-` priority, `:`
tags, `R` or `C-c C-w` refile, `m` `*` `U` `B` to mark and act on marked
rows, `f` `b` `.` to move in time, `<` and `/` to filter, `v` and `l` for
classic and log mode, `r` `g` to refresh, `u` undo. `⏎` edits an entry's
title, state, priority, tags and dates in place, previewing the exact
change; `C-e` opens it in `$EDITOR`, as `RET` does in org-agenda. `?` lists
every key next to its org-agenda equivalent, filtered as you type.

![Keys and their org-agenda equivalents](help.webp)

## Agents work in your agenda, not around it

`agd mcp` is a local stdio server with seven tools (`agenda`, `query`,
`describe`, `capture`, `edit`, `plan`, `set_state`) and an `agenda://today`
resource. No HTTP and no telemetry.

```sh
claude plugin marketplace add johnnybravo-xyz/agd   # Claude Code: the plugin
claude plugin install agd@johnnybravo-xyz
agd setup install --directory . codex               # or opencode
```

- Every agent write is journaled with the client and project, and waits on
  the Agents tab
- Captures carry `REPO`, `BRANCH` and a `SOURCE` link back to the line
- `agent_inbox = "t"` sends agent captures through your template `t`
  without dates, so agents add work without planning your day
- A changed heading is a clean `ref_conflict` error, never a lost update

## Lives in tmux and herdr

```sh
tmux split-window -h -l 36 'agd --pane'                 # a glance beside your agent

# ~/.tmux.conf
bind T display-popup -E -w 80 -h 22 'agd capture -i'    # prefix T: capture from anywhere
set -g status-right '#(agd agenda --status)'            # 1 overdue · 3 today · 9 inbox · 2 unreviewed
```

Files are watched, so edits from Emacs, nvim or an agent show up as they
happen.

## Org files are your data

Writes patch only their bytes and refuse a stale, ambiguous or duplicate-ID
target. An unsaved Emacs buffer blocks the write; files are re-read under a
lock kept in agd's own data directory and replaced atomically, keeping mode
and line endings. A dry run creates nothing. Undo applies the journaled
inverse only where those bytes are unchanged. agd reads only your
configured roots and never touches the network. See [safety](docs/safety/).

## Try it

```sh
git clone https://github.com/johnnybravo-xyz/agd && cd agd
cargo build --release && AGD=target/release/agd docs/demo/demo.sh
```

The demo needs `python3`, runs on sample data and never touches your files.
