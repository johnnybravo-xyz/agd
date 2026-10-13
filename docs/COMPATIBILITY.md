# Org and editor compatibility

Compatibility claims are fixture-backed. The agenda suite runs the same Org
files through pinned Emacs/Org and `agd`, normalizes only documented transport
differences, and compares CSV rows. The write suite compares resulting bytes
and untouched ranges. Editor imports compare cold native snapshots with values
resolved inside the selected editor.

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
| Custom agenda commands as views (agenda, todo, alltodo, tags, tags-todo blocks) | Exact rows | `agd setup import emacs --verify` runs each view in Emacs and agd |
| Tags/TODO match expressions | Native | `work`, `+a-b`, `a&b`, `a\|b`, `/TODO\|NEXT`, `/-WAIT`, `/!`; property and regexp matches are rejected |
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

| Source | Imported exactly | Imported with a documented difference | Retained disabled/unsupported |
| --- | --- | --- | --- |
| Emacs/Org | Agenda files inside approved roots; TODO sequences; tags; priority bounds; warning and skip settings; sorting; logging; literal capture templates | Editor display-only settings are normalized to native fields | Hooks, advice, function targets, arbitrary Lisp, `%(sexp)`, diary sexps, faces, themes |
| org-super-agenda | Declarative tag, priority, TODO, scheduled, deadline, time-grid, category, `and`, and `not` entries are retained in the snapshot | Retained as portable group records; the native agenda does not execute the package | Arbitrary predicates and executable forms |
| Custom agenda commands (Emacs and nvim-orgmode) | agenda, todo, alltodo, tags and tags-todo blocks, including block agendas, with literal `org-agenda-span` and `org-agenda-overriding-header` | Become `[views.KEY]`, run by the TUI dispatcher and `agd agenda --view KEY`; other options are listed as not imported | Lambdas, function types, search/stuck views, property or regexp matches |
| Installed nvim-orgmode | Agenda files, TODO keywords, tags, priorities, and declarative capture templates exposed by the running plugin | Org/Emacs-only settings use native defaults and are reported as differences | Lua callbacks, functions, a profile where setup was not called, or a plugin version missing the required runtime capabilities |

Every imported entry receives `exact`, `difference`, or `unsupported` status in
`--verify` output. Unsupported entries remain visible in the snapshot and
report; they are not silently dropped or executed.

## Tested editor paths

- Vanilla Emacs batch profile using the fixed probe.
- A selected Emacs daemon through `emacsclient`; no daemon is chosen
  implicitly.
- Explicit Doom/alternate Emacs argv profile.
- Neovim 0.12 with nvim-orgmode 0.7.5 as the reproducible conformance oracle;
  installed plugin versions are accepted when their required runtime
  capabilities load and return supported values.
- Explicit alternate Neovim command/argv profiles.
- Setup-not-called and out-of-root failure fixtures.

The scheduled conformance workflow runs current and previous pinned Org
versions to expose upstream semantic drift before release.
