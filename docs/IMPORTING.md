# Importing your Emacs or Neovim setup

Import is a one-time setup step. Point agd at your editor config, and it
writes `~/.config/agd/config.toml` with your agenda files, TODO keywords,
logging, priorities, capture templates and custom agenda commands. From then
on `config.toml` is yours to edit; agd never reads your editor config again
unless you rerun the import.

```bash
agd setup import ~/.doom.d               # or ~/.config/emacs, ~/.emacs.d, ~/.config/nvim
agd setup import ~/.emacs.d/init.el      # or name the files
agd setup import ~/.config/nvim --dry-run   # show the config it would write
agd setup check
agd
```

Your Org files' own syntax (`#+TODO`, `#+SEQ_TODO`, `#+STARTUP`, `#+CATEGORY`,
`#+FILETAGS`) always applies, imported or not.

Without an import, agd reads every `.org` file under `~/org` with Org's
defaults.

## What agd reads

A file is read as given. For a directory, agd reads the editor's entry points
first (`early-init.el`, `init.el`, `config.el`, `.emacs`, `.spacemacs`,
`config.org`, `init.lua`), then other files that mention org settings:
`.el` files containing `org-`, `.lua` files containing `orgmode`, and literate
`config.org`/`init.org` files with `emacs-lisp` blocks. It respects
`.gitignore`, skips package and cache directories (`.git`, `.local`, `elpa`,
`straight`, `eln-cache`, `var`, `pack`, `plugged`, `lazy`, `node_modules`),
and stops at 4 levels, 300 files or 1 MB per file, saying so when it does.

Your editor's normal startup never runs. agd starts a clean `emacs -Q` or
`nvim --clean` with its own reader, so an import does not depend on a running
daemon, your plugin manager, or your plugins being installed.

The report lists the files read, then every setting with a mark:

```
read Emacs 1 file(s):
  ~/.doom.d/config.el
imported  roots ~/org, 2 agenda entries, 3 capture templates, 3 views
  ✓ org-agenda-files  config.el:379
  ✓ org-todo-keywords  config.el:457
  ~ org-agenda-start-with-log-mode  config.el:393: not used by agd
  ✗ capture j  a computed capture target is unsupported
  ✗ view e  regexp matches ({...}) are not supported in '+{^@}'
```

`✓` imported, with where it came from; `~` imported with a difference, or
set in your config but not used by agd; `✗` not imported, with the reason.
Nothing is dropped silently.

## Emacs

### What imports

These Org settings, wherever they are assigned:

| Org | agd |
| --- | --- |
| `org-directory` | the root, on a first import |
| `org-agenda-files` | `agenda_files` (files, directories, globs) |
| `org-todo-keywords` | `todo_keywords` |
| `org-tag-alist`, `org-tag-persistent-alist`, `org-tags-column`, `org-tags-exclude-from-inheritance` | `tag_alist`, `persistent_tags`, `tags_column`, `tag_inheritance_exclusions` |
| `org-priority-highest`/`-lowest`/`-default` (or the older `org-highest-priority` names) | `highest_priority`, `lowest_priority`, `default_priority` |
| `org-deadline-warning-days`, `org-agenda-span`, `org-agenda-start-day`, `org-agenda-start-on-weekday` | `deadline_warning_days`, `agenda_span`, `agenda_start_day`, `week_start` |
| `org-agenda-skip-{scheduled,deadline,timestamp}-if-done` | `skip_*_if_done` |
| `org-agenda-sorting-strategy` (its agenda part) | `sorting_strategy` |
| `org-log-done`, `-repeat`, `-reschedule`, `-redeadline`, `-into-drawer` | `log_*` |
| `org-capture-templates` | `[[capture]]` |
| `org-agenda-custom-commands` | `[[view]]` |

Assignments are found in `setq`, `setq-default`, `setq!`, `setopt`,
`customize-set-variable`, `custom-set-variables`, `add-to-list`, `push` and
`defvar`/`defconst`, at the top level or inside `progn`, `when`, `unless`,
`after!`, `with-eval-after-load`, `eval-after-load`, `use-package` and
`use-package!` (`:custom`, `:init`, `:config`), and Spacemacs's
`dotspacemacs/user-config`.

The files are read with Emacs's own reader and never loaded. Only the values
of those settings are evaluated, plus the top-level variables they use, such
as `org-directory` in `(concat org-directory "inbox.org")` or a variable of
your own. Order follows Emacs: top-level forms first, then `after!` and
`with-eval-after-load` bodies, each in file order, so the last assignment
wins.

### What does not import, and what you see

- **A value that calls your own function or a package's**, such as
  `(my/org-file "inbox")` or a Doom helper: `✗ <setting> config.el:N: <the
  error>; set it in config.toml`. The setting keeps Org's default.
- **Settings your config never assigns.** Defaults set by Doom modules or
  packages are not visible to the reader: `~ org-todo-keywords not set in
  these files; Org's default applies`.
- **Assignments inside `let`, `if`, hooks, lambdas or your own `defun`s** are
  not found. `when` and `unless` bodies are read without testing their
  condition.
- **`org-agenda-files` naming a file that lists your agenda files**:
  `✗ org-agenda-files`; list them in `agenda_files`.
- **Capture templates** other than `entry`, with a computed target or body
  (`function`, a lambda), or using `%(sexp)`: `✗ capture KEY` with the reason;
  the rest import. Supported targets are `file`, `file+headline`,
  `file+olp`, `file+datetree` and `id`.
