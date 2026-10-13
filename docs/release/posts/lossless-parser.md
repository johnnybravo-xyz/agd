# Parsing Org without taking ownership of the file

Most task applications own a database. An Org tool is a guest in a document
that also belongs to Emacs, Neovim, Git, shell scripts, and the person who typed
its odd spacing three years ago. That changes the parser's job. Producing the
right abstract syntax tree is necessary, but not sufficient: after changing one
TODO keyword, every unrelated byte must still belong to the user.

Agenda starts with a retained-source tree from `orgize`, pinned to an exact
release. The first experiment was deliberately boring. Copies of real Org files
were parsed and rendered without edits, then compared byte-for-byte. The corpus
includes comments, drawers, custom TODO declarations, Unicode, planning lines,
inline markup, ranges, repeaters, and both LF and CRLF. A bounded fuzz target
adds arbitrary source around supported structures and checks that parsing never
creates an accidental rewrite.

The useful model is not "an Org heading." It is an index over the original
source. Each heading records the byte span of its headline, title, TODO keyword,
priority, tags, planning timestamps, properties, body, and complete subtree.
Reads can expose typed values, but writes are expressed as replacements of
those spans. A state transition from `TODO` to `DONE` does not serialize a
heading; it replaces four bytes and, when configured, inserts a log entry at a
known gap.

That distinction catches bugs that snapshot tests miss. A test that parses the
result and sees `DONE` will pass even if a serializer changed indentation,
reordered properties, normalized blank lines, or converted CRLF. Agenda's
write fixtures instead assert the observable post-image and every untouched
range. The parser can understand more syntax over time without gaining
permission to reformat it.

Identity needs the same restraint. A unique Org `:ID:` is stable and readable,
so Agenda uses it. A heading without one gets a ref containing its file, a
line hint, and a digest of the original heading bytes. The line accelerates the
lookup but is not identity: an unchanged heading may move. A changed heading
makes the ref stale. Duplicate IDs are an error listing all candidates, never a
reason to pick the first file returned by a directory walk.

This model also made refusal behavior simpler. Non-UTF-8 input is reported, not
rewritten. Reads through a symlink are allowed only when canonicalization stays
inside an approved root. Writes through symlinks are refused in the first
release because safely replacing a root-contained target requires a different
protocol than replacing the link itself.

The result is less ambitious than a new Org implementation and more useful for
a coexistence tool: typed enough to answer agenda questions, exact enough to
return the file unchanged, and explicit whenever identity is uncertain.
