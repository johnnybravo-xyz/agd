# Getting started with agd

`agd` is a local-first Org agenda for terminals and coding agents. The TUI,
the pane, the CLI, the MCP server and the Claude Code and Herdr panels all read
the same Org files and write through the same source-preserving engine.

## Install and try the demo

Choose one:

```bash
cargo install agd --locked                  # Cargo; Rust 1.89+
brew install johnnybravo-xyz/agd/agd        # Homebrew
nix profile add github:johnnybravo-xyz/agd  # Nix
```

Release binaries and the Arch recipe are listed in the
[README](../README.md#install). Shell completions:
`agd completions bash|zsh|fish|elvish|powershell`.

To explore sample data without touching your files (needs `python3`; `HOME`,
config, data and Org files live in a temporary directory):

```bash
git clone https://github.com/johnnybravo-xyz/agd.git && cd agd
scripts/cargo build --release
AGD=target/release/agd docs/demo/demo.sh
```

## Start

### From Emacs or Neovim

```bash
agd setup import ~/.emacs.d --stdout    # preview the generated config
agd setup import ~/.emacs.d             # write ~/.config/agd/config.toml
agd setup check
agd
```

Pass an editor directory (`~/.doom.d`, `~/.config/emacs`, `~/.config/nvim`) or
config files. The importer runs `emacs -Q` or `nvim --clean`, so your normal
startup and plugin managers never run, and reports anything it could not
carry over. See the [import reference](reference.md#import-reference).

### From nothing

Org keeps tasks in plain text. A heading with a TODO keyword is a task, one
without is a note. A **scheduled** date is when you plan to work on it; a
**deadline** is when it must be done.

```bash
mkdir -p ~/org
agd capture "Renew passport due:fri #personal !A"
agd
```

With no config, agd reads every `.org` file under `~/org`, uses
`TODO NEXT WAIT | DONE CANCELLED`, stamps `CLOSED:` on completion and captures
into an Inbox.

## Configure

`~/.config/agd/config.toml` (or under `$XDG_CONFIG_HOME`); `--config FILE`
picks another for one run.

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

- `roots` is the boundary for every read and write.
- Ordinary settings go before the first `[[capture]]` or `[[view]]`; TOML
  gives later keys to the record above them.
- A file's own `#+TODO`, `#+CATEGORY`, `#+FILETAGS`, `#+PRIORITIES` and
  `#+STARTUP` win over the config for that file.
- In a keyword sequence, `|` separates open from done; `!` logs a timestamp,
  `@` asks for a note.
- Templates: `%?` is the captured text, `%U` and `%t` are now and today, `%i` is
  the body, `%^{name}` prompts. Defining any `[[capture]]` replaces the
  defaults.
- View blocks are `agenda`, `todo`, `alltodo`, `tags` or `tags-todo`, matched
  with Emacs tags/TODO syntax; `groups` work as in org-super-agenda.
- `notation = "org"` shows `:work:` and `#A` instead of `#work` and `!A`.
- Unknown keys and bad values fail on load and name the accepted ones.

`agd setup check` checks the config and every Org file.

## Daily use

### The TUI

`agd` opens Today, or your `start_view`:

- **Today**: OVERDUE, SCHEDULE, DUE SOON, DOING and INBOX.
- **Week**: the Org weekly agenda. **Inbox**: what capture filed.
- **Find** (`F`): any heading, dated or not.
- **Triage** (`T`): overdue then inbox, or a saved view's rows, one card at a
  time.
- **Agents** (`A`): agent changes waiting for review.

| Action | Keys |
| --- | --- |
| Move / switch tab | `n` `p` (`j` `k`) / `Tab`, a number |
| Peek / edit / open in `$EDITOR` | `SPC` / `e` / `C-e` |
| Next state / done / cancel | `t` / `x` / `X` |
| Schedule / deadline / note | `s` / `d` / `z` |
| Capture / find / saved views | `c` / `F` / `a` |
| Mark / bulk / undo / help | `m` / `B` / `u` / `?` |
| Quit | `q`, which asks again before losing marks, a filter or a Triage pass |

Every key is in [KEYS.md](KEYS.md). Popups preview the exact Org change before
saving. In multi-line fields (a description, sub-tasks, a note) Enter adds a
line and Enter on an empty line saves; `S-Enter` always adds a line.

The **description** is the plain text under a heading, where Org keeps an
entry's notes: capture and the edit popup write it, peek shows it. `z` adds a
timestamped note as Org's `C-c C-z` does, in the `LOGBOOK` with
`log_into_drawer`.

`$EDITOR` opens at the heading; set an argument array to choose another:

```toml
editor = ["emacsclient", "-n", "+{line}:{col}", "{file}"]
```

### Capture

One line, the same in the CLI, the TUI and `/capture`:

```bash
agd capture "Renew passport due:fri #personal !A"
agd capture j "Shipped the release"            # a template key first
agd capture --dry-run "Dentist on:fri 14:00"   # show the change, write nothing
```

`due:` sets the deadline, `on:` the scheduled date, `#tag` a tag, `!A` a
priority. Dates take `today`, `fri`, `+3d`, `oct 30`, ISO dates, times,
repeaters and warnings, each counted from today as Org's date prompt does. A
deadline before the scheduled date is kept, with a hint naming the date you may
have meant. Put `--` before text that starts with `-`.

### Pane and status line

```bash
tmux split-window -h -l 36 'agd --pane'

# ~/.tmux.conf
bind T display-popup -E -w 80 -h 22 'agd capture -i'
set -g status-right '#(agd agenda --status)'
```

The pane shows today at a glance and takes `x` `X` `z` `u` `c` and `C-e`;
narrow terminals open it on their own. The TUI and pane follow edits made
elsewhere.

### VS Code

Run `agd`, `agd --pane` or `agd capture -i` in the integrated terminal, and
open headings in VS Code with:

```toml
editor = ["code", "-g", "{file}:{line}:{col}"]
```

## Script it with JSON

The agenda, query, capture, edit and undo commands take `--json`. Agenda rows
carry `display.lead` and `display.title`, what the lists show; items carry a
`ref` that guards edits.

```bash
# Today, as the pane says it
agd agenda --span day --json | jq -r '.[] | "\(.display.lead)\t\(.display.title)"'

# The counts a status bar shows
agd agenda --status --json | jq '{overdue, today, inbox}'

# Open tasks per category
agd query '(todo)' --json | jq 'group_by(.category) | map({(.[0].category): length}) | add'

# Deadlines in the next week
agd query '(deadline :to 7)' --json | jq -r '.[] | "\(.planning.deadline.date) \(.title)"'
```

With `--json`, capture and edit return the item and the sentence a terminal
shows:

```bash
ref=$(agd capture --json "Call the bank on:fri" | jq -r '.item.ref')
agd edit "$ref" --add-tag finance --dry-run
agd edit "$ref" --add-tag finance --json | jq -r '.summary'

agd capture --json "Book flights on:fri due:wed" | jq -r '.summary, .notes[]'
agd undo                                         # your latest change, from anywhere
```

`agd edit` sets `--title`, `--description`, `--todo` or `--done`,
`--scheduled`, `--deadline`, `--priority`, tags, properties and `--note`. A ref
goes stale when the item changes; a stale one fails with `ref_conflict`, so
read it again instead of retrying.

`agd query` takes the org-ql subset the MCP server uses: `and`, `or`, `not`,
`todo`, `done`, `tags`, `priority`, `category`, `text`, `deadline :to N` and
`scheduled :to N`; `--with-body` adds each item's text.

`agd agenda --status --watch` prints a fresh status line whenever an Org file
changes, for anything that should redraw then.

## Connect agents

Every integration runs the local `agd mcp` server, which reaches only the Org
files under your roots.

```bash
agd setup install --dry-run --detected   # preview detected integrations
agd setup install --detected
```

| Client | Setup |
| --- | --- |
| Claude Code (terminal, desktop, VS Code) | `claude plugin marketplace add johnnybravo-xyz/agd`, then `claude plugin install agd@johnnybravo-xyz` |
| Codex CLI, ChatGPT desktop, Codex IDE | `agd setup install codex` |
| GitHub Copilot in VS Code, Copilot CLI | `agd setup install copilot` |
| OpenCode / Pi / oh-my-pi | `agd setup install opencode` / `pi` / `omp` |
| Herdr | `agd setup install herdr` for the panel; set up the agent it launches |
| Paseo | set up the agent it launches |
| Any other stdio client | command `agd`, argument `mcp` |

The installer writes the shared `agenda` skill to `~/.agents/skills`, keeps
everything else in each config, and refuses the whole install on a conflict.
GUI apps may need the absolute path from `command -v agd`.

- **Claude Code** gets the MCP tools, the skill, `/agd` (your agenda in a
  pane), `/capture` and a status band that redraws when an Org file changes.
- **Pi and oh-my-pi** get `/capture` too.
- **Herdr** gets a panel that shows today as the pane does, with Done and Undo,
  and follows changes made anywhere. See its
  [recipe](../agd/herdr/RECIPE.md).
- **Copilot** reads `~/.copilot/mcp-config.json`, which VS Code also finds.

### Review agent changes

Every write is journaled in `~/.local/share/agd/journal.jsonl` (or under
`$XDG_DATA_HOME`) with who made it. The Agents tab shows each agent change's
diff and lets you accept, edit or undo it. To file every agent capture through
one template and drop the dates agents ask for:

```toml
agent_inbox = "t"
```

See the [reference](reference.md) for import details, compatibility, write
safety and troubleshooting.
