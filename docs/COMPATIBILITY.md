# Org and editor compatibility

Compatibility claims are fixture-backed. The agenda suite runs the same Org
files through pinned Emacs/Org and `agd`, normalizes only documented transport
differences, and compares CSV rows. The write suite compares resulting bytes
and untouched ranges. The import suite reads fixture configs with agd's
editor readers and compares their output.

## Agenda semantics

`Exact` means the checked fixture currently produces zero semantic drift from
Emacs `org-batch-agenda-csv`. `Native` means supported by `agd` but not claimed
as a byte-for-byte Emacs display clone. `Unsupported` is detected or ignored
without executing editor code.

| Org behavior | Status | Evidence or boundary |
| --- | --- | --- |
| Scheduled, deadline, and active timestamp rows | Exact | `conformance/agenda/plain` |
| Past scheduled carry-over | Exact | Plain and generated oracle cases |
| Overdue repeating deadlines, repeats only on base date and after today | Exact | `conformance/agenda/overdue-repeats` |
| Habit repeaters, Unicode tags and weekdays, ignored block/link timestamps | Exact | `conformance/agenda/syntax-edges` |
| Planning only on the line after the headline | Exact | `conformance/agenda/planning-lines` |
| Deadline warning window and per-timestamp warning | Exact | `conformance/agenda/warnings` |
| `+`, `++`, and `.+` day/week/month/year repeaters | Exact | `conformance/agenda/repeaters` and generated cases |
| Active timestamp ranges and range progress | Exact | `conformance/agenda/ranges` |
| Inherited file and ancestor tags | Exact | `conformance/agenda/inheritance` |
| Done-item skip settings for planning/timestamps | Exact | `conformance/agenda/done-skipping` |
| In-file `#+TODO` and `#+SEQ_TODO` workflows | Exact for parsing and writes | Round-trip and edit oracle fixtures |
| Row order: urgency, priority, time, category, alpha, todo-state keys | Exact | All cases compare Emacs's own row order; `conformance/agenda/sorting` sets a custom strategy |
| Custom agenda commands as views (agenda, todo, alltodo, tags, tags-todo blocks) | Exact rows | `agd setup import <emacs config> --verify` runs each view in Emacs and agd |
| Tags/TODO match expressions, with property terms | Exact | `conformance/agenda/match-*`: `work`, `+a-b`, `a\|b`, `/!`, `/-WAIT`, and `TODO="DONE"`, `PRIORITY="A"`, `LEVEL=2`, `CATEGORY="home"`, `Effort<15`, `CLOSED>="<-3w>"`, `DEADLINE<="<+1w>"`, `-STATUS="review"` with Org's string, number and time comparisons; regexp terms (`{...}`) are rejected |
| org-super-agenda groups in view blocks | Native | First matching group takes a row, `discard`, `order` (equal non-zero orders by name), `effort_max`/`effort_min` (inclusive, the heading's own `:Effort:` read as org-duration-to-minutes does), `by` category, parent, todo or priority, "Other items" last at order 99, empty groups hidden; no oracle, since the pinned Emacs has no org-super-agenda |
| org-ql `and`, `or`, `not`, `todo`, `tags`, `priority`, `category`, `text`, `deadline`, `scheduled`, `done` | Native subset | Unknown predicates fail explicitly |
| Plain day/week/month lists and CSV | Native | CSV fields follow the Emacs oracle schema |
| Effort totals | Native | Today sums `:Effort:`; no column view |
| Clock table and clocking UI | Unsupported | Org source is preserved; no clock evaluator |
| Habit graph and habit consistency calculations | Unsupported | Preserved as source, not interpreted |
| Diary sexps and `%%(...)` | Unsupported | Never evaluated |
| Arbitrary custom agenda skip functions | Unsupported | Never evaluated |
| Executable Lisp in agenda definitions | Unsupported | Never evaluated |
| Calendar grid rendering | Unsupported | Day-grouped agenda only |

Emacs's formatted agenda buffer is not the oracle. Prefix text, faces, time-grid
lines, and custom display functions are presentation behavior. The oracle uses
structured rows: category, title, type, TODO, tags, date, time, extra, and
priority.

## Write compatibility

| Mutation | Contract |
| --- | --- |
| TODO state | Every `#+TODO`/`#+SEQ_TODO`/`#+TYP_TODO` line or the configured sequences; `!`/`@` keyword markers, `org-log-done`, `org-log-repeat`, `org-log-into-drawer` and `#+STARTUP` log options write the same CLOSED, LAST_REPEAT and LOGBOOK lines as Emacs; repeats return to the head of the keyword's sequence |
| Schedule/deadline | Replaces only the planning timestamp and keeps the old repeater and warning cookie, as `org-schedule` does; `org-log-reschedule`/`org-log-redeadline` add Emacs's "Rescheduled from" and "New deadline from" notes |
| Title/tags/priority/properties | Patches the smallest source span and preserves surrounding bytes |
| Capture | Literal templates and supported placeholders render exact Org text before insertion |
| Refile | Moves the original subtree bytes; destination identity is revalidated under locks |
| Undo | Replays inverse byte patches where their bytes and surrounding lines are unchanged; anything else is a conflict |
| Tags | Realigned to `org-tags-column` whenever a headline is rewritten, as Emacs does |

The edit oracle covers no-op round trips, state changes, planning changes,
repeater completion, logbook behavior, and preservation of unrelated byte
ranges. Symlink writes remain intentionally unsupported.

## Import compatibility

`agd setup import` reads your editor config files; it never runs your
editor's startup. [IMPORTING.md](IMPORTING.md) lists every setting and every
case that does not import.

| Source | Imported | Reported, not imported |
| --- | --- | --- |
| Emacs config files (`.el`, literate `config.org`) | Org settings assigned at the top level or in `after!`, `with-eval-after-load`, `use-package` and Spacemacs's user-config; their values evaluated in `emacs -Q` | Values that call your own or a package's functions; defaults set by Doom modules or packages; settings inside `let`, hooks or your own functions |
| nvim-orgmode (`setup()` call or lazy.nvim spec) | Every option agd uses, with nvim-orgmode's defaults for the rest; agenda globs kept | No `setup()` in the files read; options built from other plugins' APIs |
| Capture templates | `entry` templates with `file`, `file+headline`, `file+olp`, datetree or `id` targets | Other types, computed targets or bodies, `%(sexp)` |
| Custom agenda commands | agenda, todo, alltodo, tags and tags-todo blocks with literal span and header, including property matches | Lambdas, search/stuck views, regexp matches (the view is left out); other options |
| org-super-agenda groups (a block's `org-super-agenda-groups`, literal or a variable) | `:name`, `:order`, `:discard`, `:todo`, `:tag`, `:category`, `:priority`, `:deadline`/`:scheduled` `t`/`nil`/`past`/`today`/`future`, `:property`, `:anything`, `:and`, `:not`, `:effort<`/`:effort>` (alone or in `:and`), `:auto-category`/`-parent`/`-todo`/`-priority` | `:pred`, `:time-grid`, `:effort<`/`>` ORed with other selectors, `:log`, `:habit`, `:regexp`, other `:auto-*` (the block imports ungrouped); `:face`, `:transformer`; a global `org-super-agenda-groups` |
| org-ql | Nothing | Each setting, as not used by agd |

## Tested editor paths

- Emacs configs in vanilla, use-package and Doom (`after!`) styles, read by the
  reader in the pinned Emacs 30.2 image.
- nvim-orgmode configured by a plain `setup()` call and by a lazy.nvim spec,
  and a config without `setup()`, read in the pinned Neovim 0.12.6.
- First imports deriving roots, re-imports keeping them, and entries outside
  the roots.

The scheduled conformance workflow runs current and previous pinned Org
versions to expose upstream semantic drift before release.
