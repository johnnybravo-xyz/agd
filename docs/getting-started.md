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

Review the report before using the generated configuration. See
[Import reference](#import-reference) for discovery, supported settings, and
re-import behavior.

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
template = "* %?\n%U\n%i"

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

`--dry-run` prints the exact proposed change and creates nothing. Normal
terminal output is a concise summary; piped output is the new heading's stable
ref. `agd undo` reverses the latest CLI change when the written bytes are still
identifiable.

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
| Capture / Triage / saved views | `c` / `T` / `a` |
| Mark / mark all / unmark / bulk | `m` / `*` / `U` / `B` |
| Reload / undo / contextual help | `r` `g` / `u` / `?` |

The complete generated reference is [KEYS.md](KEYS.md). Entry and capture
popups preview exact Org diffs before saving. `C-o` opens `$EDITOR` from a text
field; `C-e` opens it from list screens. Configure an argument array, not a
shell command:

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

## Connect coding agents and desktop clients

All integrations run the local `agd mcp` stdio server. The server can reach only
the Org files available on the same machine under configured roots.

### Detect supported local clients

Preview, then configure supported clients whose user config directories already
exist:

```bash
agd setup install --dry-run --detected
agd setup install --detected
```

Detection is filesystem-only. It recognizes Codex, OpenCode, Pi, and oh-my-pi
config directories; it does not launch clients, read private config contents,
or use the network. Name a newly installed client explicitly if its directory
does not exist yet. Positional names and `--detected` may be combined.

The installer writes the shared skill to
`~/.agents/skills/agenda/SKILL.md`, preserves unrelated configuration and
comments, and calculates every candidate change before writing. A conflicting
`agd` server, unsupported JSONC edit, or another skill named `agenda` refuses
the whole operation.

### Codex CLI, ChatGPT desktop Codex, and the Codex IDE extension

```bash
agd setup install codex
codex mcp get agd
```

These Codex surfaces share `~/.codex/config.toml`, or
`$CODEX_HOME/config.toml`. Start a new session after installation. In an
interactive Codex session, `/mcp` also shows connected servers.

### Claude Code terminal, desktop local sessions, and VS Code

Install `agd`, then install the repository's Claude Code plugin:

```bash
claude plugin marketplace add johnnybravo-xyz/agd
claude plugin install agd@johnnybravo-xyz
claude plugin list
claude mcp list
```

The user-installed plugin is shared by Claude Code terminal, Claude Code
desktop local sessions, and its VS Code integration. It provides the MCP tools,
the `agenda` skill, `/agd`, `/capture`, and a status band. Claude Code 2.1.296
or later is required. Claude Desktop chat is a different client; configure it
as a generic MCP client below.

### Paseo

Paseo launches an installed provider CLI and preserves that provider's config,
skills, and MCP servers. Configure the provider Paseo will run—`codex`, the
Claude Code plugin, `opencode`, `pi`, or `omp`—then start a new Paseo agent.
There is no `agd setup install paseo` because Paseo does not own a separate MCP
configuration.

### OpenCode, Pi, and oh-my-pi

```bash
agd setup install opencode
agd setup install pi
agd setup install omp
```

| Harness | User configuration |
| --- | --- |
| OpenCode | `~/.config/opencode/opencode.json`, or under `$XDG_CONFIG_HOME` |
| Pi | `~/.pi/agent/mcp.json`, or under `$PI_CODING_AGENT_DIR` |
| oh-my-pi | `~/.omp/agent/mcp.json` |

Pi and oh-my-pi also receive an `agd.ts` extension that implements `/capture`
without a model turn. Start a new session, or run `/reload` in Pi.

Running `agd setup install` without a harness installs only the shared skill.
Gemini CLI and another Agent Skills-compatible harness may read that skill, but
still need an MCP entry that starts `agd mcp`.

### Claude Desktop and other stdio MCP clients

Add a local stdio server using the configuration shape your client expects:

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

GUI applications often do not inherit the shell's `PATH`. If the client reports
that `agd` is missing, replace `"agd"` with the absolute path printed by
`command -v agd`, then restart the client. The exact settings file and reload
operation belong to that client.

### Review agent changes

The journal records each write under
`$XDG_DATA_HOME/agd/journal.jsonl`, or `~/.local/share/agd/journal.jsonl`, with
its actor and project directory. The Agents tab can inspect a diff, accept the
record without changing Org, edit its task, or safely undo it.

To let agents capture work without scheduling your day:

```toml
agent_inbox = "t"
```

Agent captures then use template `t` with requested schedules and deadlines
removed; the tool result reports that behavior.

## Import reference

Import is an explicit conversion, not synchronization. After the first run,
`config.toml` belongs to you; editor settings are read again only when you rerun
`agd setup import`.

### Discovery and report

A supplied file is read directly. For a directory, discovery considers editor
entry points first:

- Emacs: `early-init.el`, `init.el`, `config.el`, `.emacs`, `.spacemacs`;
- Neovim: `init.lua`;
- literate configuration: `config.org`.

It then considers `.el` files containing `org-`, `.lua` files containing
`orgmode`, and likely literate Org files containing Emacs Lisp blocks.
Discovery respects `.gitignore`, skips common package/cache directories, and
stops at four directory levels, 300 files, 1 MiB per file, or 120 seconds for an
editor reader. The report names any limit reached.

Every relevant setting receives a mark and source location:

| Mark | Meaning |
| --- | --- |
| `✓` | Imported |
| `~` | Imported with a stated difference, or intentionally unused |
| `✗` | Omitted with the reason |

Nothing is silently guessed or dropped.

### Emacs setting map

The reader recognizes `setq`, `setq-default`, `setq!`, `setopt`, Customize
forms, `add-to-list`, `push`, `defvar`, and `defconst` at top level and in common
`progn`, Doom, use-package, after-load, and Spacemacs configuration contexts.
Emacs parses files without loading them. Only imported values and referenced
top-level variables are evaluated; later assignments win.

| Emacs settings | `agd` result |
| --- | --- |
| `org-directory`, `org-agenda-files` | roots and `agenda_files` |
| `org-todo-keywords` | `todo_keywords` sequences |
| priority, tag-column, inheritance settings | priority range/default, `tags_column`, exclusions |
| deadline, span, start, skipping, sorting settings | corresponding agenda settings |
| `org-log-*`, `org-log-into-drawer` | corresponding logging settings |
| `org-capture-templates` | `[[capture]]` records |
| `org-agenda-custom-commands` | `[[view]]` records |

### Neovim setting map

The reader finds `require("orgmode").setup()` in direct configuration and
lazy.nvim `opts` or `config`. Files mentioning orgmode run under `nvim --clean`;
stand-ins prevent unrelated missing plugins from failing the read.

| nvim-orgmode options | `agd` result |
| --- | --- |
| `org_agenda_files`, `org_default_notes_file` | `agenda_files` and default capture target |
| TODO, priority, tags, deadline, span, start, skipping, sorting options | corresponding `agd` settings |
| `org_log_done`, `org_log_repeat`, `org_log_into_drawer` | corresponding logging settings |
| `org_capture_templates` | `[[capture]]`, sorted by key |
| `org_agenda_custom_commands` | `[[view]]` records |

Options not supplied use nvim-orgmode defaults. Neovim has no stable exported
agenda rows, so `--verify` is Emacs-only.

### Templates, views, and groups

Emacs entry capture targets may use `file`, `file+headline`, `file+olp`,
`file+datetree`, `file+olp+datetree`, or `id`; literal bodies and template files
are supported. Neovim datetree and function targets are not. Non-entry
templates, computed targets or bodies, functions, lambdas, and `%(sexp)` are
reported and omitted.

Custom views import supported `agenda`, `todo`, `alltodo`, `tags`, and
`tags-todo` blocks, literal spans and headers, and property comparisons. A view
using a lambda, `search`, `stuck`, or a regexp term is omitted with a reason.

Block-local literal org-super-agenda groups import supported TODO, tag,
category, priority, planning, property, Boolean, effort, discard, order, and
automatic grouping selectors. Tags remain case-sensitive; property selectors
read only a heading's own drawer; matching the default priority also includes a
heading without a priority cookie. Unsupported predicates, time grids, logs,
habits, global groups, faces, and transformers are reported; the affected block
remains ungrouped where safe.

Executable Lisp is never imported as behavior. Computed package values, hidden
hook/function assignments, numeric priorities, Emacs agenda-file indirection,
plugin-derived Lua values, and missing Neovim `setup()` calls are reported
rather than inferred.

### Roots, verification, and re-importing

On first import, roots come from `org-directory`; otherwise they are derived
from agenda-entry directories. `-r DIR` overrides that choice. Files remain
files, directories become non-recursive `dir/*.org` entries, and globs retain
their pattern. In an existing config, roots remain unchanged and agenda entries
outside them are reported.

```bash
agd setup import ~/.emacs.d --verify
agd setup import ~/.emacs.d --dry-run
agd setup import ~/.emacs.d --force
```

`--verify` compares each imported Emacs view with `org-batch-agenda-csv` before
grouping. If a config already exists, import shows a diff and stops; `--force`
applies it. Re-import preserves `agd`-owned settings such as roots, IDs, theme,
editor, start view, agent inbox, Triage settings, and agent profiles. Existing
capture templates or views remain when the editor defines none. Rendering does
not preserve handwritten TOML comments, so review with `--dry-run` or
`--stdout`.

## Compatibility

Compatibility evidence uses pinned Emacs 30.2 with Org 9.7 and pinned Neovim
0.12.6 for import fixtures.

| Classification | Supported behavior |
| --- | --- |
| **Exact** | Agenda text/CSV, dates and repeaters, inherited tags, warnings, spans, skipping, sorting, tags/TODO/property matches, in-file workflows, state/logging/planning edits, checklists, capture placement, refile, and byte-exact round trips match pinned Emacs fixtures. |
| **Native** | TUI workflows, saved-view grouping, letter `#+PRIORITIES`, MCP org-ql subset, journal/undo, agent review, and import reporting are implemented and tested by `agd` without claiming identical Emacs presentation. |
| **Unsupported** | Regex match terms, time grids, clocking/tables, habit graphs, diary sexps, executable Lisp, skip functions, calendar grids, and numeric priorities are not evaluated; source text remains untouched and unsupported configuration is reported. |

The MCP `query` subset supports Boolean forms; TODO/done, tags, priority,
category, and text predicates; and deadline/scheduled `:to` ranges. Unknown
forms return `invalid_query`.

Agenda and edit fixtures compare recorded behavior with pinned Emacs; write
fixtures require identical resulting Org bytes. Import fixtures compare clean
editor readers with recorded settings. CI reruns the pinned oracles, and weekly
checks round-trip pinned Org and Worg corpora. This establishes the documented
subset, not compatibility with every historical Org version.

User-visible limits:

- only letter priorities are supported;
- agenda output does not draw time grids, calendar grids, clock tables, or habit
  consistency graphs;
- nested checklist cookies in body/item text are left to the editor;
- imported dynamic or executable forms are reported, never run as behavior;
- reads return bounded result sets; narrow the query or increase an exposed
  limit when a response reports omitted rows.

## Safety, transactions, and privacy

Org files are user-owned source documents. A successful write changes only the
intended byte ranges; uncertainty refuses the operation before replacement.

### Roots and identity

- Configured `roots` authorize every read and write. `..` paths are rejected.
- A symlink may be read only when it resolves inside a root; writes through any
  symlink are refused.
- Files must be UTF-8. Invalid files are reported and never rewritten.
- A unique `:ID:` is stable identity. A heading without one uses a ref containing
  its path, line hint, and heading digest; the line is only a lookup hint.
- A changed or missing heading is `ref_conflict`. Any duplicated `:ID:` under
  roots is `id_conflict`, including duplicates outside agenda files.

### Write protocol

Every CLI, TUI, pane, capture, and MCP write follows one sequence:

1. Load a fresh workspace, resolve identity, validate intent, and build sorted,
   non-overlapping byte patches.
2. Reject unauthorized paths, symlink writes, and Emacs `.#file` locks that
   indicate unsaved changes.
3. For `--dry-run`, return the exact diff without creating files, directories,
   locks, or journal records.
4. Acquire per-file advisory locks in deterministic order and settle pending
   journal state.
5. Re-read under lock and resolve refs again.
6. Append and fsync a pending journal record with hashes and inverse patches.
7. Write a same-directory temporary file, preserve mode, fsync, rename
   atomically, and fsync the directory.
8. Append and fsync the committed record.

Cross-file refile and bulk edit are one transaction. If any replacement fails,
already replaced files are restored. An interrupted write is reconciled from
old/new hashes; unrelated current bytes produce `journal_conflict` until the
file is reviewed and `agd setup check --resolve` accepts its current contents.

Undo applies inverse patches only where written bytes still match or relocate
uniquely with context. Modified or ambiguous bytes produce `undo_conflict` and
write nothing. A transaction undoes all files or none. The TUI, CLI, and Agents
tab select only changes from their own histories.

### Refusal codes

| Code | Condition |
| --- | --- |
| `id_conflict` | More than one heading has the target `:ID:` |
| `invalid_id_change` | A request changes or removes an existing `:ID:` |
| `ref_conflict` | The ref's heading changed or disappeared |
| `invalid_path` | A path escapes roots, contains `..`, or is a prohibited symlink |
| `editor_conflict` | Emacs has unsaved changes for the file |
| `invalid_utf8` | A source file is not UTF-8 |
| `invalid_patch` | Patches overlap or exceed the file |
| `invalid_refile` | A heading would move into its own subtree |
| `journal_conflict` | Interrupted-write bytes match neither journal hash |
| `undo_conflict` | Undo bytes changed or became ambiguous |

Each refusal leaves Org files unchanged.

### Privacy and operational limits

`agd` makes no network requests and starts no background service. MCP uses only
stdin/stdout and logs to stderr. The data directory is mode `0700`; the journal
is mode `0600`. Explicit editor/import subprocesses run as argument arrays,
never through a shell.

Advisory locks cannot make an unrelated writer that ignores both agd and Emacs
locks transactional. Atomic replacement assumes ordinary same-directory rename
and fsync semantics; weaker network filesystems are outside the tested
boundary. The journal provides undo and interrupted-write recovery, not backup,
encryption, synchronization, or version history.

## Troubleshooting

### A client says `agd` is not found

Run `command -v agd` in a terminal. If it succeeds but a GUI client cannot start
the MCP server, use that absolute path in the client's command field and
restart it. This is an executable/PATH failure, not an MCP protocol failure.

### The MCP server exists but is disconnected

Run `agd mcp` in a terminal: it should remain waiting for stdio input rather
than exit with a configuration error. Then inspect the client entry for command
`agd`, argument `mcp`, and valid local roots. For installed integrations, use
`codex mcp get agd`, `claude mcp list`, or the provider's MCP listing.

### Local and cloud sessions differ

Local stdio works only on the machine—or provider host—that can execute `agd`
and reach the configured Org files. Local configuration does not make ChatGPT
web, Claude cloud sessions, or a remote provider able to access those files.
Install and configure `agd` on the actual provider host, or use a local session.

### Top-level TOML settings appear unknown

An error such as ``unknown field `tui` `` or a direction to move `start_view`
usually means the key appears after `[[capture]]` or `[[view]]`. Move all
ordinary settings above the first array record.

### No configuration or Org root exists

For `no config at ... and no ~/org to read`, create `~/org`, import editor
settings, or pass `agd -r DIR`.

### A capture outline path is missing

`capture heading not found on level 1: Projects` means an `olpath` target does
not exist. Create the path first or use `heading`, which `agd` can create.

### A date or priority is rejected

`unsupported date '...'` lists accepted forms; common values are `fri`, `+2d`,
`oct 30`, `2026-10-30 14:00`, and `fri +1w -3d`. Priorities must be letters;
configure `highest_priority = "A"` and `lowest_priority = "C"` as needed.

### Colors are absent or incorrect

A non-empty `NO_COLOR` disables color while retaining bold, dim, reverse, and
strikethrough meaning. `theme = "auto"` queries the terminal background;
`"dark"` and `"light"` select explicitly. Without `COLORTERM=truecolor`, `agd`
uses its 16-color palette. Leave `nerd_font = false` unless the terminal font
contains the required glyphs.
