# Getting started with agd

`agd` is a local-first Org agenda for terminals and coding agents. The full TUI,
narrow pane, CLI, and stdio MCP server all read the same Org files and use the
same source-preserving write engine.

## Install and try the demo

```bash
cargo install agd --locked                 # Rust 1.89+
brew install johnnybravo-xyz/agd/agd
nix run github:johnnybravo-xyz/agd
agd --version
```

Release binaries, signed checksums, and the Arch recipe are listed in the
[README](../README.md#install). Generate shell completions with
`agd completions bash|zsh|fish|elvish|powershell`.

To explore sample data without touching your files:

```bash
git clone https://github.com/johnnybravo-xyz/agd.git
cd agd
cargo build --release
AGD=target/release/agd docs/demo/demo.sh
```

The demo requires `python3` and isolates `HOME`, configuration, application
data, and Org files in a temporary directory.

## Start from existing settings or new Org files

### Import an existing Emacs or Neovim setup

```bash
agd setup import ~/.emacs.d --stdout
agd setup import ~/.emacs.d
agd setup check
agd
```

The input may be an editor directory such as `~/.doom.d`, `~/.config/emacs`,
`~/.emacs.d`, or `~/.config/nvim`, or one or more configuration files. The
importer runs `emacs -Q` or `nvim --clean`; the selected editor must be on
`PATH`, but normal startup, daemons, and plugin managers do not run.

Review the report before using the generated configuration. See the
[import reference](reference.md#import-reference) for discovery, supported
settings, and re-import behavior.

### Start with new Org files

Org stores notes and tasks in plain text. A heading with a TODO keyword is a
task; a heading without one is a note:

```org
* TODO Renew passport
```

A **scheduled date** says when you plan to work on a task. A **deadline** says
when it must be complete and becomes visible during its warning period. Most
tasks need neither; some need one; few need both.

Create the default root and capture a first task:

```bash
mkdir -p ~/org
agd capture "Renew passport due:fri #personal !A"
agd
```

Without a configuration file, `agd` uses `~/org`, reads every `.org` file under
it, uses `TODO NEXT WAIT | DONE CANCELLED`, adds a `CLOSED:` timestamp on
completion, shows a week agenda, and captures TODO headings under Inbox with
template `t`.

## Configure roots, capture, and views

The default configuration is `~/.config/agd/config.toml`, or
`$XDG_CONFIG_HOME/agd/config.toml`. `agd --config FILE` selects another file for
one invocation.

```toml
roots = ["~/org"]
todo_keywords = ["TODO(t) NEXT(n) WAIT(w@/!) | DONE(d!) CANCELLED(c@)"]
log_into_drawer = true
start_view = "d"

[[capture]]
key = "t"
desc = "Todo"
file = "inbox.org"
heading = "Inbox"
template = "* TODO %?\n:PROPERTIES:\n:CREATED: %U\n:END:"

[[capture]]
key = "n"
desc = "Note"
file = "notes.org"
heading = "Notes"
template = "* %?\n:PROPERTIES:\n:CREATED: %U\n:END:\n%i"

[[capture]]
key = "m"
desc = "Meeting"
file = "calendar.org"
heading = "Meetings"
template = "* %?"

[[capture]]
key = "j"
desc = "Journal"
file = "journal.org"
datetree = true
template = "* %U %?\n%i"

[[view]]
key = "d"
desc = "Dashboard"
blocks = [
  { kind = "agenda", span = "day" },
  { kind = "todo", match = "NEXT", header = "Next" },
  { kind = "todo", match = "WAIT", header = "Waiting" },
]
```

Important rules:

- `roots` is the authorization boundary for reads and writes.
- Every ordinary setting is top-level. Put it before the first `[[capture]]` or
  `[[view]]`; TOML assigns later keys to the current array record.
- A file's `#+TODO`, `#+SEQ_TODO`, `#+CATEGORY`, `#+FILETAGS`, `#+PRIORITIES`,
  and `#+STARTUP` directives override configured defaults for that file.
- Keywords before `|` are open; keywords after it are complete. `!` logs a
  timestamp and `@` requests a note.
- A capture target's `file` is relative to a root. `heading` can be created;
  every component of `olpath` must already exist; `datetree = true` creates
  year, month, and day headings.
- `%?` receives captured text, `%U` inserts the current inactive timestamp,
  `%i` receives body text, and `%^{name}` defines a prompt.
- Defining any `[[capture]]` record replaces the default template set.
- Saved-view blocks may be `agenda`, `todo`, `alltodo`, `tags`, or `tags-todo`.
  Match expressions use Emacs tags/TODO syntax, including property comparisons.
- Groups use first-match-wins ordering and support `discard`, `order`, effort
  bounds, and `by = "category"|"parent"|"todo"|"priority"`. Unmatched rows
  appear under “Other items.”
- `notation = "org"` displays tags and priorities as `:work:` and `#A` instead
  of the default `#work` and `!A`.
- Unknown keys and invalid values fail at load time and list accepted values.

Check configuration and Org files with `agd setup check`.

## Daily workflow

### Capture

The CLI, TUI, and direct agent commands share one-line capture syntax:

```bash
agd capture "Renew passport due:fri #personal !A" --dry-run
agd capture "Renew passport due:fri #personal !A"
agd capture n "Wifi password is on the router"
agd capture m "Dentist on:fri 14:00"
agd capture j "Shipped the release"
```

A leading template key selects that template when more text follows. `due:DATE`
sets `DEADLINE:`, `on:DATE` sets `SCHEDULED:`, `#tag` adds a tag, and `!A` adds
a priority. Date forms include `today`, `tomorrow`, weekdays, `+3d`, `+2w`,
`oct 30`, ISO dates, optional times, repeaters, and warning periods. Put `--`
before text beginning with `-`; use `--body TEXT` for `%i` and
`--prompt KEY=VALUE` for template prompts.

Each date counts from today, as Org's date prompt does, so `on:fri due:tue`
on a Monday puts the deadline on the coming Tuesday, before Friday. agd writes
that as typed, as Org does, and says so under the chips:
`scheduled 3 days after the deadline · due:+2tue for Tue 27 Oct`, the second
part naming the token for the Tuesday after. The edit popup, the `s` and `d`
prompts, `agd capture` (on stderr) and MCP `capture` and `plan` results (in
`notes`) say the same.

In the TUI capture popup, the optional Description panel stays collapsed until
`S-Enter` from the task line reveals and focuses it. Paste a paragraph there to
preserve all of its lines and blank lines. `Tab` opens the first sub-task.

Enter continues, and a blank Enter finishes. On the task line and in one-line
fields, Enter saves, or moves to a template prompt still unanswered. On a
description, sub-task or note line, Enter adds the next line, and Enter on an
empty line drops it and saves; the footer shows it as `⏎ new line ⏎⏎ save`.
`S-Enter` always adds a line, so it also types a blank line between paragraphs.

The description is plain text written under the heading, where Org
keeps an entry's notes, so Emacs and Neovim show it as it is. `C-o` moves it,
with the sub-tasks, into `$EDITOR`.

`--dry-run` prints the exact proposed change and creates nothing. Normal
terminal output is a concise summary; piped output is the new heading's stable
ref. `agd undo` reverses the latest CLI change when the written bytes are still
identifiable.

Search every configured Org file with the same org-ql subset as the MCP server:

```bash
agd query '(and (todo) (deadline :to 7))'
agd query '(tags "work")' --limit 20 --json
agd query '(text "release")' --with-body --json
```

Queries support `and`, `or`, `not`, `todo`, `done`, `tags`, `priority`,
`category`, `text`, `deadline :to DAYS`, and `scheduled :to DAYS`.

Every agenda, query, capture, or edit result carries an item `ref`. Use the
latest ref for guarded scripted changes:

```bash
agd edit 'REF' --done
agd edit 'REF' --scheduled fri --deadline +7d --add-tag work
agd edit 'REF' --description $'First line\nSecond line' --dry-run
agd edit 'REF' --title 'Ship release' --json
```

`--dry-run` prints the exact diff without writing. An edit can also add or
remove tags, set or clear a priority, set or remove properties, and add a
timestamped note. A stale ref fails with `ref_conflict`; read the item again
instead of retrying blindly.

### Today, Week, Inbox, Agents, and Triage

Run `agd`. It opens Today or the view named by `start_view`:

- **Today** separates OVERDUE, SCHEDULE, DUE SOON, DOING, and INBOX.
- **Week** provides the Org-compatible weekly agenda.
- **Inbox** collects open entries filed by capture templates.
- **Agents** groups unreviewed MCP writes by client for inspection, acceptance,
  editing, or safe undo.
- **Triage** (`T`) reviews overdue work and inbox items one card at a time.

Common keys:

| Action | Keys |
| --- | --- |
| Next / previous row | `n` `j` / `p` `k` |
| Change tab / select tab | `Tab` `S-Tab` / number |
| Preview / edit / open in editor | `SPC` / `Enter` `e` / `C-e` |
| Advance state / done / cancel | `t` / `x` / `X` |
| Schedule / deadline | `s` / `d` |
| Add a timestamped note | `z` |
| Capture / Triage / saved views | `c` / `T` / `a` |
| Mark / mark all / unmark / bulk | `m` / `*` / `U` / `B` |
| Reload / undo / contextual help | `r` `g` / `u` / `?` |
| Quit | `q`; it asks first when marks, a filter, a Triage pass or the pane would be lost |

The complete generated reference is [KEYS.md](KEYS.md). Entry and capture
popups preview exact Org diffs before saving. In the edit popup, Description
changes the plain text under the heading. On a task, in Triage too, `z` records
a timestamped note without changing TODO state, as Org's `C-c C-z` does: in the
`LOGBOOK` with `log_into_drawer = true`, else under the heading. A note takes
several lines, by the same Enter rule. Peek (`SPC`) shows the description under the title and the
three newest notes. `C-o` opens
`$EDITOR` from a text field; `C-e` opens it from list screens. Configure an
argument array, not a shell command:

```toml
editor = ["emacsclient", "-n", "+{line}:{col}", "{file}"]
```

If unset, `agd` uses `$EDITOR +{line} {file}`, then `vi`.

### Pane and status line

```bash
tmux split-window -h -l 36 'agd --pane'

# ~/.tmux.conf
bind T display-popup -E -w 80 -h 22 'agd capture -i'
set -g status-right '#(agd agenda --status)'
set -g set-clipboard on
```

The pane shows upcoming timed work, overdue counts, completion, capture, undo,
and editor actions. Narrow terminals select it automatically; at 60 columns or
wider it can open Triage and the full view. `herdr` can run `agd --pane` in the
same way. The TUI and pane watch source files for external edits.

### VS Code

Run `agd`, `agd --pane`, or `agd capture -i` in the integrated terminal. To
open headings in the editor with `C-e`, set:

```toml
editor = ["code", "-g", "{file}:{line}:{col}"]
```

Use VS Code terminal profiles when you want dedicated Agenda and Capture tabs.
Agent setup is the same as the corresponding terminal client below.

## Connect coding agents and desktop clients

Every integration starts the local `agd mcp` stdio server. It can reach only
Org files available on the same machine under configured roots.

Preview and install every supported client whose user config already exists:

```bash
agd setup install --dry-run --detected
agd setup install --detected
```

Or select one explicitly:

| Client | Setup |
| --- | --- |
| Codex CLI, ChatGPT desktop Codex, Codex IDE | `agd setup install codex` |
| Claude Code terminal, desktop local sessions, VS Code | `claude plugin marketplace add johnnybravo-xyz/agd`, then `claude plugin install agd@johnnybravo-xyz` |
| GitHub Copilot in VS Code and Copilot CLI | `agd setup install copilot` |
| OpenCode | `agd setup install opencode` |
| Pi / oh-my-pi | `agd setup install pi` / `agd setup install omp` |
| Paseo agent | Configure the provider it launches |
| Herdr | `agd setup install herdr` for the native panel; configure its provider for MCP |
| Claude Desktop or another stdio client | command `agd`, argument `mcp` |

`--detected` recognizes Codex, OpenCode, Pi, oh-my-pi, Copilot, and Herdr
directories. It never launches clients, reads private config contents, or uses
the network. The installer writes the shared skill to
`~/.agents/skills/agenda/SKILL.md`, preserves unrelated configuration and
comments, and refuses every write if one target conflicts. Name a new client
explicitly when its config directory does not exist yet.

Codex surfaces share `~/.codex/config.toml`, or `$CODEX_HOME/config.toml`.
Claude Code's plugin provides the MCP tools, `agenda` skill, `/agd`, `/capture`,
and status band; it requires Claude Code 2.1.296 or later. Pi and oh-my-pi also
receive an `agd.ts` `/capture` extension and can reload it with `/reload`.
Copilot uses `~/.copilot/mcp-config.json`; VS Code discovers that file by
default. A custom `$XDG_CONFIG_HOME` is not inherited by Copilot unless its MCP
entry includes the variable.

Paseo and Herdr launch an installed provider CLI with that provider's config,
skills, and MCP servers. Configure Codex, the Claude Code plugin, OpenCode, Pi,
or oh-my-pi first, then start a new agent. Herdr's install target adds only its
native panel; it does not create a second MCP configuration.

For another stdio client, use its equivalent of:

```json
{
  "mcpServers": {
    "agd": {
      "command": "agd",
      "args": ["mcp"]
    }
  }
}
```

GUI apps may need the absolute path from `command -v agd`.

### Herdr agenda panel

The native Herdr mod shows today's unfinished agenda, refreshes once a minute,
and opens the full TUI or capture popup in terminal tabs. It runs the local
`agd` executable and makes no network requests.

Preview and install it without overwriting another mod named `agd`:

```bash
agd setup install --dry-run herdr
agd setup install herdr

herdr_home="${HERDR_APP_HOME:-${XDG_CONFIG_HOME:-$HOME/.config}/herdr-app}"
herdr-app check "$herdr_home/mods/agd"
```

Herdr docks it on the right when it first sees the folder. Re-running the
installer updates files owned by agd. The source and security notes are in the
[mod recipe](../agd/herdr/RECIPE.md).

Without the mod, open terminal tabs directly:

```bash
herdr-app do tab.new '{"name":"Agenda","argv":["agd","--pane"]}'
herdr-app do tab.new '{"name":"Capture","argv":["agd","capture","-i"]}'
```

The panel runs on the machine hosting the Herdr window. A provider running on a
remote machine needs `agd`, its provider integration, and the intended Org files
on that remote host; local stdio configuration is not forwarded.

### Review agent changes

The journal records each write under
`$XDG_DATA_HOME/agd/journal.jsonl`, or `~/.local/share/agd/journal.jsonl`, with
its actor and project directory. The Agents tab can inspect a diff, accept the
record without changing Org, edit its task, or safely undo it.

To route agent captures through one template and remove requested schedules and
deadlines:

```toml
agent_inbox = "t"
```

MCP items carry a `description`: the first plain text under the heading.
`edit` changes it, `capture` writes it through `body`, and `edit.note` adds a
timestamped note without changing TODO state. See the
[reference](reference.md) for import details, compatibility boundaries, write
safety, and troubleshooting.
