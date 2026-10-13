# Upstream conformance patch offering

Agenda is a product fork of `szaffarano/org-mcp-server`. Product-specific
configuration, TUI, transactional journal format, imports, and branding stay in
this fork. The following generic fixes should be offered upstream as small MIT
pull requests, preserving upstream style and public APIs where possible.

## Proposed patch series

### 1. Source-preservation regression harness

Scope:

- Add fixtures containing comments, property drawers, planning lines, CRLF,
  non-ASCII headings, and custom TODO declarations.
- Assert parse/no-op/serialize identity and unchanged byte ranges around a
  targeted field.
- Keep the fixture runner independent of Agenda configuration and journal code.

Why upstream: byte stability protects every consumer of the existing parser and
update functions. The tests can be MIT without moving Agenda's product code.

Acceptance: each fixture round-trips byte-for-byte, and a deliberately broad
line-reassembly mutation fails the untouched-range assertion.

### 2. Duplicate-ID diagnostics before targeted updates

Scope:

- Build an ID-to-locations index during discovery.
- Return an actionable ambiguity error listing every matching file and heading.
- Add one exact-ID success fixture and one duplicate-ID refusal fixture.

Why upstream: choosing the first duplicate can edit the wrong user document and
is independent of Agenda's item-ref format.

Acceptance: no file changes when an ID is duplicated.

### 3. Emacs agenda CSV fixture runner

Scope:

- Add a batch script around `org-batch-agenda-csv` with pinned date and agenda
  files.
- Normalize paths and line endings only; retain semantic fields.
- Seed past-scheduled and done-skip cases relevant to the upstream agenda.

Why upstream: the oracle is editor-owned behavior, not Agenda product logic.
It can expose compatibility regressions in the upstream CLI directly.

Acceptance: fixture output is deterministic in CI and documents the tested
Emacs/Org version.

### 4. Direct argv editor launch

Scope:

- Represent the editor as executable plus argv elements.
- Substitute file and line placeholders per argument.
- Use `Command` directly; remove shell command construction.

Why upstream: paths containing spaces and shell metacharacters should be safe in
any CLI.

Acceptance: a fixture editor records argv and receives the exact file path and
line as separate arguments.

## Excluded from the offer

Do not upstream the Agenda name, native config schema, MCP tool reduction,
Ratatui workflow, import snapshot formats, journal format, agent provenance,
release packaging, or roadmap. Those choices deliberately diverge from the
upstream server.

## Submission order

Offer the fixture harness first, then the smallest behavior fixes that make it
pass. Each pull request should state that the patch is offered under upstream's
MIT license, contain no GPL-only Agenda code, and avoid combining refactors
with behavior changes. Link back to the full Agenda compatibility table only
as evidence, not as a requirement that upstream adopt the fork.
