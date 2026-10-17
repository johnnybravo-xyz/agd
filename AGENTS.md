# Repository guidance

## Workspace boundaries

`agd` is a local-first Rust workspace for reading and safely changing Org files
from terminals and coding agents.

- `agd-core/` publishes `agd-core`. It owns parsing, discovery, agenda logic,
  search, capture, updates, and source-preserving behavior.
- `agd/` publishes the single `agd` executable. It owns command-line input,
  output, exit status, the TUI, and the stdio MCP adapter.
- `conformance/` contains Emacs-oracle, Neovim-oracle, and round-trip fixtures.

Put behavior in `agd-core`, then expose it through the thinnest necessary CLI,
TUI, or MCP adapter. Declare shared dependency versions in the root
`Cargo.toml`.

## Product invariants

- Org files are user data. Preserve every untouched byte exactly or refuse the
  write.
- Honor in-file Org syntax such as `#+TODO` and `#+SEQ_TODO`; never silently
  reinterpret a user's workflow.
- Detect ambiguous identity, especially duplicated `:ID:` values, before a
  targeted write.
- Keep default operation local. Production MCP uses stdio; do not add HTTP
  transports, remote services, telemetry, or implicit network access.
- Prefer deterministic ordering, stable machine-readable output, and explicit
  errors.
- Keep terminal workflows focused: responsive layouts, stock Ratatui widgets,
  keyboard-first interaction, `NO_COLOR` support, and exact write previews.

## Engineering style

Optimize for correctness and for code that a future maintainer can understand,
debug, and regenerate.

- Start from first principles and existing repository patterns.
- Prefer flat, explicit control flow and boring data types over abstraction,
  metaprogramming, or deep module trees.
- Keep shared utilities minimal; do not build a framework for one use case.
- Pass state explicitly. Avoid globals and hidden mutation.
- Use descriptive, simple names.
- Reserve comments for invariants, external constraints, and non-obvious safety
  decisions.
- Make errors actionable and preserve their source context.
- Follow platform and library conventions directly unless a wrapper has a
  concrete benefit.
- Make clean cutovers: update every caller and remove obsolete aliases,
  adapters, fixtures, instructions, and compatibility paths.
- Do not expand scope with speculative validation, retries, caching, or
  configurability.

## Implementation workflow

1. Read the relevant implementation, tests, configuration, and current product
   specification before editing.
2. Check references before changing an exported Rust symbol.
3. Add the smallest complete behavior in `agd-core`; keep transport code thin.
4. Test consumer-visible behavior and safety boundaries, not source text,
   forwarding, or mock echoes.
5. Exercise the actual changed CLI, TUI, or stdio scenario. Unit tests alone do
   not prove the surface works.
6. Remove temporary scripts and generated test data before committing.

Run the repository checks:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
nix flake check
```

For changes under `agd/claude-plugin/`, also run:

```bash
claude plugin validate --strict ./agd/claude-plugin
claude plugin test ./agd/claude-plugin
```

For CLI work, run the affected command against a temporary Org directory. For
write behavior, cover exact preservation, stale or ambiguous targets, and
failure without partial mutation.

## Documentation and releases

- Update `README.md` when installation or the public command surface changes.
- Update the affected crate's `CHANGELOG.md` for consumer-visible changes.
- Keep examples runnable and generic. Never commit workstation-specific paths
  or personal data.
- Keep `orgize` exactly pinned until source-preserving conformance proves an
  upgrade safe.
- Keep the Nix default package aligned with the single `agd` executable.
- Keep `agd/claude-plugin/.claude-plugin/plugin.json` at the same version as the
  `agd` crate; Claude Code uses it to update installed plugins.

## Git policy

- Sign commits with `-S`.
- Use concise commit messages without emoji or `Co-Authored-By` trailers.
- Normally commit completed, verified work in reviewable chunks; follow an
  explicit request for different grouping.
- Preserve unrelated user changes. Never reset, clean, force-push, or amend
  without explicit permission.
