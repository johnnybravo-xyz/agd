# A write path for files edited by people and agents

A logically correct patch can still lose data. Between displaying an agenda and
marking a task done, Emacs may save the file, a sync tool may replace it, or an
agent may move the heading. Applying offsets calculated from the old image is
the dangerous shortcut. `agd` treats that race as the normal case.

## Resolve intent, not stale offsets

A client submits a heading ref and an intent such as “set TODO to DONE.” The
write engine resolves the ref against a fresh workspace read and computes a
candidate change. It refuses symlink writes and files Emacs holds with unsaved
changes through a `.#` lock.

A dry run ends here. It returns the exact diff and creates no lock, journal
record, directory, or file.

## Re-read under lock

A real write acquires an advisory lock stored in `agd`'s data directory, never
beside the Org file. Synced folders therefore remain free of tool lock files.

Under that lock, the engine reads the source again and discards every offset
from the earlier read. It resolves the same ref against the current bytes. If
the heading changed, the write returns `ref_conflict` without applying a patch.

## Journal before replacement

The intended change becomes a sorted list of non-overlapping byte
replacements, applied in memory. Before the Org file changes, `agd` appends and
fsyncs a pending journal record containing:

- hashes of the old and new file;
- the actor and project;
- each forward and inverse patch;
- surrounding lines used to relocate an undo safely.

The new bytes are written to a temporary file in the same directory. `agd`
preserves the original mode, fsyncs the temporary file, renames it over the
target, and fsyncs the directory. It then appends and fsyncs the committed
journal record. Line endings remain unchanged because bytes outside patches are
copied directly.

If the process stops between the two records, the next write compares the file
with both hashes. New bytes mean the write happened; old bytes mean it did not;
anything else blocks further writes until the user reviews the file and runs
`agd setup check --resolve`.

## Undo is another guarded patch

Undo never writes an old whole-file snapshot. If the source still contains
exactly what `agd` wrote, the engine applies inverse patches. If unrelated lines
changed, it relocates each patch using its context and applies it only when the
written text occurs exactly once.

A changed or ambiguous edited range becomes `undo_conflict`, naming the heading
and actor, and leaves the file untouched. Refile and bulk edit are multi-file
transactions and undo as a unit.

## Agent provenance uses the same protocol

MCP adds identity, not a separate write implementation. The stdio server
journals each mutation under the connected client, such as
`mcp:claude-code`, and records its working directory as the project.

The TUI's Agents tab shows those records, their diffs, and the same guarded undo.
Accepting a change updates only journal review state; it does not rewrite the
Org file.

## Keep the invariant narrow

Some cases remain refusals by design:

- writes through symlinks;
- non-UTF-8 source;
- duplicated IDs;
- ambiguous undo context;
- stale heading refs.

An uncooperative process can ignore advisory locks, but it cannot make `agd`
apply a patch to bytes that moved because target resolution happens again under
the lock. The invariant is narrow and testable: preserve every unrelated byte,
or do nothing.
