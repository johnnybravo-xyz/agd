# agd reference

Detailed import behavior, compatibility boundaries, write safety, and troubleshooting. Start with [Getting started](getting-started.md) for installation, daily use, and client setup.

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

The CLI and MCP `query` subset supports Boolean forms; TODO/done, tags,
priority, category, and text predicates; and deadline/scheduled `:to` ranges.
Unknown forms return `invalid_query`.

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
