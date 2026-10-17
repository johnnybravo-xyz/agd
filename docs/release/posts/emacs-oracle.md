# Treating Emacs as an executable Org specification

“Supports Org mode” can describe anything from recognizing heading stars to
reproducing years of agenda semantics. `agd` makes a narrower claim: it defines
a supported subset and asks pinned Emacs for the expected result whenever that
subset changes.

## The oracle

The agenda oracle uses Emacs's own batch interfaces:

- `org-batch-agenda-csv` produces one structured row per occurrence, including
  category, heading, occurrence type, TODO state, inherited tags, date, time,
  extra text, and priority.
- `org-batch-agenda` produces the text people read, including column widths and
  leaders such as `Sched.12x:`.

Together they distinguish a scheduled task from a deadline warning, a
carried-over item from a plain timestamp, and a range from either.

Each fixture directory contains an Org file, a fixed “today,” a date range, and
the CSV and text produced by pinned Emacs 30.2. `agd` parses the same source and
must emit the same bytes. Rows, ordering, spacing, and labels all participate in
the comparison; a failure prints the complete diff.

## Choosing fixtures

The first cases target behavior that plausible implementations often get
wrong:

- past scheduled items that carry into today;
- deadline warnings that begin before the due date;
- `+`, `++`, and `.+` repeaters;
- active timestamp ranges;
- inherited file and ancestor tags;
- completed-item skip settings.

A seeded generator adds mixed dates, states, repeaters, and ranges. Separate
cases cover tags/TODO property expressions such as
`TODO="DONE"+CLOSED>="<-3w>"`.

A weekly CI job reruns every fixture through the pinned Emacs, preventing a
recording from drifting away from the editor output it claims to represent.

## Writes need two contracts

Emacs also acts as the semantic oracle for edits: custom TODO sequences,
repeater completion, logging, planning placement, and related behavior. Byte
preservation remains an independent contract.

Matching Emacs does not permit `agd` to reproduce incidental reformatting. A
write fixture compares the resulting file with Emacs and also proves that every
byte outside the planned patches remained untouched.

## Unsupported behavior remains explicit

The compatibility table is as important for exclusions as for passing cases.
`agd` does not evaluate diary sexps, arbitrary skip functions, executable custom
commands, clock reports, or habit consistency graphs. The org-ql integration is
a documented subset with an explicit grammar, not a claim of complete package
compatibility.

## What this changes in development

A compatibility bug becomes the smallest Org fixture that differs from Emacs,
with the editor version recorded. The fix is complete when `agd` reproduces the
oracle for that case without weakening source preservation.

If Org releases disagree, `agd` records the version boundary instead of relying
on maintainer memory. The executable oracle costs more than intuition and much
less than debugging a user's calendar after release. It turns compatibility
from branding into a repeatable artifact reviewers can inspect without reading
the Rust implementation.
