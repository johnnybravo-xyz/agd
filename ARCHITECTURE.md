# Architecture

`agd` is a local-first Rust workspace built around one constraint: Org files
are user data. A read must not mutate them. A write must change only the
intended bytes or refuse the operation.

## Ownership

| Path | Responsibility |
| --- | --- |
| `agd-core/` | Parsing, discovery, agenda calculation, search, capture, edits, configuration, import, and source-preserving writes |
| `agd/` | The `agd` executable: CLI, TUI, stdio MCP, editor readers, setup, and output formatting |
| `agd/claude-plugin/` | Claude Code MCP entry, skill, `/agd`, `/capture`, and status band |
| `agd/pi/` | Pi and oh-my-pi `/capture` extension installed by setup |
| `conformance/` | Emacs and Neovim oracle fixtures, write fixtures, and byte-exact round trips |

Domain behavior belongs in `agd-core`; executable and integration code should
translate input and output. State is explicit. Production MCP uses stdin and
stdout; no component opens a network listener or performs implicit network
access.

## Read and write flow

```text
config.toml ── load_settings ──▶ Settings
                                      │
                              WorkspaceIndex::load
                                      │
                     ┌────────────────┴────────────────┐
                     │                                 │
               AgendaEngine                       WriteEngine
        agenda, search, saved views       capture, edit, refile, undo
                     │                                 │
          text, CSV, JSON, TUI rows       Mutation, diff, journal record
```

`WorkspaceIndex::load` discovers authorized agenda files and retains both typed
values and original byte spans for headings, planning lines, properties,
checklists, and subtrees. Reads calculate agendas, queries, views, counts, and
exports without normalizing source text or creating journal state.

All mutations pass through `agd-core/src/write/`:

1. Read a fresh index, resolve the target, and reject stale refs, duplicate
   `:ID:` values, and unauthorized paths.
2. Validate the requested semantic change and build sorted, non-overlapping byte
   patches in memory.
3. Return the exact diff immediately for a dry run.
4. Acquire file locks in deterministic order and reject Emacs files with
   unsaved changes.
5. Re-read under lock, resolve targets again, and settle pending journal state.
6. Append and fsync a pending journal record containing hashes and inverse
   patches.
7. Write same-directory temporary files, preserve mode, fsync, rename
   atomically, and fsync directories.
8. Append and fsync the committed journal record.

A multi-file edit is one transaction; failure restores already replaced files.
Undo applies inverse patches only where current bytes still match or relocate
uniquely with context. Changed or ambiguous targets are conflicts, never
permission to overwrite.

## Sources of truth

Each convention has one implementation owner:

| Convention | Source |
| --- | --- |
| Agenda, query, and write semantics | `agd-core/` |
| Configuration keys, defaults, validation, rendering | `agd-core/src/config/` |
| Error codes and versioned schema names | `agd-core/src/model.rs` |
| Key bindings and generated key reference | `agd/src/tui/keys.rs` and `docs/KEYS.md` |
| Colors, glyphs, spacing, and terminal presentation | `agd/src/tui/theme.rs` |
| Human-readable dates, change summaries, and errors | `agd/src/words.rs` |
| User setup, compatibility, safety, and troubleshooting | `docs/getting-started.md` |

`docs/KEYS.md` is generated. Update the key table, then run
`AGD_UPDATE_DOCS=1 cargo test -p agd keys_md`.

## Cross-surface language and interaction

Reuse Org terminology and behavior where supported. Keep one meaning per term,
key, schema field, error code, color, and glyph across the TUI, pane, CLI, and
integrations.

| Concept | Public term |
| --- | --- |
| Primary destinations | Today, Week, Inbox, Agents, Triage, saved views |
| Today sections | OVERDUE, SCHEDULE, DUE SOON, DOING, INBOX |
| Org task or note | heading |
| Mutation | capture, state change, schedule, deadline, priority, tags, checklist, edit, or refile |
| Agent identity | MCP client name, such as `claude-code` or `codex` |

Use Org-agenda bindings where equivalents exist. A key keeps the same meaning
wherever available: `n`/`p` and `j`/`k` move, `x` completes, `C-e` opens the
editor from lists, `Esc` returns one level, and `q` quits. Text fields retain
Emacs editing keys and use `C-o` for the external editor.

Text output preserves Org-owned formats. Piped agenda text and CSV match the
supported pinned-Org subset; JSON uses absolute paths, ISO dates, `HH:MM` times,
and schemas named `agd.<record>/v1`. Public configuration and JSON fields use
snake_case and one name per concept: `olpath`, `todo`, `ref`, `template_key`,
`scheduled`, and `deadline`.

Human errors explain what happened and the recovery action. Machine codes use a
stable `invalid_*`, `unknown_*`, `*_not_found`, `*_conflict`, or `*_failed`
scheme. Exact write previews and guarded undo are preferred to extra
confirmation prompts.

## Verification layers

- Unit tests live beside their code and cover behavior and refusal boundaries.
- `agd/tests/` runs the executable for CLI, MCP, capture, setup, check, and undo
  scenarios.
- TUI tests render real screens on Ratatui's `TestBackend` and verify resulting
  writes.
- `conformance/` compares agenda output, edits, imports, and round trips against
  pinned Emacs and Neovim fixtures.
- Claude Code plugin tests exercise the packaged integration.
- Manual smoke runs use isolated Org/config/data directories and exercise the
  changed CLI, stdio, or terminal surface.

See [CONTRIBUTING.md](CONTRIBUTING.md) for required checks and repository rules.
