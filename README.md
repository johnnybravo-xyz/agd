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

Point `agd` at your Org files, or import the settings your editor already
uses, then check the files before any write:

```bash
mkdir -p ~/.config/agd
printf '[files]\nroots = ["~/org"]\n' > ~/.config/agd/config.toml
agd setup import emacs --dry-run --verify   # or: agd setup import nvim
agd setup check
agd
```

`agd` opens Today. `agd --pane` opens the narrow glance pane for a tmux or
herdr split, and `agd capture -i` opens only the capture popup, e.g. from
`tmux display-popup -E agd capture -i`.

## Commands

```text
agd                 Today, triage, capture and Agent review (full-screen TUI)
agd --pane          Narrow glance pane
agd agenda          The agenda as org-agenda prints it, or --json / --csv
agd capture         One line through a template, or the popup with -i
agd mcp             Stdio MCP server for coding agents
agd setup import    Emacs or nvim-orgmode settings into a snapshot
agd setup install   MCP server and skill for Claude Code, Codex, OpenCode
agd setup check     Parse drift, invalid UTF-8 and duplicate IDs
```

People change tasks in the TUI and agents change them through MCP; both use the
same guarded write path and journal.

```bash
agd agenda --span week
agd agenda --match 'work-someday/NEXT|TODO' --json
agd agenda --span day --csv            # same columns as org-batch-agenda-csv
agd capture t "Renew passport due:fri +personal #A" --dry-run
```

Capture text uses the TUI tokens: `due:` deadline, `on:` scheduled, `+tag`,
`#A`; dates take org relative syntax (`fri`, `+2d`, `2026-10-30`). JSON items
carry a stable `ref`: the heading `:ID:` when unique, otherwise file, line hint
and content digest. Errors use stable codes such as `stale_ref`,
`duplicate_id`, `symlink_write_refused` and `editor_dirty`.

## Configuration

The default file is `~/.config/agd/config.toml` on Linux and the platform
configuration directory plus `agd/config.toml` elsewhere. `--config`
selects another file; `--root-directory` replaces `files.roots` for one run.
Unknown fields are errors.

```toml
[files]
roots = ["~/org"]
agenda = ["**/*.org"]

[todo]
sequences = [["TODO", "NEXT", "WAIT", "|", "DONE", "CANCELLED"]]
ids = "preserve" # "add-on-write" adds a UUID :ID: on first edit

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

[agents]
capture_to = "template" # or "inbox": agent captures land unplanned in inbox_template
inbox_template = "t"

[agents.profiles.work]
files = ["work/**/*.org"]
match = "work"
```

Precedence is explicit: built-in defaults, imported cold snapshots, the manual
config file, then the `--root-directory` override. Imported snapshots may not
replace approved roots, editor commands, or import commands.

`editor.command` and each `editor.args` element are passed directly to the
process API. `{file}`, `{line}` and `{col}` are substituted without a shell,
so `args = ["-n", "+{line}:{col}", "{file}"]` works for `emacsclient`. This keeps
paths with spaces intact and prevents Org text from becoming shell syntax.

## Editor coexistence and import

`agd` never needs an editor process for normal reads, agenda evaluation, or
writes. It notices external file changes in the TUI. Before a write it refuses
an Emacs dirty-buffer lock, takes its own sidecar lock, reloads the file, and
re-resolves the item ref. Press `e` or Enter in the TUI to hand the heading
back to the configured editor.

Configuration import runs an editor probe once and writes a versioned, cold
TOML snapshot:

```bash
agd setup import emacs --server work --dry-run --verify
agd setup import emacs --batch-profile vanilla --verify
agd setup import nvim --profile orgmode --dry-run --verify
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
journal for Agent review in the TUI. With `agents.capture_to = "inbox"`, every
agent capture uses the `agents.inbox_template` template and drops its
`SCHEDULED`/`DEADLINE`, so agent work reaches your day only through triage.
CLI and TUI captures are never redirected.

Install project-local client configuration and the `agd` skill with:

```bash
agd setup install --directory . claude
agd setup install --directory . codex
agd setup install --directory . opencode
```

Installers do not expose agenda content at session start. Claude's session
agenda is opt-in and requires a named filtered profile:

```bash
agd setup install --directory . claude --with-session-agenda work
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
and write library in `org-core/`, Emacs-checked fixtures in `conformance/`, and
editor conformance probes under `conformance/` and `scripts/`.

## Origin and license

This fork preserves the history and copyright notice of
[`szaffarano/org-mcp-server`](https://github.com/szaffarano/org-mcp-server).
The combined distribution and new contributions are licensed under
GPL-3.0-or-later. Upstream-derived code remains available under its original
MIT license in [`LICENSE-MIT`](LICENSE-MIT).
