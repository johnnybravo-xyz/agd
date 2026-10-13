# Treating Emacs as an executable Org specification

"Supports Org mode" can mean anything from recognizing stars to reproducing a
decade of agenda semantics. Agenda avoids that claim. It names a supported
subset and asks Emacs for the expected result every time the subset grows.

The oracle is `org-batch-agenda-csv`. Unlike the formatted agenda buffer, it
emits one structured row per occurrence: category, heading, occurrence type,
TODO state, inherited tags, date, time, extra text, and priority. That is enough
to distinguish a scheduled task from a deadline warning, a carried-over task
from a date stamp, and a range block from either. Faces, columns, and local
presentation functions stay out of the comparison.

Each fixture directory contains an Org file, a fixed today/from/to range, and
the CSV produced by a pinned Emacs/Org pair. The native engine parses the same
file and emits the same schema. The harness normalizes transport details such
as fixture-root paths and line endings, but not semantic fields or row order.
A mismatch prints the full diff.

The initial cases were selected for places where plausible implementations are
wrong: past scheduled items that carry into today; a deadline warning that
starts before the deadline; `+`, `++`, and `.+` repeaters; active timestamp
ranges; inherited file and ancestor tags; and the three done-item skip
settings. Property-generated cases combine dates and settings within bounded
ranges. Scheduled CI runs the suite against current and previous pinned Org
versions so upstream drift becomes a visible decision.

Emacs is also used as an edit oracle, but byte preservation remains a separate
contract. The editor confirms workflow semantics—custom TODO sequences,
repeat completion, and logging—while Agenda additionally proves that unrelated
bytes did not move. Matching Emacs does not grant permission to reproduce an
editor's incidental reformatting.

The table matters as much as the passing cases. Diary sexps, habit consistency,
arbitrary skip functions, executable custom commands, and clock reports are
unsupported. They remain Org source but are not interpreted. Tags/TODO matching
and a small org-ql predicate set are native features with explicit grammar,
not claims of full package compatibility.

This approach changes feature work. A bug report should become the smallest Org
fixture that differs from Emacs, with editor versions recorded. The fix is done
when both the hand case and generated neighborhood agree. If two Org releases
disagree, Agenda documents the version boundary rather than choosing whatever
a maintainer remembers.

An executable oracle is slower than intuition and much faster than debugging a
user's calendar after release. It turns compatibility from branding into a
repeatable artifact that reviewers can inspect without reading the Rust
implementation.
