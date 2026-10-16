# Security

agd reads and writes your Org files and lets coding agents do the same over
a local stdio MCP server. It never opens a network port. A bug that lets
input escape the configured roots, write Org structure it was not asked to
write, change bytes it was not asked to change, or run as someone else's
agent is a security bug.

## Reporting a vulnerability

Report it privately through GitHub:
[Report a vulnerability](https://github.com/johnnybravo-xyz/agd/security/advisories/new).
Please don't open a public issue for it.

Include the agd version (`agd --version`), what you ran (the command, or
the MCP request), what happened and what you expected. A small Org file and
config that reproduce it help most; leave out your own tasks.

You'll get a reply within a week. Fixes ship in the next release, with
credit in the changelog unless you'd rather not be named.

## Supported versions

Security fixes go into the latest release only.
