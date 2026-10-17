# Getting started with agd

`agd` is an org-agenda for the terminal and for coding agents. It reads plain
Org files, either the ones you already keep for Emacs or nvim-orgmode or a new
`~/org`, and shows what is due, scheduled and open in a keyboard-first TUI.
Agents such as Claude Code, Codex and OpenCode read and change the same tasks
over MCP. agd writes only the bytes it means to change, journals every write,
and can undo it.

One binary covers four uses:

| Use  | Command                     | For                                                    |
| ---- | --------------------------- | ------------------------------------------------------ |
| TUI  | `agd`                       | planning the day, triage, capture, the Agents tab      |
| Pane | `agd --pane`                | a narrow glance in a tmux or herdr split               |
| CLI  | `agd agenda`, `agd capture` | scripts, status bars, one-line capture                 |
| MCP  | `agd mcp`                   | coding agents, over stdio                              |

If you know Emacs: agd is `org-agenda` and `org-capture` outside Emacs, driven
by your `org-agenda-files`, `org-todo-keywords`, `org-capture-templates` and
`org-agenda-custom-commands`, with org-agenda's keys. If Org is new to you: a
task is a heading in a text file, such as `* TODO Renew passport`, optionally
with a date under it. agd finds those headings and arranges them by day.

## Install

```bash
cargo install agd --locked                 # Rust 1.89+
brew install johnnybravo-xyz/agd/agd
nix run github:johnnybravo-xyz/agd
agd --version
```

