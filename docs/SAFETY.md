# Safety boundary

`agd` treats Org files as user data. A write either changes the requested byte
spans and preserves all other bytes, or it fails before replacing the file.

## Read boundary

Configured `files.roots` are the trust boundary. Discovery is deterministic
and ignores paths outside those roots. A read through a symlink is accepted
only when canonicalization ends inside an approved root. Files must be UTF-8
for parsing or mutation; `agd setup check` reports invalid input without rewriting it.

A unique `:ID:` is the preferred identity. Duplicate IDs are reported before a
targeted write. Headings without IDs receive a ref composed from file, line
hint, and a truncated SHA-256 digest of the heading bytes. The line is a lookup
hint, not identity: a moved but unchanged heading can resolve, while changed
bytes return `stale_ref` and include the current candidate when one exists.

## Write protocol

Every mutation uses the same core path, including CLI, TUI, pane, capture, and
MCP calls:

1. Resolve the requested item and retain the target path.
2. Refuse a symlink write. This release does not replace symlink targets.
3. Refuse an Emacs lock file that indicates an unsaved buffer.
4. Acquire the sidecar advisory lock for the target.
5. Re-read the file and resolve the original ref against fresh bytes.
6. Build non-overlapping byte-span patches and an exact unified diff.
7. Append and fsync a pending journal record containing hashes and inverse
   patches.
8. Write and fsync a same-directory temporary file with the original mode and
   line-ending convention.
9. Atomically rename the temporary file and fsync the parent directory.
10. Mark the journal record committed and fsync it.

A failed precondition leaves the Org file unchanged. A process interruption
between journal preparation and commit is visible to recovery; it is not
silently treated as a successful write.

`--dry-run` performs resolution and patch construction but writes neither the
file nor the journal. The displayed diff is generated from the exact candidate
post-image.

## Undo

Journal entries store pre-image and post-image hashes plus inverse byte patches.
Undo (`u` in the TUI) applies an inverse only where the bytes it wrote are
still there. Each journaled patch keeps the surrounding lines; if the file
changed elsewhere, undo finds the patch again by that context and applies it
only when it matches exactly once. A later change to the edited lines is a
conflict, so undo never overwrites newer work. Multi-file refile transactions are grouped
and reversed as one operation.

The journal also records the actor and project. CLI operations use a local
actor; MCP operations use the MCP client identity. TUI Agent review can accept
an entry without changing Org data or invoke the same guarded undo.

## Explicit refusals

| Condition | Result |
| --- | --- |
| Duplicate target `:ID:` | `duplicate_id`; no write |
| Missing or changed item ref | `stale_ref`; no write |
| Target is a symlink | `symlink_write_refused`; no write |
| Resolved path escapes all roots | path error; no write |
| Emacs dirty-buffer lock exists | `emacs_dirty_lock`; no write |
| File is non-UTF-8 | parse/input error; no write |
| Patch spans overlap or exceed source | patch error; no write |
| Refile target is inside source subtree | refile error; no write |
| Undo post-image hash differs | conflict; no write |

## What is not promised

- Symlink writes are intentionally unsupported in this release.
- `agd` coordinates through its sidecar lock and detects Emacs dirty locks. An
  unrelated program that ignores both can still race; fresh-byte ref validation
  prevents stale offsets from being applied, but no tool can make an
  uncooperative writer transactional.
- Filesystems must provide ordinary same-directory atomic rename and fsync
  behavior. Network filesystems with weaker semantics are outside the tested
  boundary.
- Encryption, backup, synchronization, and Git history remain the user's
  responsibility. The journal is local recovery metadata, not a backup.

## Privacy and process boundary

Normal operation performs no network requests and starts no background service.
`agd mcp` communicates only over stdin/stdout. Logs go to stderr or local files
and do not contain telemetry exporters.

The only runtime subprocesses are explicit actions: opening the editor from the TUI, editor import,
and opt-in integration hooks. Commands and arguments are passed as argv arrays,
never interpolated into a shell command.

`agd setup import` starts `emacs -Q` or `nvim --clean` with agd's reader. For
Emacs it reads your config files without loading them and evaluates only the
values of the org settings it imports, and the top-level variables they use.
For Neovim it runs the files that mention orgmode, with stand-ins for plugins.
That is your own code, run as you; agd adds no network access. A re-import
keeps the roots you have, and agenda entries outside them are left out with a
diagnostic.
