# Parsing Org without taking ownership of the file

Most task applications own a database. An Org tool is a guest in a document
that also belongs to Emacs, Neovim, Git, shell scripts, and the person who typed
its odd spacing three years ago. That changes the parser's job. Producing the
right abstract syntax tree is necessary, but not sufficient: after changing one
TODO keyword, every unrelated byte must still belong to the user.

agd starts with a retained-source tree from `orgize`, pinned to an exact
release. The first experiment was deliberately boring. Copies of real Org files
were parsed and printed without edits, then compared byte for byte. The corpus
includes comments, drawers, custom TODO declarations, Unicode, planning lines,
inline markup, ranges, repeaters, and both LF and CRLF. A weekly CI run adds
every Org file in pinned checkouts of the Org and Worg repositories, and a
fuzz target adds arbitrary source around supported structures and checks that
parsing never creates an accidental rewrite.

The useful model is not "an Org heading." It is an index over the original
source. Each heading records the byte span of its heading line, title, TODO
keyword, priority, tags, planning timestamps, properties, body and complete
subtree. Reads can expose typed values, but writes are replacements of those
spans. A state change from `TODO` to `DONE` does not serialize a heading; it
replaces four bytes and, when configured, inserts a log entry where Org puts
it.

That distinction catches bugs that snapshot tests miss. A test that parses the
result and sees `DONE` will pass even if a serializer changed indentation,
reordered properties, normalized blank lines or converted CRLF. agd's write
fixtures instead compare the whole file with what Emacs writes, and every
byte outside a patch is copied exactly. The
parser can understand more syntax over time without gaining permission to
reformat it.

Identity needs the same restraint. A unique Org `:ID:` is stable and readable,
so agd uses it. A heading without one gets a ref made of its file, a line
hint and a digest of its heading line. The line speeds up the lookup but is
not identity: an unchanged heading may move. A changed heading makes the ref
stale. An ID that more than one heading has is an error, never a reason to
pick the first file a directory walk returns.

This model also made refusals simpler. A file that is not UTF-8 is reported,
not rewritten. A symlink is read only when it resolves inside a root; one
that leaves the roots is skipped and reported. Writes through symlinks are
refused, because safely replacing a link's target needs a different protocol
than replacing the link itself.

The result is less ambitious than a new Org implementation and more useful for
a tool that shares files: typed enough to answer agenda questions, exact
enough to return the file unchanged, and explicit whenever identity is
uncertain.
