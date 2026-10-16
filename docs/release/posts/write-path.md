# A write path for files edited by humans and agents

A correct patch can still lose data. Between showing an agenda and pressing
"done," Emacs may have saved the file, a sync tool may have replaced it, or an
agent may have moved the heading. Applying offsets calculated from the old
image is the dangerous shortcut. agd's write path is designed around that
race rather than treating it as an edge case.

The client supplies an item ref and an intent such as "set TODO to DONE." The
engine resolves the ref against a fresh read of the files and computes the
change. It refuses a symlink target and a file Emacs holds unsaved changes
for (its `.#` lock). A dry run stops here and returns the diff, creating
nothing at all: no lock, no journal entry, no file.

A real write then takes the file's advisory lock. Lock files live in agd's
own data directory, never beside your Org files, so a synced folder stays
clean. Under the lock the engine reads the file again and discards every
offset from the first read. The same ref is resolved against the fresh
bytes; a changed heading returns `ref_conflict` and no patch is applied.

The change becomes a sorted list of non-overlapping byte replacements,
applied to the source in memory. Before touching the Org file, the engine
appends a pending journal record with the hashes of the old and new file,
who made the change, the project, and the patches with the lines around
them, then fsyncs the journal.

The new file is written to a temporary file in the same directory with the
original mode; line endings stay because only patched bytes change. The
temporary file is fsynced, renamed over the target, and the directory is
fsynced. Finally a committed record is appended and fsynced. If agd stops in
between, the next write to that file settles the pending record from the
file's bytes: the new bytes mean it happened, the old bytes mean it never
did, and anything else blocks writes to that file until the person looks at
it and runs `agd setup check --resolve`.

Undo is not "write the old file." If the file still holds exactly what agd
wrote, undo applies the recorded patches in reverse. If the file changed
elsewhere, undo finds each patch again by its surrounding lines and applies
it only when that text occurs exactly once. If someone changed the edited
lines themselves, undo is a conflict that names the heading and who changed
it, and writes nothing. A refile or a bulk edit touching several files is
one transaction and is undone as one.

MCP adds provenance, not a different write path. The stdio server journals
each write under the client's advertised name, such as `mcp:claude-code`,
with its working directory as the project. The TUI's Agents tab lists these
changes, shows each one's diff, and offers the same guarded undo. Accepting
a change writes nothing to the Org files; it only marks the journal record
as reviewed.

The protocol refuses cases that need more design. Symlink writes stay
disabled. Files that are not UTF-8 are not transcoded. A duplicated ID never
picks a winner. An uncooperative program can ignore advisory locks, but it
cannot make agd apply a patch to bytes that moved. The invariant is narrow
and testable: preserve unrelated bytes, or do nothing.
