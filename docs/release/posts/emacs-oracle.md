# Treating Emacs as an executable Org specification

"Supports Org mode" can mean anything from recognizing stars to reproducing a
decade of agenda semantics. agd avoids that claim. It names a supported
subset and asks Emacs for the expected result every time the subset grows.

The oracle is Emacs's own batch output: `org-batch-agenda-csv` and
`org-batch-agenda`. The CSV gives one structured row per occurrence:
category, heading, occurrence type, TODO state, inherited tags, date, time,
extra text and priority. That is enough to tell a scheduled task from a
deadline warning, a carried-over task from a date stamp, and a range from
either. The text is what a person reads, down to the column widths and
the `Sched.12x:` leaders.

Each fixture directory holds an Org file, a fixed today and date range, and
the CSV and text produced by a pinned Emacs 30.2. agd parses the same file
and must print the same bytes: `agd agenda --csv` and the text agenda are
compared with Emacs's output byte for byte, rows, order and spacing
included. A mismatch prints the full diff.

The first cases were chosen where plausible implementations go wrong: past
scheduled items that carry into today; a deadline warning that starts before
the deadline; `+`, `++` and `.+` repeaters; active timestamp ranges;
inherited file and ancestor tags; and the three done-item skip settings. A
seeded generator adds a case that mixes dates, states, repeaters and
ranges. Tags/TODO matches with property terms, such as
`TODO="DONE"+CLOSED>="<-3w>"`, have their own cases. A weekly CI run asks
the pinned Emacs again for every case, so a fixture can never drift from
what Emacs prints.

Emacs is also the edit oracle, but byte preservation stays a separate
contract. Emacs confirms workflow semantics (custom TODO sequences, repeat
completion, logging), while agd also proves that unrelated bytes did not
move. Matching Emacs does not grant permission to reproduce an editor's
incidental reformatting.

The table of what is not supported matters as much as the passing cases.
Diary sexps, habit consistency, arbitrary skip functions, executable custom
commands and clock reports are unsupported. They stay Org text and are
never interpreted. The org-ql subset is a native feature with an explicit
grammar, not a claim of full package compatibility.

This changes feature work. A bug report becomes the smallest Org fixture
that differs from Emacs, with the editor version recorded. The fix is done
when agd prints what Emacs prints for it. If two Org releases disagree, agd
documents the version boundary rather than choosing whatever a maintainer
remembers.

An executable oracle is slower than intuition and much faster than
debugging a user's calendar after release. It turns compatibility from
branding into a repeatable artifact that reviewers can inspect without
reading the Rust implementation.
