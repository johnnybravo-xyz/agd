# Safety boundary

agd treats Org files as user data. A write changes the bytes it means to
change and keeps every other byte, or it fails before the file is replaced.

## What agd reads

The `roots` in `config.toml` are the trust boundary: agd reads and writes
only files under them. A path with a `..` segment is refused. A symlink is
followed only when it resolves inside a root; one that leaves the roots is
skipped, and `agd setup check` reports it (`invalid_path`). Files must be
UTF-8; a file that is not is skipped and reported (`invalid_utf8`), never
rewritten.

A unique `:ID:` is a heading's identity. A heading without one gets a ref
made of its file, a line hint and a digest of the heading line
(`file:line#sha256:digest`). The line is only a hint: an unchanged heading
that moved still resolves, while a changed heading returns `ref_conflict`
with the current heading at that line, when there is one. An `:ID:` that occurs
more than once under the roots, in agenda files or not, is refused for
writes (`id_conflict`). agd never gives a heading an ID another heading
has, and never changes or removes an existing `:ID:` (`invalid_id_change`).

## How a write happens

Every write takes the same path, whether it comes from the CLI, the TUI,
the pane, capture or an agent over MCP:

1. Resolve the ref against a freshly read index and compute the change.
2. Refuse a target outside the roots or a symlink (`invalid_path`), and a
   file Emacs holds unsaved changes for, shown by its `.#file` lock
   (`editor_conflict`).
3. A dry run stops here and prints the diff. It creates nothing: no lock,
   no journal entry, no directory, no file.
4. Take the file's advisory lock. Lock files live in agd's data directory,
   `$XDG_DATA_HOME/agd/locks` (`~/.local/share/agd/locks` by default),
   never beside your files. Several files are locked in sorted order.
5. Settle any interrupted earlier write to the file (see below).
6. Read the file again and resolve the ref against the fresh bytes.
7. Build non-overlapping byte patches (`invalid_patch` otherwise) and the
   exact unified diff.
8. Append a pending journal record with the hashes of the old and new file
   and the patches, and fsync it.
9. Write the new bytes to a temporary file in the same directory, keep the
   file's mode, fsync it, rename it over the file and fsync the directory.
10. Append the committed record and fsync it.

Line endings are kept because only the patched bytes change. A write to
several files, such as a refile across files or an edit of marked rows, is
one transaction: when one file cannot be written, the files already
written get their old bytes back.

## Interrupted writes

When agd stops between steps 8 and 10, the next write to that file reads
the file to settle the pending record: the new bytes mean the write
happened, the old bytes mean it never did. Anything else means the file
changed since, and agd refuses writes to that file (`journal_conflict`)
until you look at it and run `agd setup check --resolve`, which keeps the
file exactly as it is on disk. `agd setup check` lists such files.

## Undo

The journal records each change's patches with the lines around them.
Undo puts the old bytes back only where the file still holds exactly what
agd wrote:

- If the file is unchanged since the write, undo applies the recorded
  patches.
- If the file changed elsewhere, undo finds each patch again by its
  surrounding lines and the text it wrote, and applies it only when that
  text occurs exactly once.
- Otherwise nothing is written, and the error (`undo_conflict`) names the
  heading as the file reads, who made the change and what to do: `can't undo:
  Water the plants changed after you edited it (inbox.org:7); edit it by
  hand`.

A transaction is undone as one: every file of a refile or a bulk edit, or
none. Undo takes the same locks and refuses the same Emacs locks as a
write. A change already undone, or one that never completed, is refused
(`invalid_journal_state`).

Each surface undoes its own changes:

- `u` in the TUI undoes the latest change this TUI session made; at the end
  of a Triage pass it undoes the whole pass.
- `agd undo` undoes your latest command-line change that is still in effect,
  never one made in the TUI or by an agent.
- `u` on the Agents tab undoes the selected agent change; `a` accepts it and
  writes nothing to your files.

The journal records who made each change: `cli`, `tui`, or `mcp:<client>`
for an agent, such as `mcp:claude-code`, with the project directory it ran
in.

## Refusals

| Condition | Code | Result |
| --- | --- | --- |
| An `:ID:` more than one heading has | `id_conflict` | nothing written |
| A change to or removal of an existing `:ID:` | `invalid_id_change` | nothing written |
| A ref whose heading changed or is gone | `ref_conflict` | nothing written; the error carries the current heading |
| A symlink target, a path outside the roots, a `..` segment | `invalid_path` | nothing written |
| Emacs holds unsaved changes (`.#file` lock) | `editor_conflict` | nothing written |
| A file that is not UTF-8 | `invalid_utf8` | nothing written |
| Patches that overlap or run past the file | `invalid_patch` | nothing written |
| A refile into the moved subtree itself | `invalid_refile` | nothing written |
| An interrupted write whose file changed since | `journal_conflict` | writes to that file refused until `agd setup check --resolve` |
| An undo whose text changed since | `undo_conflict` | nothing written |

## Limits

- Writes through symlinks are refused.
- agd coordinates through its own locks and Emacs's `.#` locks. A program
  that ignores both can still race with a write; re-reading the file under
  the lock keeps agd from applying a change to bytes that moved, but no
  tool can make an uncooperative writer transactional.
- Atomic replacement needs a filesystem with ordinary same-directory
  rename and fsync. Network filesystems with weaker guarantees are outside
  what is tested.
- Encryption, backups, sync and version history stay yours. The journal is
  for undo and recovery, not a backup.

## Privacy and processes

agd makes no network requests and starts no background service. `agd mcp`
talks only over stdin and stdout and logs to stderr; there is no
telemetry. agd's data directory, with the journal and the locks, is
readable only by you (mode 0700; the journal 0600).

agd starts a program only when you ask: `$EDITOR` (the `editor` setting)
from the TUI, and your editor during `agd setup import`. Commands and
arguments are passed as argument lists, never through a shell.

`agd setup import` starts `emacs -Q` or `nvim --clean` with agd's own
reader. For Emacs it reads your config files without loading them and
evaluates only the values of the Org settings it imports and the top-level
variables they use. For Neovim it runs the files that mention orgmode, with
stand-ins for plugins. That is your own code, run as you; agd adds no
network access. A reader that has not finished after 120 seconds is
stopped.