Release binaries, the Arch recipe and checksums are in the
[README](../README.md#install), with the lines that install shell completions
from `agd completions bash`, `zsh` or `fish`.

To look around on sample data first, run
`AGD=target/release/agd docs/demo/demo.sh` from a built checkout (it needs
`python3`). It works in a temporary directory and leaves your files and
config alone.

## Start fresh

### Make an Org directory

```bash
mkdir ~/org
```

`~/org` is the default root, the only directory agd reads or writes. Every
`.org` file under it is part of the agenda. A task file looks like this:

```org
* TODO Renew passport                                              :personal:
DEADLINE: <2026-10-30 Fri>
* NEXT Review the release notes                                        :work:
SCHEDULED: <2026-10-19 Mon 10:00>
```

### Capture a task

```bash
agd capture "Renew passport due:fri #personal !A" --dry-run
agd capture "Renew passport due:fri #personal !A"
agd capture "Call the plumber on:+2d"
```

`--dry-run` prints the exact change and writes nothing:

```diff
+* Inbox
+** TODO [#A] Renew passport                                        :personal:
+DEADLINE: <2026-10-23 Fri>
+:PROPERTIES:
+:CREATED: [2026-10-16 Fri 13:30]
+:END:
```

The default template, `t`, files new entries under `* Inbox` in
`~/org/inbox.org`, and creates the file and the heading the first time. That
is your inbox: where your capture templates file new entries.

| Token      | Becomes                                         |
| ---------- | ----------------------------------------------- |
| `due:DATE` | `DEADLINE:`                                     |
| `on:DATE`  | `SCHEDULED:`                                    |
| `#tag`     | a tag (`:tag:` with `notation = "org"`)         |
| `!A`       | a priority cookie (`#A` with `notation = "org"`) |

All other words form the title, and `#` followed by digits stays text
(`Fix #42`). A `DATE` reads as Org's date prompt reads it: `today`,
`tomorrow`, a weekday, `+3d`, `+2w`, `oct 30`, `2026-10-30`, then an optional
time, repeater and warning: `due:fri 14:00`, `due:2026-11-01 +1m -3d`. A
weekday is the next one, so `fri` on a Friday is a week ahead, as in Org.

When the first word names a template, the line goes through that template
(`agd capture t …`); otherwise through the first one. Put `--` before text
that starts with `-`. Without `--dry-run`, the command says where the task
went (`inbox.org:2  captured · Renew passport · deadline Fri 23 Oct`); piped,
it prints the new heading's `ref` for scripts. `agd undo` takes back your
latest command-line change, and running it again the one before.

### Work in the TUI

```bash
agd
```

Today lists overdue tasks first, then the day's schedule, deadlines coming up,
tasks in progress and the inbox. Move with `j`/`k` (or `n`/`p`). `x` marks the
selected task `DONE` and adds `CLOSED:`. `s` schedules it: type `mon`, `+2d`
or `2026-10-30 14:00`, see the change in the prompt, then press `⏎`. `d` sets
a deadline the same way, and `2026-10-30 -3d` adds a warning period. `u`
undoes the last write, `?` lists every key, and `q` quits.

### Write a config

agd runs without a config file. Its defaults are: root `~/org`, every `.org`
file under it, the keywords `TODO NEXT WAIT | DONE CANCELLED`, a week agenda, a
`CLOSED:` timestamp when you finish a task, and the capture template `t`.

To change any of that, create `~/.config/agd/config.toml`. The path is the same
on Linux and macOS. If `XDG_CONFIG_HOME` is set, agd reads
`$XDG_CONFIG_HOME/agd/config.toml` instead, and `agd --config FILE` points one
run at another file.

```toml
roots = ["~/org"]

todo_keywords = ["TODO(t) NEXT(n) WAIT(w@/!) | DONE(d!) CANCELLED(c@)"]
log_into_drawer = true

[[capture]]
key = "t"
desc = "Todo"
file = "inbox.org"
heading = "Inbox"
template = "* TODO %?\n:PROPERTIES:\n:CREATED: %U\n:END:"
```

- `roots` lists the directories agd may read and write. It touches nothing
  outside them.
- `todo_keywords` holds one `#+TODO:` line per sequence, as
  `org-todo-keywords` does: the states before `|` are open, the ones after it
  done. `!` logs the change with a timestamp and `@` asks for a note. A file's
  own `#+TODO:` lines take precedence for that file.
- `log_into_drawer = true` puts those log lines in a `:LOGBOOK:` drawer, as
  `org-log-into-drawer` does.
- `[[capture]]` is an `org-capture-templates` entry. `file` is relative to the
  root and `heading` names the heading to file under (Org's `file+headline`).
  In `template`, `%?` is where the captured text goes and `%U` the current
  inactive timestamp. To file under a nested heading (Org's `file+olp`), use
  `olpath = ["Projects", "Website"]` instead of `heading`; those headings must
  exist, as in Org.

The `t` template above is the one agd uses when the config has none; once you
add a `[[capture]]`, only the templates you list exist. Every other key is top
level, and a key you leave out takes its default. To write tags and
priorities as Org does, `:work:` and `#A`, add `notation = "org"`.
`agd setup check` loads the config, reads your files, and says how many it
checked and what is wrong, if anything.

## Bring your Emacs or Neovim setup

```bash
agd setup import ~/.emacs.d --stdout   # print the config; the report goes to stderr
agd setup import ~/.emacs.d            # write ~/.config/agd/config.toml
agd setup check
```

The path can be a config directory (`~/.doom.d`, `~/.config/emacs`,
`~/.emacs.d`, `~/.config/nvim`) or individual files. agd starts a clean
`emacs -Q` or `nvim --clean` with its own reader, so your init never runs; the
editor still has to be on your `PATH`.

From Emacs it reads `org-directory` (the root on a first import), the agenda
files, TODO keywords, priorities, tags column, logging and agenda options,
`org-capture-templates` and `org-agenda-custom-commands`, including
`org-super-agenda-groups` set on a block. From nvim-orgmode it reads the
options passed to `require("orgmode").setup()`, called directly or through a
lazy.nvim spec. The report gives the source line of each imported setting
and the reason for each one it left out:

```
read 1 Emacs file:
  /home/you/.emacs.d/init.el
imported  roots ~/org, 3 agenda entries, 1 capture template, 1 view
  ✓ org-agenda-files  init.el:2
  ✓ org-agenda-custom-commands  init.el:12
  ~ org-tag-alist  init.el:6: not used by agd
  ✗ capture x  a computed capture target is unsupported
  ✗ view r  regexp matches ({...}) are not supported in '+{^@}'
```

`✓` means imported, `~` imported with a difference or not used by agd, and `✗`
left out. Values computed by your own functions, assignments inside `let`,
hooks or `defun`s, and defaults that Doom modules or packages set are not
visible to the reader. [IMPORTING.md](IMPORTING.md) lists every case.

The file holds only the settings that differ from agd's defaults. Compare
`agd agenda --view KEY` with `C-c a KEY` in Emacs, or import with `--verify`,
which checks each imported view's rows against Emacs's own
`org-batch-agenda-csv`. Once `config.toml` exists, the import prints the diff
and stops; `--force` replaces the file, keeping the keys the import does not
set (`roots`, `editor`, `theme`, `agent_inbox`) but not your comments.

Agenda files outside the roots are left out (`✗ ... agenda entry left out:
outside the roots`). Add their directories to `roots`, such as
`roots = ["~/org", "~/work/notes"]`, and import again with `--force`.

Imported or not, each file's own `#+TODO`, `#+SEQ_TODO`, `#+CATEGORY`,
`#+FILETAGS`, `#+PRIORITIES` and `#+STARTUP` logging options apply to it.

## Daily use

### Tabs and sections

`agd` opens on Today, or on the saved view named by `start_view`, which then
becomes the first tab. The tabs are Today, Week, Inbox and Agents; `tab` and
`S-tab` switch, and `1` to `9` pick one by position. `a` opens any saved view,
as `C-c a` does, `esc` goes back, and `q` quits.

| Today section | Holds                                                                                     |
| ------------- | ----------------------------------------------------------------------------------------- |
| OVERDUE       | missed deadlines, then old scheduled items; the first three, the rest behind `+N more`   |
| SCHEDULE      | today's scheduled and timed items, by clock time                                          |
| DUE SOON      | deadlines within their warning period (`deadline_warning_days`, 14 by default)            |
| DOING         | open tasks past their sequence's first keyword, such as NEXT or WAIT, with no date today  |
| INBOX         | the open entries your capture templates filed, with who captured each                     |

`v` swaps in the classic org-agenda day list. Week is the org-agenda week
view; `f`, `b` and `.` move by week and back to today. `l` toggles log mode,
which adds closed items.

### Keys

| Key               | Action                                          | org-agenda            |
| ----------------- | ----------------------------------------------- | --------------------- |
| `n` `j` / `p` `k` | next / previous row                             | `n` / `p`             |
| `SPC`             | peek at the entry                               | `SPC`                 |
| `t` / `x` / `X`   | next state / done / cancel                      | `t`                   |
| `s` / `d`         | schedule / deadline                             | `C-c C-s` / `C-c C-d` |
| `,` `+` `-` / `:` | priority / tags                                 | `,` `+` `-` / `:`     |
| `R` `C-c C-w`     | refile                                          | `C-c C-w`             |
| `S`               | someday: tag it and drop its dates              |                       |
| `m` `*` `U` `B`   | mark, mark all, unmark all, act on marked rows  | `m` `*` `U` `B`       |
| `<` / `/`         | filter by category / by text or tag             | `<` / `/`             |
| `r` `g`           | re-read every file                              | `r` `g`               |
| `y`               | copy an Org link to the heading (OSC 52)        | `C-c l`               |
| `u`               | undo the last write                             | `C-_`                 |

`?` lists every key; type to filter it, and `⏎` runs the one you pick.
[KEYS.md](KEYS.md) has the same list. `⏎` or `e` opens the edit popup, with
fields for title, state, priority, tags, scheduled and deadline. `tab` moves
between fields, the preview shows the exact change, and `⏎` saves it. `C-e`
opens the heading in your editor from the lists, Triage, the Agents tab and
the pane; in a popup, where `C-e` ends the line, it is `C-o`:

```toml
editor = ["emacsclient", "-n", "+{line}:{col}", "{file}"]   # default: $EDITOR +{line} {file}, else vi
```

agd runs the editor command directly, without a shell.

### Capture and Triage

`c` opens the capture popup, and `agd capture -i` opens it on its own. Press a
template's key, or pick one with `↑`/`↓` and `⏎`, then type the same line as
on the command line. The preview is the exact Org text that will be written,
`tab` changes the template, and `C-o` writes the body in `$EDITOR`.

For subtasks, `M-⏎` (alt-return) adds a checklist line under the task, and
each line is written as `- [ ] line`. `S-⏎` does the same in terminals that
report shift-return apart from return (kitty, WezTerm, Ghostty, foot, or
after Claude Code's `/terminal-setup`). `⌫` on an empty line removes it.
To check the lines off later, press `⏎` on the task: the edit popup lists
them under the dates, and `x` on a line checks it, as `x` marks a task
done. Lists show the progress after the title, `1/3`, and a `[/]` or `[%]`
cookie on the heading is counted as Org counts it.

```text
› Plan the offsite due:fri #work
  - [ ] book venue
  - [ ] send invites
```

`T` starts Triage: overdue items, then the inbox, one card at a time. The keys
are `.` today, `>` tomorrow, `w` in a week, `s` a date you type, `d` move a
missed deadline, `x` done, `N` make it NEXT, `S` someday (adds the `someday`
tag and removes the dates that put it on the agenda), `X` cancel, `R` refile,
`n`/`p` skip forward and back, and `u` to undo a step. After nine
reschedules a card suggests someday; `reschedule_nudge = 5` in `config.toml`
suggests it sooner.

### Saved views and a dashboard

A `[[view]]` is an `org-agenda-custom-commands` entry. Each block has a `kind`
(`agenda`, `todo`, `alltodo`, `tags` or `tags-todo`) and an optional Emacs
tags/TODO `match`, property terms included, such as
`TODO="DONE"+CLOSED>="<-1w>"`. `groups` work like org-super-agenda groups: a
row goes to the first group it matches, and rows no group takes appear under
"Other items".

```toml
start_view = "d"   # top-level keys go above the first [[capture]] or [[view]]

[[view]]
key = "d"
desc = "Dashboard"
blocks = [
  { kind = "agenda", span = "day" },
  { kind = "tags-todo", match = "work", header = "Work", groups = [
    { name = "Next", match = "/NEXT" },
    { name = "Quick wins", effort_max = "0:15" },
    { name = "Waiting", match = "/WAIT" },
  ] },
]
```

Groups also take `discard = true` to drop their rows, `by` (`category`,
`parent`, `todo` or `priority`) to make one group per value, `effort_min`, and
`order`. Org's built-in `n` view ("Agenda and all TODOs") stays beside yours;
a `[[view]]` with key `n` replaces it.

On the command line, `agd agenda --views` lists the views, `agd agenda --view d`
prints one in the org-agenda layout, and `agd agenda --view` alone prints
`start_view`.

### A pane and a status bar

```bash
tmux split-window -h -l 36 'agd --pane'

# ~/.tmux.conf
bind T display-popup -E -w 80 -h 22 'agd capture -i'   # prefix T: capture from anywhere
set -g status-right '#(agd agenda --status)'           # 1 overdue · 3 today · 9 inbox · 2 unreviewed
set -g set-clipboard on                                # lets y reach your clipboard
```

The pane shows today's timed items by clock, with overdue items as a count and
done items dimmed. Its keys are `x` done, `X` cancel, `u` undo, `c` capture and
`C-e` to edit; at 60 columns or more, `T` triages and `⏎` opens the full TUI. A
terminal narrower than 60 columns switches to the pane by itself. Click a row
to select it, and click it again to open the full TUI there.
[herdr](https://herdr.dev), a terminal workspace manager for AI coding
agents, runs `agd --pane` in a pane the same way. The TUI and the pane watch
your files, so edits from Emacs, nvim or an agent appear as they happen.

`agd agenda --status` prints one line of counts and leaves out the zeros.

## Work with agents

### Claude Code

Install agd first. The plugin runs the `agd` on your `PATH`, which reads your
`config.toml`.

```bash
claude plugin marketplace add johnnybravo-xyz/agd
claude plugin install agd@johnnybravo-xyz
```

The plugin brings:

- the MCP server, with the tools `agenda`, `query`, `describe`, `capture`,
  `edit`, `plan` and `set_state`, plus an `agenda://today` resource;
- the `agenda` skill, which tells Claude to call `describe` before writing,
  to read an item before changing it, to decide from the current state after
  a `ref_conflict` error instead of retrying, and to record `REPO`, `BRANCH`
  and `SOURCE` properties on tasks it files from your code;
- a mod, which needs Claude Code 2.1.296 or later:
  - `/agd` opens or closes a pane beside the transcript on your `start_view`,
    or today's agenda, with a tab per saved view. It uses your `theme`,
    `todo_keyword_colors` and `nerd_font`.
  - `/capture t text`, such as `/capture t Review the deploy checklist due:fri`,
    files the task at once through `agd capture`, without a Claude turn.
  - a line above the prompt with the day's counts, which refreshes every
    minute and after Claude changes a task. The plugin's `show_band` option
    hides it.

Then ask in plain words: "what is due this week?", "add a task to fix the flaky
login test, scheduled tomorrow, tagged work", or "mark the release-notes task
done". Your agenda reaches the model only when it calls a tool.

### Codex, OpenCode, Pi and oh-my-pi

Install agd once for every harness:

```bash
agd setup install --dry-run codex pi   # show the diff first
agd setup install codex opencode pi omp
```

The `agenda` skill goes to `~/.agents/skills/agenda/SKILL.md`, the Agent
Skills location that Codex, OpenCode, Pi, oh-my-pi and Gemini CLI all read
in every project. Each harness you name also gets agd's MCP server in its
user config:

| Harness  | MCP config                                                       |
| -------- | ---------------------------------------------------------------- |
| codex    | `~/.codex/config.toml` (`$CODEX_HOME`)                           |
| opencode | `~/.config/opencode/opencode.json` (`$XDG_CONFIG_HOME/opencode`) |
| pi       | `~/.pi/agent/mcp.json` (Pi 1.0 or later; `$PI_CODING_AGENT_DIR`) |
| omp      | `~/.omp/agent/mcp.json`                                          |

The command adds an `agd` server entry to each file and keeps everything else
in it, comments included. If a file already has a different `agd` entry, or
another skill named `agenda` exists, it stops before writing anything. Any
other harness that reads `~/.agents/skills` and takes a stdio MCP server
works the same way: point it at `agd mcp`.

Pi and oh-my-pi also get `/capture` from agd's extension, in
`~/.pi/agent/extensions/agd.ts` or `~/.omp/agent/extensions/agd.ts`:
`/capture t Review the deploy checklist due:fri` files the task at once
through `agd capture`, without a model turn, as in Claude Code. Start a new
session, or run `/reload` in Pi, to load it.

### Review what agents changed

agd journals every write in `~/.local/share/agd/journal.jsonl` (or under
`$XDG_DATA_HOME/agd`), with who made it and from which project directory.
The Agents tab lists the agents' changes, each under its client's name, such
as `claude-code` or `codex`, and its label counts the unreviewed ones
(`Agents ● 2`). `a` accepts the marked changes, else the selected one;
accepting writes nothing. `m`, `*` and `U` mark, `u` undoes a change if the
bytes it wrote are still there, `SPC` shows its diff and then the task, `<`
shows one agent's changes at a time, `⏎` edits the task, and `C-e` opens it
in `$EDITOR`.

To let agents add work without planning your day, send every agent capture
through one template, with its dates removed:

```toml
agent_inbox = "t"
```

## What agd does to your files

- It reads and writes only under `roots`. Symlinks that lead outside the
  roots are skipped and reported, and writes never go through a symlink.
- A write takes a lock in agd's data directory (`~/.local/share/agd/locks`),
  never beside your files, re-reads the file, patches only its own bytes,
  keeps the file's mode and line endings, journals the inverse patch, and
  renames the result into place. `--dry-run` creates nothing.
- When it cannot be sure, it refuses and writes nothing: `ref_conflict` (the
  heading changed since it was read), `id_conflict` (the same `:ID:` occurs
  twice), `editor_conflict` (Emacs holds unsaved changes to the file), or
  `invalid_path` (a symlink, or a path outside the roots).
- Undo applies the inverse patch only where the bytes it wrote are unchanged;
  anything else is reported as a conflict. `u` in the TUI undoes this
  session's own changes, `agd undo` your command-line changes, and the Agents
  tab an agent's.
- If a write is interrupted and the file changes before the next one, writes
  to that file stop with `journal_conflict` until `agd setup check --resolve`
  keeps the file as it is.
- It makes no network requests. MCP runs over stdio, and import runs your
  local editor in batch mode.

`agd setup check` reports parse drift, invalid UTF-8, duplicate IDs and
interrupted writes with file and line, and exits with status 1 when it finds
errors. See [SAFETY.md](SAFETY.md) and [COMPATIBILITY.md](COMPATIBILITY.md)
for details.

## Troubleshooting

**``start_view` is a top-level setting; move it above the first [[capture]] ...``**
or **``unknown field `tui` ``**: every setting is a top-level key, and TOML
gives keys written after a `[[capture]]` or `[[view]]` line to that record.
Move them above the first `[[...]]` line; the error lists the valid keys.

**`no config at ... and no ~/org to read`**: create `~/org`, import your
editor's settings, or run `agd -r DIR` to read another directory once.

**`capture heading not found on level 1: Projects`**: an `olpath` template
needs its headings in the file. Add them, or use `heading`, which agd creates.

**`unsupported date '...'`**: the error lists every form agd reads, such as
`fri`, `+2d`, `oct 30`, `2026-10-30 14:00` and `fri +1w -3d`.

**Priorities**: agd uses letters (`highest_priority = "A"`,
`lowest_priority = "C"`); numeric priorities are not supported, and
`agd setup check` reports a `#+PRIORITIES` line it cannot use.

**No colors, or the wrong ones**: a non-empty `NO_COLOR` turns color off,
and state shows with bold, dim, reverse and strikethrough. `theme = "auto"`
asks the terminal for its background; `theme = "dark"` or `"light"` sets it.
Without truecolor (`COLORTERM=truecolor`) agd uses 16 colors. Boxes around
priority chips mean `nerd_font = true` without a Nerd Font in the terminal;
the default is `false`.
