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
| Deadline warning window and per-timestamp warning | Exact | `conformance/agenda/warnings` |
| `+`, `++`, and `.+` day/week/month/year repeaters | Exact | `conformance/agenda/repeaters` and generated cases |
| Active timestamp ranges and range progress | Exact | `conformance/agenda/ranges` |
| Inherited file and ancestor tags | Exact | `conformance/agenda/inheritance` |
| Done-item skip settings for planning/timestamps | Exact | `conformance/agenda/done-skipping` |
| In-file `#+TODO` and `#+SEQ_TODO` workflows | Exact for parsing and writes | Round-trip and edit oracle fixtures |
| Priority, time, and category agenda ordering | Exact for supported strategies | Generated agenda oracle cases |
| Tags/TODO match expressions | Native | Boolean include/exclude tags plus TODO terms |
| org-ql `and`, `or`, `not`, `todo`, `tags`, `priority`, `category`, `text`, `deadline`, `scheduled`, `done` | Native subset | Unknown predicates fail explicitly |
| Plain day/week/month lists and CSV | Native | CSV fields follow the Emacs oracle schema |
| Clock table, clocking UI, and effort calculations | Unsupported | Org source is preserved; no clock evaluator |
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
| TODO state | Uses the file's own TODO sequence; repeat completion and logging honor supported Org settings |
| Schedule/deadline | Replaces only the planning timestamp span; repeaters and warning suffixes remain typed data |
| Title/tags/priority/properties | Patches the smallest source span and preserves surrounding bytes |
| Capture | Literal templates and supported placeholders render exact Org text before insertion |
| Refile | Moves the original subtree bytes; destination identity is revalidated under locks |
| Undo | Replays inverse byte patches only against the recorded post-image |

The edit oracle covers no-op round trips, state changes, planning changes,
repeater completion, logbook behavior, and preservation of unrelated byte
ranges. Symlink writes remain intentionally unsupported.

## Import compatibility

| Source | Imported exactly | Imported with a documented difference | Retained disabled/unsupported |
| --- | --- | --- | --- |
| Emacs/Org | Agenda files inside approved roots; TODO sequences; tags; priority bounds; warning and skip settings; sorting; logging; literal capture templates | Editor display-only settings are normalized to native fields | Hooks, advice, function targets, arbitrary Lisp, `%(sexp)`, diary sexps, faces, themes |
| org-super-agenda | Declarative tag, priority, TODO, scheduled, deadline, time-grid, category, `and`, and `not` entries are retained in the snapshot | Retained as portable group records; the native agenda does not execute the package | Arbitrary predicates and executable forms |
| Custom Emacs agenda commands | Agenda/TODO/tags/tags-todo/search and block metadata are retained when declarative | Retained as portable command records for inspection | Arbitrary skip lambdas and executable command forms |
| nvim-orgmode 0.7.5 | Agenda files, TODO keywords, tags, priorities, and declarative capture templates exposed by the probe | Org/Emacs-only settings use native defaults and are reported as differences | Lua callbacks, functions, or a profile where orgmode setup was not called |

Every imported entry receives `exact`, `difference`, or `unsupported` status in
`--verify` output. Unsupported entries remain visible in the snapshot and
report; they are not silently dropped or executed.

## Tested editor paths

- Vanilla Emacs batch profile using the fixed probe.
- A selected Emacs daemon through `emacsclient`; no daemon is chosen
  implicitly.
- Explicit Doom/alternate Emacs argv profile.
- Neovim 0.12 with nvim-orgmode 0.7.5 through a fixed headless Lua probe.
- Explicit alternate Neovim command/argv profiles.
- Setup-not-called and out-of-root failure fixtures.

The scheduled conformance workflow runs current and previous pinned Org
versions to expose upstream semantic drift before release.
