# Keys

<!-- Generated from agd/src/tui/keys.rs, which drives agd's dispatch, footers and `?` help. Edit the tables there, then run `AGD_UPDATE_DOCS=1 cargo test -p agd keys_md`. -->

Every key of the terminal UI, screen by screen, beside org-agenda's key for the same thing where it has one. Press `?` in agd for the same list, filtered as you type.

Moving: j/k move like n/p everywhere; C-n/C-p or C-j/C-k move in pickers.

## Today · Week · Inbox · views

### Move

| Keys | Does | org-agenda |
| --- | --- | --- |
| `n` `j` `↓` | next row | `n` |
| `p` `k` `↑` | previous row | `p` |
| `PgDn` | ten rows down |  |
| `PgUp` | ten rows up |  |
| `Home` | first row |  |
| `End` | last row |  |

### Switch

| Keys | Does | org-agenda |
| --- | --- | --- |
| `tab` | next tab |  |
| `S-tab` | previous tab |  |
| `1–9` | tab by number, left to right |  |
| `c` | capture a task | `k` |
| `r` `g` | re-read every file | `r / g` |
| `?` | these keys |  |

### Show

| Keys | Does | org-agenda |
| --- | --- | --- |
| `f` | next day (week in Week) | `f` |
| `b` | previous day (week in Week) | `b` |
| `.` | back to today | `.` |
| `SPC` | peek at the entry | `SPC` |
| `a` | saved views | `C-c a` |
| `v` | classic org-agenda layout |  |
| `l` | log mode: closed items | `l` |
| `<` | filter by category, cycling | `<` |
| `/` | filter by text or tag | `/` |

### Change (marked rows, else the cursor row)

| Keys | Does | org-agenda |
| --- | --- | --- |
| `⏎` `e` | edit title, state, dates in a popup |  |
| `C-e` | open the heading in $EDITOR | `RET` |
| `t` | next TODO state | `t` |
| `x` | mark done |  |
| `X` | cancel: the file's last done state |  |
| `s` | schedule | `C-c C-s` |
| `d` | deadline | `C-c C-d` |
| `,` | set priority | `,` |
| `+` | raise priority | `+` |
| `-` | lower priority | `-` |
| `:` | add or remove tags | `:` |
| `S` | someday: tag it, drop the date |  |
| `R` `C-c C-w` | refile under another heading | `C-c C-w` |
| `u` | undo the last write | `C-_` |
| `y` | copy an Org link to the heading | `C-c l` |
| `m` | mark or unmark the row | `m` |
| `*` | mark every row shown | `*` |
| `U` | unmark every row | `U` |
| `B` | act on the marked rows, from a menu | `B` |

### Go

| Keys | Does | org-agenda |
| --- | --- | --- |
| `T` | triage overdue, then inbox |  |
| `A` | review agent changes |  |
| `esc` | close peek, clear marks or filter, go back |  |
| `q` | quit | `q` |

## Bulk menu · B with rows marked

### On every marked row

| Keys | Does | org-agenda |
| --- | --- | --- |
| `.` | schedule today |  |
| `>` | schedule tomorrow |  |
| `w` | schedule in a week |  |
| `s` | schedule on a date you type | `s` |
| `d` | deadline on a date you type | `d` |
| `S` | someday: tag it, drop the date |  |
| `x` | mark done |  |
| `X` | cancel: the file's last done state |  |
| `esc` `q` `B` | close the menu |  |

## Triage

### Decide

| Keys | Does | org-agenda |
| --- | --- | --- |
| `.` | schedule today |  |
| `>` | schedule tomorrow |  |
| `w` | schedule in a week |  |
| `s` | schedule on a date you type | `C-c C-s` |
| `d` | move a missed deadline | `C-c C-d` |
| `x` | mark done |  |
| `N` | make it NEXT |  |
| `S` | someday: tag it, drop the date |  |
| `X` | cancel: the file's last done state |  |
| `R` `C-c C-w` | refile under another heading | `C-c C-w` |
| `u` | undo a step; at the end, the whole pass | `C-_` |

### Cards

| Keys | Does | org-agenda |
| --- | --- | --- |
| `n` `j` `↓` | skip to the next card | `n` |
| `p` `k` `↑` | back to the previous card | `p` |
| `C-e` | open the heading in $EDITOR | `RET` |

### Switch

As on Today · Week · Inbox · views.

### Go

As on Today · Week · Inbox · views.

## Agents

### Review

| Keys | Does | org-agenda |
| --- | --- | --- |
| `a` | accept the marked changes, else this one |  |
| `m` | mark or unmark the change | `m` |
| `*` | mark every change shown | `*` |
| `U` | unmark every change | `U` |
| `u` | undo the change if its bytes are unchanged |  |
| `SPC` | peek: the change's diff, then its task | `SPC` |
| `⏎` `e` | edit the changed task in a popup |  |
| `C-e` | open the changed heading in $EDITOR | `RET` |
| `<` | show one agent's changes, cycling |  |

### Move

As on Today · Week · Inbox · views.

### Switch

As on Today · Week · Inbox · views.

### Go

As on Today · Week · Inbox · views.

## Pane

### Glance

| Keys | Does | org-agenda |
| --- | --- | --- |
| `n` `j` `↓` | next row |  |
| `p` `k` `↑` | previous row |  |
| `x` | mark done |  |
| `X` | cancel |  |
| `u` | undo the last write |  |
| `c` | capture a task |  |
| `C-e` | open the heading in $EDITOR |  |
| `⏎` | the full view, at 60+ columns |  |
| `T` | triage, at 60+ columns |  |
| `?` | these keys |  |
| `q` | quit |  |

## Pickers · views, templates, refile, edit fields

### Move in a picker

| Keys | Does | org-agenda |
| --- | --- | --- |
| `↓` `C-n` `C-j` | next choice or field |  |
| `↑` `C-p` `C-k` | previous choice or field |  |

## Popups · capture, edit, prompts, refile

### Type

| Keys | Does | org-agenda |
| --- | --- | --- |
| `M-⏎` `S-⏎` | in capture, add a checklist line |  |
| `⏎` | save what the preview shows |  |
| `x` `SPC` | check or uncheck a checklist line |  |
| `tab` | next field; in capture, the next template |  |
| `S-tab` | previous field |  |
| `esc` `C-g` | close without writing |  |
| `C-o` | the heading, or capture's body, in $EDITOR |  |
| `C-a` `Home` | start of the line |  |
| `C-e` `End` | end of the line |  |
| `C-d` `Delete` | delete the character at the cursor |  |
| `C-w` `M-DEL` | delete the word before the cursor |  |
| `C-u` | clear the line |  |
