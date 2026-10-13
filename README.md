# agd

[![CI](https://github.com/johnnybravo-xyz/agd/actions/workflows/ci.yml/badge.svg)](https://github.com/johnnybravo-xyz/agd/actions/workflows/ci.yml)
[![agd on crates.io](https://img.shields.io/crates/v/agd.svg)](https://crates.io/crates/agd)
[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](LICENSE)

`agd` is a local-first agenda and safe write engine for org-mode files. One
binary provides deterministic CLI output, a keyboard-first TUI, a narrow pane,
and a stdio MCP server. It reads and writes normal Org files; Emacs and Neovim
remain the editors of record.

The write path preserves every untouched byte, revalidates item identity under
a lock, journals the inverse patch before replacement, and refuses a write
rather than guessing when the target is stale or ambiguous.

## Install

Rust 1.89 or newer is required for source installs.

```bash
cargo install agd --locked
```

Current source:

```bash
git clone https://github.com/johnnybravo-xyz/agd.git
cd agd
cargo install --path agd --locked
```

Tagged releases also publish checksummed binaries for Linux, macOS, and
Windows, a Homebrew formula, and a Nix package:

```bash
brew install johnnybravo-xyz/agd/agd
nix run github:johnnybravo-xyz/agd
```

Release archives have per-file SHA-256 hashes plus a keyless Sigstore bundle
for the aggregate `SHA256SUMS` file. The Arch recipe lives in
[`packaging/aur`](packaging/aur).

## First run

Create the native configuration, point it at one or more existing Org roots,
and validate the files before enabling writes:

```bash
agd config init
$EDITOR "$(agd config path)"
agd check
agd agenda --span day
```

With no subcommand, `agd` opens Today in the full-screen TUI. `agd pane` opens
the compact glance surface. `agd capture -i` opens only the focused capture
popup.

## Command surface

```text
agd agenda        Day, week, or month occurrences; text, JSON, or Emacs CSV
agd todo          Unfinished items with an optional Org tags matcher
agd q             Supported org-ql subset
agd capture       Template capture, exact dry-run preview, or interactive popup
agd state         Change TODO state
agd schedule      Set or remove SCHEDULED
agd deadline      Set or remove DEADLINE
agd edit          Change title, tags, priority, properties, or add a note
agd open          Open the exact heading with configured editor argv
agd refile        Move one subtree below another heading
agd undo          Guarded journal rollback
agd check         Parsing, byte-stability, path, UTF-8, and duplicate-ID checks
agd import        Cold Emacs or nvim-orgmode configuration snapshot
agd install       Claude Code, Codex, or OpenCode integration
agd mcp           Stdio-only MCP server
agd tui           Full terminal workflow
agd pane          Narrow watched agenda
```

The retained `list`, `read`, `outline`, `heading`, `element-by-id`, and `search`
commands provide direct knowledge-base reads. Run `agd <command> --help` for the
complete arguments.

### Agenda and queries

```bash
agd agenda --span day --from 2026-10-13
agd agenda --span week --json
agd agenda --span day --csv
agd todo --match '+work/NEXT'
agd q '(and (todo "NEXT") (tags "work"))' --json
```

Agenda JSON and TODO output include a stable `ref`. A heading `:ID:` is used
when unique. Otherwise the ref is a file, line hint, and content digest. Write
commands accept either form and return `stale_ref` if the referenced heading
has changed.

### Writes and previews

```bash
# Capture with template key t.
agd capture t "Renew passport" --scheduled +2d --tag personal

# Preview exact source changes without touching the file or journal.
agd state '<ref>' DONE --dry-run
agd edit '<ref>' --tags +work,-inbox --priority A --dry-run

# Apply and, if necessary, roll back the latest journal entry.
agd schedule '<ref>' fri
agd undo
```

All machine-facing write commands accept `--json`. Errors use stable codes such
as `stale_ref`, `duplicate_id`, `symlink_write_refused`, and
`emacs_dirty_lock`.

## Configuration

The default file is `~/.config/agendum/config.toml` on Linux and the platform
configuration directory plus `agendum/config.toml` elsewhere. `--config`
selects another file; `--root-directory` replaces `files.roots` for one run.
Unknown fields are errors.

```toml
[files]
roots = ["~/org"]
agenda = ["**/*.org"]

[todo]
sequences = [["TODO", "NEXT", "WAIT", "|", "DONE", "CANCELLED"]]
ids = "preserve" # or "add-on-write"

[agenda]
span = "week"
week_start = "monday"
deadline_warning_days = 14
sorting_strategy = ["time-up", "priority-down", "category-keep"]

[capture.t]
desc = "Todo"
file = "inbox.org"
heading = "Inbox"
template = "* TODO %?\n:PROPERTIES:\n:CREATED: %U\n:END:"

[editor]
command = "nvim"
args = ["+{line}", "{file}"]

[agents.profiles.work]
files = ["work/**/*.org"]
match = "work"
```

Precedence is explicit: built-in defaults, imported cold snapshots, the manual
config file, then the `--root-directory` override. Imported snapshots may not
replace approved roots, editor commands, or import commands.

`editor.command` and each `editor.args` element are passed directly to the
process API. `{file}` and `{line}` are substituted without a shell. This keeps
paths with spaces intact and prevents Org text from becoming shell syntax.

## Editor coexistence and import

`agd` never needs an editor process for normal reads, agenda evaluation, or
writes. It notices external file changes in the TUI. Before a write it refuses
an Emacs dirty-buffer lock, takes its own sidecar lock, reloads the file, and
re-resolves the item ref. Use `e` in the TUI or `agd open <ref>` to hand the
heading back to the configured editor.

Configuration import runs an editor probe once and writes a versioned, cold
TOML snapshot:

```bash
agd import emacs --batch-profile vanilla --dry-run --verify
agd import emacs --batch-profile vanilla --verify
agd import nvim --profile orgmode --dry-run --verify
```

Daemon selection is explicit. Batch and Neovim profiles are executable plus
argv arrays; no shell is involved. Unsupported executable Lisp, hooks, or
Neovim callbacks remain disabled with diagnostics. See
[`docs/IMPORTING.md`](docs/IMPORTING.md) and
[`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md).

## MCP and coding agents

`agd mcp` is stdio only. It exposes six tools—`agenda`, `query`, `capture`,
`set_state`, `plan`, and `edit`—and the `agenda://today` resource. There is no
HTTP transport, remote service, telemetry, or implicit network access. Every
MCP write records the client identity and project directory in the local
journal for Agent review in the TUI.

Install project-local client configuration and the `agendum` skill with:

```bash
agd install --directory . claude
agd install --directory . codex
agd install --directory . opencode
```

Installers do not expose agenda content at session start. Claude's session
agenda is opt-in and requires a named filtered profile:

```bash
agd install --directory . claude --with-session-agenda work
```

## Safety and compatibility

- Reads follow symlinks only when the resolved target remains inside an
  approved root. Writes refuse every symlink in this release.
- Non-UTF-8 files, duplicate IDs, stale refs, dirty Emacs buffers, and ambiguous
  refile targets fail before mutation.
- Successful writes use same-directory temporary files, preserve mode and line
  endings, fsync the journal and file, then atomically rename.
- `undo` checks the recorded post-image hash; later edits produce a conflict,
  not data loss.
- Runtime operation is local. Only explicit import subprocesses and package or
  release installation use external programs or networks.

The complete boundary is in [`docs/SAFETY.md`](docs/SAFETY.md). Measured Emacs
agenda behavior and known unsupported Org forms are in
[`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md).

## Development

```bash
make build
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
cargo deny check
cargo audit
nix flake check
```

The workspace contains the released `agd` executable, the `agd-core` parsing
and write library in `org-core/`, deterministic fixtures in `test-utils/`, and
editor conformance probes under `conformance/` and `scripts/`.

## Origin and license

This fork preserves the history and copyright notice of
[`szaffarano/org-mcp-server`](https://github.com/szaffarano/org-mcp-server).
The combined distribution and new contributions are licensed under
GPL-3.0-or-later. Upstream-derived code remains available under its original
MIT license in [`LICENSE-MIT`](LICENSE-MIT).
