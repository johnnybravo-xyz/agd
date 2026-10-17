# Security policy

`agd` reads and writes Org files and exposes the same operations to local coding
agents over stdio MCP. It does not open a network port.

Treat an issue as a security vulnerability when it could allow `agd` to:

- read or write outside configured roots;
- change Org structure or unrelated bytes without authorization;
- select an ambiguous target, including a duplicated `:ID:`;
- bypass write ownership or act as a different MCP client;
- disclose agenda data outside the local MCP session.

## Report a vulnerability

Use GitHub's private
[security advisory form](https://github.com/johnnybravo-xyz/agd/security/advisories/new).
Do not open a public issue.

Include:

- the output of `agd --version`;
- the CLI command or MCP request;
- the observed and expected behavior;
- a minimal Org file and configuration that reproduce the issue.

Remove personal task content before attaching files. You should receive an
initial response within one week. Confirmed fixes ship in the next release,
with credit in the changelog unless you prefer to remain anonymous.

## Supported versions

Security fixes are provided for the latest released version only.
