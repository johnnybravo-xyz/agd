# AGENTS.md

## Scope

`agd` is a local-first Rust workspace for reading and safely changing org-mode
files from terminals and coding agents.

- `org-core/` publishes `agd-core` and owns parsing, discovery, agenda, search,
  capture, update, and source-preserving behavior.
- `agd/` publishes the single `agd` executable and owns command-line input,
  output, exit status, and the stdio MCP adapter.
- `test-utils/` contains deterministic shared fixtures.

Put behavior in `agd-core`, then expose it through the thinnest required CLI or
MCP adapter. Shared dependency versions belong in the root `Cargo.toml`.

## Product invariants

- Org files are user data. Preserve untouched bytes exactly or refuse the
  write.
- Honor in-file org syntax such as `#+TODO` and `#+SEQ_TODO`; do not silently
  reinterpret a user's workflow.
- Detect ambiguous identity, especially duplicate `:ID:` values, before a
  targeted write.
- Keep default operation local. The production MCP path is stdio; do not add
  HTTP transports, remote services, telemetry, or implicit network access.
- Prefer deterministic ordering, stable machine-readable output, and explicit
  errors.
- Treat future TUI work as a focused terminal workflow, not a dashboard:
  responsive layouts, stock Ratatui widgets, keyboard-first interaction,
  `NO_COLOR` support, and exact write previews.

## Working style

Optimize for correctness and for code that a future model can understand,
debug, and regenerate.

- Start from first principles and existing repository patterns.
- Prefer flat, explicit control flow and boring data types over abstraction,
  metaprogramming, or deep module trees.
- Keep shared utilities minimal. Do not create a framework for one use case.
- Pass state explicitly; avoid globals and hidden mutation.
- Use descriptive, simple names. Comments document invariants, external
  constraints, or non-obvious safety decisions only.
- Make errors actionable and preserve their source context.
- Follow platform and library conventions directly instead of wrapping them
  without a concrete benefit.
- Make clean cutovers: update every caller and remove obsolete aliases,
  adapters, fixtures, instructions, and compatibility paths.
- Do not expand scope with speculative validation, retries, caching, or
  configurability.

## Implementation workflow

1. Read the relevant implementation, tests, configuration, and current product
   specification before editing.
2. Check references before changing exported Rust symbols.
3. Add the smallest complete behavior in `agd-core`; keep transport code thin.
4. Test consumer-visible behavior and safety boundaries, not source text,
   forwarding, or mock echoes.
5. Run the actual changed CLI or stdio scenario. A passing unit test alone is
   not proof that the surface works.
6. Remove temporary scripts and generated test data before committing.

Use these repository checks:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
nix flake check
```

For CLI work, also run the affected command against a temporary org directory.
For write behavior, cover exact preservation, stale or ambiguous targets, and
failure without partial mutation.

## Documentation and releases

- Update `README.md` when the public command surface or installation changes.
- Update the affected crate's `CHANGELOG.md` for consumer-visible changes.
- Keep examples runnable and generic; never commit workstation-specific paths
  or personal data.
- Keep `orgize` exactly pinned until source-preserving conformance proves an
  upgrade safe.
- Keep the Nix default package aligned with the single `agd` executable.

## Git

- Sign commits with `-S`.
- Use concise commit messages without emojis or `Co-Authored-By` trailers.
- Normally commit completed, verified work in reviewable chunks; follow an
  explicit user request for a different grouping.
- Preserve unrelated user changes. Never reset, clean, force-push, or amend
  without an explicit request.
