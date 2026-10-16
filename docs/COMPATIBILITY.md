# Org and editor compatibility

Every compatibility claim here has a fixture behind it, checked against a
pinned Emacs 30.2 and the Org it ships (9.7).

- **Agenda.** Each case under `conformance/agenda/` holds Org files, a
  fixed today and a date range. Emacs's `org-batch-agenda` text and
  `org-batch-agenda-csv` rows for it are recorded, and agd's text agenda
  and `--csv` must equal them byte for byte. The only adjustments: the
  fixtures are read with CRLF turned into LF, so a Windows checkout
  compares the same; and for a tags match, the two lines Emacs prints
  above the rows (the match header and its `C-u r` hint) are left out,
  since agd prints the rows alone.
- **Writes.** Each case under `conformance/edit/` applies one change to an
  Org file with a fixed clock, in Emacs and in agd, and the files must be
  identical.
- **Round trips.** Every file under `conformance/roundtrip/` must parse and
  print back byte for byte.
- **Import.** Each case under `conformance/import/` records what agd's
  editor reader reads from a sample config in pinned Emacs or Neovim
  0.12.6, and the import of that result is checked.

The cases run in `cargo test`. The Conformance workflow in CI also runs
the pinned Emacs and Neovim on every case again and fails when their
output differs from the recorded one. It runs on changes to `agd-core`,
the fixtures, the editor readers or the oracle scripts, and once a week;
the weekly run also round-trips every Org file in pinned checkouts of the
Org and Worg repositories. It tests one Emacs version, the pinned one.

## Agenda

`Exact` means agd's output equals pinned Emacs's for the fixtures named.
`Native` means agd supports it, without an Emacs fixture. `Unsupported`
means agd leaves it alone and never evaluates it.

