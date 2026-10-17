# agd

[![CI](https://github.com/johnnybravo-xyz/agd/actions/workflows/ci.yml/badge.svg)](https://github.com/johnnybravo-xyz/agd/actions/workflows/ci.yml)
[![agd on crates.io](https://img.shields.io/crates/v/agd.svg)](https://crates.io/crates/agd)
[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](https://github.com/johnnybravo-xyz/agd/blob/main/LICENSE)

**A local-first Org agenda for terminals and coding agents.**

`agd` reads the Org files and settings you already use with Emacs or
nvim-orgmode. It presents the same tasks through a keyboard-first TUI, a compact
pane, command-line output, and a local stdio MCP server. Org files remain the
source of truth: writes preserve untouched bytes, refuse ambiguous or stale
targets, and are journaled for guarded undo. Runtime operation requires no
network access.

![Claude Code captures into agd, then agd shows what you, Claude, and Codex added](https://raw.githubusercontent.com/johnnybravo-xyz/agd/main/docs/release/agd-story.gif)

## Why agd

- **Keep your existing workflow.** Import agenda files, TODO sequences, capture
  templates, logging rules, priorities, and custom agenda commands from Emacs
  or nvim-orgmode.
- **Use one task system everywhere.** Plan in the TUI, glance from tmux or
  herdr, script against the CLI, and let coding agents work through MCP.
- **Preserve the document.** Writes patch only the intended byte ranges. File
  mode, line endings, comments, drawers, and unrelated formatting remain
  unchanged.
- **Match Org deliberately.** Agenda output and supported edits are checked
  byte for byte against pinned Emacs 30.2 and Org 9.7 fixtures.
- **Stay local.** The MCP server uses stdio, opens no port, and sends agenda data
  only when a connected client calls a tool.

## Install

```bash
cargo install agd --locked                 # Rust 1.89+
brew install johnnybravo-xyz/agd/agd
nix run github:johnnybravo-xyz/agd
```

Other options:

- download a signed Linux, macOS, or Windows build from
  [GitHub Releases](https://github.com/johnnybravo-xyz/agd/releases/latest);
- build the [AUR package](https://github.com/johnnybravo-xyz/agd/tree/main/packaging/aur)
  on Arch Linux;
- generate completions with `agd completions bash|zsh|fish|elvish|powershell`.

## Quick start

### Existing Emacs or Neovim setup

```bash
agd setup import ~/.doom.d     # also ~/.config/emacs, ~/.emacs.d, or ~/.config/nvim
agd setup check
agd
```

The importer runs `emacs -Q` or `nvim --clean`, reports unsupported values, and
writes `~/.config/agd/config.toml` without running normal editor startup.

### New Org files

```bash
mkdir -p ~/org
agd capture "Renew passport due:fri #personal !A"
agd
```

No config is required. The default reads `.org` files under `~/org`, uses
`TODO NEXT WAIT | DONE CANCELLED`, and captures into an inbox.

To try isolated sample data instead:

```bash
git clone https://github.com/johnnybravo-xyz/agd.git
cd agd
cargo build --release
AGD=target/release/agd docs/demo/demo.sh
```

## Interfaces and agents

| Surface | Entry point | Purpose |
| --- | --- | --- |
| Full TUI | `agd` | Today, Week, Inbox, Agents, Triage, saved views |
| Narrow pane | `agd --pane` | Persistent tmux or herdr view |
| CLI | `agd agenda`, `agd capture`, `agd undo` | Scripts, status bars, capture, guarded undo |
| MCP | `agd mcp` | Local coding-agent access over stdio |
| Setup | `agd setup import`, `agd setup install`, `agd setup check` | Editor import, agent config, workspace diagnostics |

Install supported local agent configuration by detection:

```bash
agd setup install --dry-run --detected
agd setup install --detected
```

| Client | Setup |
| --- | --- |
| Codex CLI, ChatGPT desktop Codex, Codex IDE | `agd setup install codex` |
| Claude Code terminal, desktop local sessions, VS Code | `claude plugin marketplace add johnnybravo-xyz/agd`, then `claude plugin install agd@johnnybravo-xyz` |
| Paseo | Configure the provider it launches; Paseo reuses provider config and MCP servers |
| OpenCode, Pi, oh-my-pi | `agd setup install opencode`, `agd setup install pi`, or `agd setup install omp` |
| Claude Desktop chat or another stdio client | command `agd`, argument `mcp` |

`--detected` recognizes existing Codex, OpenCode, Pi, and oh-my-pi config
directories. The installer preserves unrelated configuration and refuses the
whole operation on a conflict. GUI clients may need the absolute `agd` path.

## Safety

Every interface uses the same read and write engines. Configured roots authorize
access. Writes resolve a fresh identity, refuse duplicate IDs, stale refs,
symlinks, and unsaved Emacs files, build byte-range patches, lock and re-read,
then journal and atomically replace. Multi-file writes are transactional;
`--dry-run` creates nothing; undo refuses changed or ambiguous bytes. MCP opens
no port and works only where the local process can execute `agd` and reach the
Org files.

Supported agenda and edit behavior is fixture-checked against pinned Emacs 30.2
and Org 9.7. Import fixtures also use pinned Neovim 0.12.6.

## Documentation

- [Getting started and complete user guide](https://github.com/johnnybravo-xyz/agd/blob/main/docs/getting-started.md)
- [Generated key reference](https://github.com/johnnybravo-xyz/agd/blob/main/docs/KEYS.md)
- [Architecture](https://github.com/johnnybravo-xyz/agd/blob/main/ARCHITECTURE.md)
- [Contributing](https://github.com/johnnybravo-xyz/agd/blob/main/CONTRIBUTING.md)
- [Security policy](https://github.com/johnnybravo-xyz/agd/blob/main/SECURITY.md)
- [CLI changelog](https://github.com/johnnybravo-xyz/agd/blob/main/agd/CHANGELOG.md)
- [Library changelog](https://github.com/johnnybravo-xyz/agd/blob/main/agd-core/CHANGELOG.md)

## Development and license

`just check` runs formatting, Clippy, workspace tests, Claude Code plugin checks,
and `nix flake check`. `just oracles` compares agd with pinned Emacs and Neovim
environments in Docker.

`agd` began as a fork of
[`szaffarano/org-mcp-server`](https://github.com/szaffarano/org-mcp-server).
The combined work is licensed under GPL-3.0-or-later. Upstream-derived code is
also available under the
[MIT license](https://github.com/johnnybravo-xyz/agd/blob/main/LICENSE-MIT).
