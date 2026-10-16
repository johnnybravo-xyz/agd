# Architecture

agd is two crates and a plugin:

| Path | What it is |
| --- | --- |
| `agd-core/` | The library (`agd_core`): parsing, the index, the agenda, every write, configuration and import. All behavior lives here. |
| `agd/` | The `agd` binary: the CLI, the TUI, the stdio MCP server, the importer's editor readers. It turns input into calls on `agd_core` and results into text, JSON or screens. |
| `agd/claude-plugin/` | The Claude Code plugin: the MCP server entry, the `agenda` skill and a mod (`/agd` pane, `/capture`, the band above the prompt). It only runs the `agd` binary. |
| `conformance/` | Cases checked against pinned Emacs and Neovim: agenda text and CSV, edits, import, and byte-exact round trips. |

The rule that shapes everything: **org files are user data.** A write
changes only the bytes it means to change, or it is refused. Reads never
write. Nothing talks to the network.

## How a request flows

```text
config.toml ──load_settings──▶ Settings
                                  │
                       WorkspaceIndex::load      (every agenda file, parsed,
                                  │               byte spans kept)
                 ┌────────────────┴────────────────┐
           AgendaEngine                        WriteEngine
   agenda days, TODO lists, tags,      capture, edit, edit_batch, refile,
   org-ql queries, saved views         undo; each one journaled
                 │                                  │
     text, CSV, JSON, TUI rows         Mutation (the new item, the diff,
                                       the journal id)
```

Every surface goes through the same two engines, so the CLI, the TUI, MCP
and the Claude Code mod can't disagree about what an agenda shows or what a
write does.

A write, in order (`agd-core/src/write/`):

1. Resolve the target ref against a fresh index; refuse stale refs,
   duplicate `:ID:`s and paths outside the roots (`path.rs`).
2. Validate the input: no newlines, headings or tags smuggled into a title,
   priority within the file's range (`validation.rs`).
3. Build byte patches (`org_edit.rs`, `placement.rs`) and apply them in
   memory (`patch.rs`); refuse a change that would touch other bytes.
4. On a dry run, stop here and return the diff.
5. Take the file's lock in agd's data directory and refuse if an editor has
   the file locked (`file.rs`), write a pending journal record, replace the
   file atomically, then mark the record committed (`journal.rs`).

Undo (`undo.rs`) applies the journaled inverse patch only where those bytes
are still unchanged.

## agd-core

Every module starts with a `//!` note on what it owns; this is the map.

| Module | Owns |
| --- | --- |
| `config/` | `Settings` and the records in `config.toml` (`model.rs`), reading and writing the file (`file.rs`), checking every value with an error that names the key (`validation.rs`). |
| `document/` | One org file parsed from its bytes (`org_document.rs`), headlines and properties as spans (`headline.rs`), in-file directives such as `#+TODO`, `#+STARTUP`, `#+PRIORITIES` (`directive.rs`), timestamps (`timestamp.rs`), and `WorkspaceIndex` over all agenda files (`index.rs`). |
| `agenda/` | `AgendaEngine` (`engine.rs`) and what it is made of: spans, saved views and groups, keyword colors, the Emacs tags/TODO matcher, the org-ql query subset, sorting, repeaters, and the Emacs text and CSV exports. |
| `write/` | `WriteEngine` (`engine.rs`) and every write: edits and batches, capture and placement, refile, undo, validation, the journal, file safety and patches. |
| `date.rs` | Org's date prompt: `fri`, `+2d`, `-wed`, `oct 30 9am`, as org-read-date reads them. |
| `capture_line.rs`, `capture_template.rs` | The one-line capture syntax (`due:fri #tag !A`, or `:tag:` and `#A` with `notation = "org"`) and capture templates (`%?`, `%U`, `%^{prompt}`). |
| `path.rs` | `RootSet`: which paths agd may touch, and where config and data live. |
| `import.rs` | Turning an Emacs or Neovim configuration into `config.toml`. |
| `check.rs` | `agd setup check`, a read-only report on the files and the journal, through the index. |
| `model.rs` | What callers get back: `Item`, `AgendaEntry`, `Mutation`, `AgdError`, every `ErrorCode` and every schema name (`agd.item/v1`, …). |

## agd

| Path | Owns |
| --- | --- |
| `main.rs` | Argument parsing and how errors print (a sentence on a terminal, `agd.error/v1` with `--json`). |
| `commands/` | One file per command: `agenda`, `capture`, `undo`, `completions`, and `setup` with `import`, `install`, `check`. |
| `importer.rs`, `readers/` | Running Emacs or Neovim in batch mode to read an editor configuration, without loading the person's startup. |
| `mcp/` | The stdio MCP server: `server.rs` (instructions, the `agenda://today` resource), `tools.rs` (one section per tool), `describe.rs` (what an agent needs before it writes). |
| `tui/` | The terminal UI. `mod.rs` holds `App` and the event loop; `keys.rs` is the one key table that drives dispatch, footers, Triage's chips and `?`; `theme.rs` is the visual system; the other files are one screen or popup each. |
| `words.rs` | How agd says things: dates, change lines, errors. The TUI, the pane and the CLI's terminal sentences all use it. |

## Where conventions live

Each convention has one home. Change it there, and every surface follows.

| Convention | Home |
| --- | --- |
| Colors, glyphs, spacing and type | `agd/src/tui/theme.rs` (the Claude Code pane reads the same palettes from `agd agenda --views --json`) |
| Key bindings | `agd/src/tui/keys.rs` (rendered into [`docs/KEYS.md`](docs/KEYS.md) by a test) |
| Dates, change lines, error sentences | `agd/src/words.rs` |
| Error codes and schema names | `agd-core/src/model.rs` |
| Config keys, defaults and accepted values | `agd-core/src/config/` |
| The design system as a whole | [`docs/DESIGN.md`](docs/DESIGN.md) |

## Tests

- Unit tests sit next to the code they test, with shared fixtures in each
  module's `test_support.rs`.
- `agd/tests/` runs the real binary: CLI scenarios, MCP over stdio, undo,
  capture, check.
- `agd/src/tui/tests/` (one file per screen) draws screens on Ratatui's
  `TestBackend` and checks both what is drawn and what is written.
- `conformance/` compares agd with pinned Emacs and Neovim byte for byte;
  the cases run in `cargo test`, and `just oracles` regenerates their
  expected output from the real editors (Docker).
- `agd/claude-plugin/tests/` tests the mod with `claude plugin test`.

See [CONTRIBUTING.md](CONTRIBUTING.md) for how to run them.