| Org behavior | Status | Evidence or boundary |
| --- | --- | --- |
| `agd agenda` text and `--csv` (Emacs's eleven columns, with `;` for commas inside a field) | Exact | Every case compares both; on a terminal the text adds color, piped it is Emacs's bytes |
| Scheduled, deadline and active timestamp rows | Exact | `plain` |
| Past scheduled carry-over and missed deadlines | Exact | `overdue-repeats`, `repeaters`, `export-columns` |
| Overdue repeating deadlines; repeats only on the base date and after today | Exact | `overdue-repeats` |
| Habit repeaters, Unicode tags and weekdays, ignored block and link timestamps | Exact | `syntax-edges` |
| Planning only on the line after the heading | Exact | `planning-lines` |
| Deadline warning window, per-timestamp warnings and their units (`-3d`, `-1w`, `-1m`) | Exact | `warnings`, `warning-units` |
| `+`, `++` and `.+` repeaters by day, week, month and year | Exact | `repeaters`, `generated` |
| Times with one-digit hours and time ranges | Exact | `short-hours`, `export-columns` |
| Active timestamp ranges and range progress | Exact | `ranges` |
| Inherited file and ancestor tags | Exact | `inheritance` |
| Skipping done items' scheduled, deadline and timestamp rows | Exact | `done-skipping` |
| Day, week, month and N-day spans; log mode (CLOSED entries) | Exact | `month-span`, `export-columns` |
| Row order: Org's default sorting strategy and a configured one | Exact | Every case compares Emacs's order; `sorting` sets its own strategy |
| A seeded mix of dates, states, repeaters and ranges | Exact | `generated` (from `scripts/generate-agenda-cases.py`) |
| Tags/TODO matches with property terms, string, number and time comparisons, and `-` negation | Exact | `match-*`: `PRIORITY="A"+Emacs/!SOMEDAY`, `TODO="DONE"+CLOSED>="<-3w>"`, `DEADLINE>="<today>"+DEADLINE<="<+1w>"`, `TODO="NEXT"+Effort<15+Effort>0`, `LEVEL=2+CATEGORY="home"\|LEVEL=1+chores`, `-TODO="WAIT"+STATUS="review"`; regexp terms (`{...}`) are refused (`invalid_match`) |
| In-file `#+TODO`, `#+SEQ_TODO` and `#+TYP_TODO` workflows | Exact | Round-trip and edit cases |
| Custom agenda commands as saved views (agenda, todo, alltodo, tags, tags-todo blocks) | Exact rows | `agd setup import <emacs config> --verify` runs each view in Emacs and agd and compares the rows |
| Letter priorities and per-file `#+PRIORITIES` | Native | Numeric priorities are not supported; `agd setup check` reports a `#+PRIORITIES` line agd cannot use, and the configured range applies |
| org-super-agenda groups in view blocks | Native | The first matching group takes a row; `discard`; `order` (equal orders by name); `effort_max`/`effort_min` (inclusive, the heading's own `:Effort:` read as `org-duration-to-minutes` does); `by` category, parent, todo or priority; "Other items" last; empty groups hidden. No Emacs fixture, since the pinned Emacs has no org-super-agenda |
| org-ql queries (MCP `query`) | Native subset | `and`, `or`, `not`, `(todo)` for any not-done keyword, `(todo "NEXT" "WAIT")`, `done`, `tags`, `priority`, `category`, `text`, `(deadline :to N)` and `(scheduled :to N)`, where `:to` includes past dates as in org-ql; anything else is `invalid_query` |
| Effort totals | Native | Today sums `:Effort:`; no column view |
| Time grid | Not drawn | The Emacs fixtures run with `org-agenda-use-time-grid` off |
| Clock tables and clocking | Unsupported | Kept as Org text; never evaluated |
| Habit graphs and consistency | Unsupported | Kept as Org text |
| Diary sexps and `%%(...)` | Unsupported | Never evaluated |
| Skip functions and executable Lisp in agenda commands | Unsupported | Never evaluated; import reports them |
| Calendar grid | Unsupported | Day-grouped agenda only |

Faces and custom display functions are presentation in Emacs's
interactive buffer and are not compared; agd prints Org's default prefix
format.

## Writes

`conformance/edit/` holds 23 cases: state changes with and without
`!`/`@` markers, CLOSED, repeats (inline, in a drawer, habits, unlogged),
schedules, deadlines and their logs, priorities, `#+STARTUP` options, and
tags kept at `org-tags-column`.

| Change | Contract |
| --- | --- |
| TODO state | The file's `#+TODO`/`#+SEQ_TODO`/`#+TYP_TODO` lines or the configured `todo_keywords`; `!`/`@` markers, `org-log-done`, `org-log-repeat`, `org-log-into-drawer` and `#+STARTUP` options write the same CLOSED, LAST_REPEAT and LOGBOOK lines as Emacs; a repeat returns to the first keyword of its sequence |
| Schedule and deadline | Planning keywords go where `org-add-planning-info` puts them; the old repeater and warning stay, as `org-schedule` keeps them; `org-log-reschedule`/`org-log-redeadline` add Emacs's "Rescheduled from" and "New deadline from" notes; setting a date drops CLOSED |
| Title, tags, priority, properties | Only the smallest span changes; untouched property lines stay byte for byte |
| Tags | Realigned to `org-tags-column` whenever a heading line is rewritten, as Emacs does |
| Capture | Templates render the exact Org text before it is written; entries are placed as org-capture places them for `file`, `file+headline`, `file+olp`, `id` and datetree targets |
| Refile | Moves the subtree's own bytes; both files are written as one transaction |
| Undo | Puts the old bytes back only where the file still holds what agd wrote; anything else is a conflict (see [SAFETY.md](SAFETY.md)) |

Writes through symlinks are refused.

## Import

`agd setup import` reads your editor config files; it never runs your
editor's startup. [IMPORTING.md](IMPORTING.md) lists every setting and every
case that does not import.

| Source | Imported | Reported, not imported |
| --- | --- | --- |
| Emacs config files (`.el`, literate `config.org`) | Org settings assigned at the top level or in `after!`, `with-eval-after-load`, `use-package` and Spacemacs's user-config; their values evaluated in `emacs -Q` | Values that call your own or a package's functions; defaults set by Doom modules or packages; settings inside `let`, hooks or your own functions |
| nvim-orgmode (`setup()` call or lazy.nvim spec) | Every option agd uses, with nvim-orgmode's defaults for the rest; agenda globs kept | No `setup()` in the files read; options built from other plugins' APIs |
| Capture templates | `entry` templates with `file`, `file+headline`, `file+olp`, datetree or `id` targets; a `(file "...")` body as `template_file` | Other types, computed targets or bodies, `%(sexp)` |
| Custom agenda commands | agenda, todo, alltodo, tags and tags-todo blocks with literal span and header, including property matches | Lambdas, search and stuck views, regexp matches (the view is left out); other options |
| org-super-agenda groups (a block's `org-super-agenda-groups`, literal or a variable) | `:name`, `:order`, `:discard`, `:todo`, `:tag`, `:category`, `:priority`, `:deadline`/`:scheduled` `t`/`nil`/`past`/`today`/`future`, `:property`, `:anything`, `:and`, `:not`, `:effort<`/`:effort>` (alone or in `:and`), `:auto-category`/`-parent`/`-todo`/`-priority` | `:pred`, `:time-grid`, `:effort<`/`>` ORed with other selectors, `:log`, `:habit`, `:regexp`, other `:auto-*` (the block imports ungrouped); `:face`, `:transformer`; a global `org-super-agenda-groups` |
| Tag lists (`org-tag-alist`, `org-tag-persistent-alist`), org-ql | Nothing | Each setting, as not used by agd |

## Tested editor configs

- Emacs configs in vanilla, use-package and Doom (`after!`) styles, and
  one with org-super-agenda groups, read in the pinned Emacs 30.2.
- nvim-orgmode configured by a plain `setup()` call and by a lazy.nvim
  spec, and a config without `setup()`, read in the pinned Neovim 0.12.6.
- First imports deriving roots, re-imports keeping them, and entries
  outside the roots.
