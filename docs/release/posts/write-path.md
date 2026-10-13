# A write path for files edited by humans and agents

A correct patch can still lose data. Between showing an agenda and pressing
"done," Emacs may have saved the file, a sync tool may have replaced it, or an
agent may have moved the heading. Applying offsets calculated from the old
image is the dangerous shortcut. Agenda's write path is designed around that
race rather than treating it as an edge case.

The client supplies an item ref and an intent such as "set TODO to DONE." The
engine first resolves the ref to identify the target path. It then rejects a
symlink target and an Emacs dirty-buffer lock, acquires a sidecar advisory lock,
and reads the file again. All offsets from the first read are discarded. The
same ref is resolved against fresh bytes; a changed heading returns `stale_ref`
and no patch is applied.

The mutation becomes a sorted list of non-overlapping byte-span replacements.
The engine applies them to retained source in memory and computes both forward
and inverse patches. Before touching the Org file, it appends a pending journal
record containing the pre-image hash, post-image hash, actor, project, and
inverse data, then fsyncs the journal.

The new image is written to a temporary file in the same directory. Its mode
and the original line-ending convention are preserved. The temporary file is
fsynced, renamed atomically over the target, and followed by a parent-directory
fsync. Finally the journal entry is marked committed and fsynced. Recovery can
therefore distinguish a prepared operation from a committed one after a crash.

Undo is not "write the old file." It checks that the current content still has
the recorded post-image hash and applies the inverse byte patches only then. If
an editor changed the file after Agenda's write, undo reports a conflict. A
refile touching two files shares a transaction identifier and reverses both
sides together.

Dry-run uses the same resolution and patch construction but stops before the
journal. Its unified diff is produced from the exact candidate post-image, not
a human summary. This is useful for people and essential for coding agents:
the caller can review the precise Org text while the actual write still repeats
fresh validation later.

MCP adds provenance, not a different mutation path. The stdio server records
the client's advertised name and the process project directory. The TUI's
Agent review screen groups these journal entries, renders the smallest semantic
change, and offers guarded undo. Accepting a change writes no Org data; it only
marks the review record.

The protocol intentionally refuses cases that need more design. Symlink writes
remain disabled. Non-UTF-8 files are not transcoded. Duplicate IDs never select
a winner. An uncooperative program can ignore advisory locks, but it cannot
make Agenda apply a stale digest as if it still matched. The invariant is
narrow and testable: preserve unrelated bytes, or do nothing.