- **Custom agenda commands** with lambdas, `search` or `stuck` blocks, or
  regexp matches (`{...}`): `✗ view KEY`, left out whole. Property matches
  such as `TODO="DONE"+CLOSED>="<-3w>"` or `Effort<15` import as written.
  Options other than `org-agenda-span`, `org-agenda-overriding-header` and
  `org-super-agenda-groups` are listed `~ ... not imported`.
- **org-super-agenda groups** set on a block (`(org-super-agenda-groups
  my-groups)` or a quoted list) import as the block's `groups`. Each
  selector becomes a match: `:todo "NEXT"` is `TODO="NEXT"`, `:deadline
  past` is `DEADLINE<"<today>"`, `:not` and `:and` combine them, and the
  `:auto-category`, `:auto-parent`, `:auto-todo` and `:auto-priority` groups
  become `by`. `:effort<` and `:effort>` become `effort_max` and
  `effort_min` when they are a group's only selector or sit in its `:and`;
  ORed with other selectors they cannot be written and leave the block
  ungrouped. Three differences are reported with `~ view KEY groups: ...`:
  tags compare case-sensitively, `:property` reads the heading's own
  drawer, and `:priority` with the default letter also takes headings
  without a cookie. A group using `:pred`, `:time-grid`, `:log`,
  `:auto-property` or another selector agd cannot run leaves the block ungrouped:
  `~ view KEY groups left out, rows ungrouped: :pred is not supported`.
  `:face` and `:transformer` are dropped with a note.
- **org-ql, prefix formats, skip functions**, a global
  `org-super-agenda-groups`, and other
  `org-agenda-*` options agd does not use: `~ <setting> config.el:N: not used
  by agd`.

`--verify` runs each imported view in the same clean Emacs with
`org-batch-agenda-csv` and in agd, and reports whether the rows match.

## Neovim (nvim-orgmode)

### What imports

The options passed to `require("orgmode").setup()`, whether it is called
directly or by a lazy.nvim spec (its `opts` and `config`, in a spec file or
inline in `require("lazy").setup`). Only the files that mention `orgmode` run,
in `nvim --clean`, with a stand-in for the plugin that records the options;
modules from plugins that are not installed are stand-ins too, so a
`require("org-bullets").setup()` next to it does no harm.

| nvim-orgmode | agd |
| --- | --- |
| `org_agenda_files` | `agenda_files` (globs kept) |
| `org_default_notes_file` | the target of templates without one |
| `org_todo_keywords` | `todo_keywords` |
| `org_priority_highest`/`_lowest`/`_default` | `highest_priority`, `lowest_priority`, `default_priority` |
| `org_tags_column`, `org_tags_exclude_from_inheritance` | `tags_column`, `tag_inheritance_exclusions` |
| `org_deadline_warning_days`, `org_agenda_span`, `org_agenda_start_day`, `org_agenda_start_on_weekday` | `deadline_warning_days`, `agenda_span`, `agenda_start_day`, `week_start` |
| `org_agenda_skip_scheduled_if_done`, `org_agenda_skip_deadline_if_done` | `skip_*_if_done` |
| `org_agenda_sorting_strategy` (its agenda part) | `sorting_strategy` |
| `org_log_done`, `org_log_repeat`, `org_log_into_drawer` | `log_done`, `log_repeat`, `log_into_drawer` |
| `org_capture_templates` | `[[capture]]` |
| `org_agenda_custom_commands` | `[[view]]` |

Options you do not set take nvim-orgmode's defaults.

### What does not import, and what you see

- **No `setup()` call** in the files read, or `setup()` only inside a
  packer or vim-plug `config` function: the import stops with `no
  require('orgmode').setup() call found`. Name the file that calls it.
- **Options built from another plugin's API** (a value read from a stand-in
  module) or a file that errors before `setup()`: `✗ <file> <the error>`.
- **Capture templates** with a function target or body, `%(sexp)`, or a
  datetree: `✗ capture KEY` with the reason.
- **Custom commands** other than `agenda`, `tags` and `tags_todo`, or with
  regexp matches (`{...}`): `✗ view KEY`. Property matches import as
  written.
- **nvim-orgmode has no tag list, timestamp skipping or reschedule
  logging**: those keys keep agd's defaults.

`--verify` applies to Emacs only; nvim-orgmode has no stable way to export
agenda rows.

## Roots and agenda files

On a first import (no `config.toml` yet), `roots` become `org-directory`, or
else the directories your agenda entries live in. Agenda entries are written
relative to the root that holds them: a file stays a file, a directory
becomes `dir/*.org` (Emacs does not recurse into it), and a glob keeps its
wildcards. When `config.toml` already exists, its `roots` stay, and entries
outside them are left out with `✗ <entry> agenda entry left out: outside the
roots`.

When agenda files live outside `org-directory`, add their directories to
`roots`, which is a list, and import again; the rerun keeps your `roots` and
imports the entries inside them:

```toml
roots = ["~/org", "~/thesis"]
```

```bash
agd setup import ~/.emacs.d --force
```

## Running it again

When `config.toml` exists, the import shows the diff and stops; rerun with
`--force` to replace the imported keys. Keys the import does not touch
(`editor`, `theme`, agents, triage, and `roots`) keep their values. The file
is rewritten, so comments you added are not kept; `--dry-run` and `--stdout`
show the result first.
