# Contributing to agd

Org files are user data: preserve untouched bytes exactly or refuse the write.
Read [ARCHITECTURE.md](ARCHITECTURE.md), [AGENTS.md](AGENTS.md), and the relevant
implementation, tests, configuration, and public guide before changing code.

## Environment

Rust 1.89 or later is required. The Nix shell contains the standard toolchain:

```bash
nix develop
```

Without Nix, install Rust with rustup and
[`just`](https://github.com/casey/just). On macOS, use `scripts/cargo`; it selects
Apple's clang and SDK, and the justfile and Makefile already call it.

Optional tools: Docker for pinned editor oracles, the `claude` CLI for plugin
checks, and `vhs` for recordings.

```bash
just                       # list commands
just build                 # build workspace targets
just run agenda --status   # run from this checkout
just install               # install under ~/.local/bin
just plugin-dev            # launch Claude Code with the local plugin
```

Use `docs/demo/demo.sh` or temporary `HOME`, `XDG_CONFIG_HOME`, and
`XDG_DATA_HOME` values. Do not point unfinished write code at personal Org files.

## Ownership and workflow

| Change | Primary location | Required follow-through |
| --- | --- | --- |
| Domain behavior | `agd-core/` | Public records, validation, safety tests, conformance when Org owns behavior |
| CLI command | `agd/src/commands/`, dispatched from `main.rs` | Stable output and real-command smoke run |
| TUI interaction | `agd/src/tui/` | Shared key meaning, rendered surface, resulting write |
| Configuration | `agd-core/src/config/` | Validation, rendering order, importer support, user guide |
| MCP tool | `agd/src/mcp/` | Strict fields, server instructions, stdio scenario |
| Error/schema | `agd-core/src/model.rs` | Stable snake_case code/schema and actionable prose |
| Keys/theme/words | `agd/src/tui/keys.rs`, `theme.rs`, `agd/src/words.rs` | Regenerate keys or keep every surface aligned |
| Agent adapter | `agd/claude-plugin/`, `agd/pi/`, setup installer | Package validation and actual provider/client smoke run |

Check references before changing an exported Rust symbol. Keep adapters thin and
state explicit. Test consumer-visible behavior and safety boundaries, then run
the changed CLI, TUI, or stdio path against isolated data. For writes, compare
the complete before/after bytes and prove refusal causes no partial mutation.
Remove temporary fixtures and generated output before committing.

## Required checks

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
nix flake check
```

Run focused checks while iterating. Additional scope checks:

```bash
just plugin    # agd/claude-plugin changes
just oracles   # agenda, writes, or editor-import semantics
```

`just oracles` requires Docker. CI covers Linux, macOS, Windows, and pinned
editor conformance.

## Documentation, releases, and commits

- Update `README.md` when installation or the public command surface changes.
- Keep detailed user behavior in `docs/getting-started.md`; keep retained
  landing and maintainer documents concise.
- Update the affected crate changelog for consumer-visible changes.
- Keep examples runnable, generic, and free of workstation paths or personal
  data.
- Keep the Claude Code plugin version equal to the `agd` crate version.
- Keep `orgize` exactly pinned until source-preserving conformance proves an
  upgrade safe.
- Use the Makefile's ordered release commands; run `make help` for details.
- Sign reviewable commits with `git commit -S`; use a concise subject without
  emoji or attribution trailers.

`agd` began as a product fork of `szaffarano/org-mcp-server`. Only generic
conformance or safety fixes may be offered upstream as small MIT-licensed
changes without GPL-only code. `agd` branding, product configuration, MCP/tool
selection, TUI workflows, imports, journal format, agent provenance, packaging,
and release work remain in this repository.

## Security reports

Do not open a public issue for a suspected vulnerability. Follow the private
process in [SECURITY.md](SECURITY.md).
