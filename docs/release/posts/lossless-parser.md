# Parsing Org without taking ownership of the file

Most task applications own a database. An Org tool is a guest in a document
also used by Emacs, Neovim, Git, shell scripts, and the person who chose its
spacing years ago. A correct abstract syntax tree is therefore necessary but
insufficient: after changing one TODO keyword, every unrelated byte must still
belong to the user.

## Start with retained source

`agd` uses the retained-source tree from an exactly pinned `orgize` release.
The first validation step is intentionally simple: parse real Org files, print
them without edits, and compare every byte.

The corpus covers comments, drawers, custom TODO declarations, Unicode,
planning lines, inline markup, ranges, repeaters, and both LF and CRLF. A weekly
CI run adds every Org file from pinned Org and Worg checkouts. A fuzz target
surrounds supported structures with arbitrary source and verifies that parsing
does not create an accidental rewrite.

## Index spans instead of serializing nodes

The useful write model is not merely “an Org heading.” It is an index over the
original source. Each heading records byte spans for its heading line, title,
TODO keyword, priority, tags, planning timestamps, properties, body, and full
subtree.

Reads expose typed values. Writes replace those source spans. Changing `TODO`
to `DONE` replaces four bytes and, when configured, inserts a log entry where
Org places it. It does not serialize the heading.

This distinction catches failures that parsed snapshots miss. A test can parse
a rewritten file and find `DONE` even if a serializer also changed indentation,
reordered properties, normalized blank lines, or converted CRLF. `agd` instead
compares complete write fixtures with Emacs and copies every byte outside its
patches exactly.

The parser can understand more syntax over time without gaining permission to
reformat it.

## Identity is also source-sensitive

A unique Org `:ID:` is stable, so `agd` uses it directly. A heading without one
receives a ref made from its file, a line hint, and a digest of the heading line.
The line accelerates lookup but does not define identity: an unchanged heading
may move.

A changed heading makes the ref stale. An ID shared by multiple headings is an
error, never permission to choose the first path returned by directory
traversal.

## Refuse unsafe input

The same model keeps refusals clear:

- invalid UTF-8 is reported, not transcoded or rewritten;
- a symlink may be read only when it resolves inside a configured root;
- a symlink escaping the roots is skipped and reported;
- writes through symlinks are refused because replacing a link target safely
  requires a different protocol from replacing a regular file.

The result is intentionally narrower than a new Org implementation: typed
enough to answer agenda questions, exact enough to return source unchanged, and
explicit whenever identity or preservation cannot be guaranteed.
