# Private alpha gate

Public 0.1 is gated on fourteen consecutive days of use against the maintainer's
real Org roots with journaling enabled. The gate starts only from a commit that
passes the full release verification matrix.

## Start criteria

- Install the exact candidate commit as the only `agd` binary under test.
- Run `agd setup check` on every approved root and archive the JSON report locally.
- Back up the Org roots through the user's normal backup mechanism.
- Confirm the journal directory is writable and perform one capture plus
  guarded undo on a disposable heading.
- Record the candidate commit, Emacs version, Org version, Neovim version, and
  nvim-orgmode version in the private alpha log.

## Daily protocol

Use the pane and Today views for normal planning. Exercise capture, state,
schedule/deadline, triage, refile, editor open/reload, and at least one MCP
client over the period. Keep normal Emacs or Neovim editing active so
coexistence and stale-ref behavior are exercised.

At the end of each day:

1. Run `agd setup check --json` over all roots.
2. Inspect pending and committed journal records in Agent review.
3. Confirm any `stale_ref`, dirty-lock, or undo conflict corresponded to a real
   concurrent change.
4. Compare changed Org files through normal version control or backup tooling;
   inspect for unrelated-byte drift.
5. Record crashes, refused writes, ambiguous behavior, and the command or key
   sequence that produced them.

Personal paths and Org contents stay in the private log and are never committed
to this repository.

## Reset conditions

Restart the fourteen-day count after any fix to parsing, identity resolution,
patch construction, locking, journal durability, atomic replacement, refile, or
undo. A documentation-only, packaging-only, or presentation-only change does
not reset the count when it cannot affect file discovery or mutation.

Any unexplained source drift, partial multi-file transaction, incorrect target,
or successful write against a stale/dirty target blocks release. Preserve the
files and journal for diagnosis; do not normalize or hand-repair the evidence
before copying it aside.

## Exit criteria

- Fourteen consecutive days completed on one eligible candidate.
- No unexplained byte drift or partial mutation.
- No unresolved crash, target-identity, lock, recovery, or undo defect.
- Every expected refusal had an actionable error and left files unchanged.
- Agenda conformance remains zero-drift for the supported table.
- The final commit passes formatting, Clippy, workspace tests, dependency
  policy, RustSec audit, import oracles, CLI/MCP/TUI smoke scenarios, and Nix CI.
- Release notes link the compatibility and safety boundaries.

The elapsed-use requirement cannot be replaced by CI or a synthetic soak test.
Until a dated private log satisfies this gate, release binaries and packaging
may be prepared, but public 0.1 must not be tagged or announced.
