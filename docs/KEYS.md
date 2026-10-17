# Keyboard reference

<!-- Generated from agd/src/tui/keys.rs, which drives dispatch, footers, and `?` help. Edit the tables there, then run `AGD_UPDATE_DOCS=1 cargo test -p agd keys_md`. -->

This is the complete TUI key reference, organized by screen. Org-agenda equivalents appear where they exist. Press `?` in `agd` to search the actions available on the current screen.

Navigation: j/k move like n/p everywhere; C-n/C-p or C-j/C-k move in pickers.

## Today · Week · Inbox · views

### Move

- `n` `j` `↓`: next row (org-agenda `n`)
- `p` `k` `↑`: previous row (org-agenda `p`)
- `PgDn`: ten rows down
- `PgUp`: ten rows up
- `Home`: first row
- `End`: last row

### Switch

- `tab`: next tab
- `S-tab`: previous tab
- `1–9`: tab by number, left to right
- `c`: capture a task (org-agenda `k`)
- `r` `g`: re-read every file (org-agenda `r` or `g`)
- `?`: these keys

### Show

- `f`: next day (week in Week) (org-agenda `f`)
- `b`: previous day (week in Week) (org-agenda `b`)
- `.`: back to today (org-agenda `.`)
- `SPC`: peek at the entry (org-agenda `SPC`)
- `a`: saved views (org-agenda `C-c a`)
- `v`: classic org-agenda layout
- `l`: log mode: closed items (org-agenda `l`)
- `<`: filter by category, cycling (org-agenda `<`)
- `/`: filter by text or tag (org-agenda `/`)

### Change (marked rows, else the cursor row)

- `⏎` `e`: edit title, state, dates in a popup
- `C-e`: open the heading in $EDITOR (org-agenda `RET`)
- `t`: next TODO state (org-agenda `t`)
- `x`: mark done
- `X`: cancel: the file's last done state
- `s`: schedule (org-agenda `C-c C-s`)
- `d`: deadline (org-agenda `C-c C-d`)
- `,`: set priority (org-agenda `,`)
- `+`: raise priority (org-agenda `+`)
- `-`: lower priority (org-agenda `-`)
- `:`: add or remove tags (org-agenda `:`)
- `S`: someday: tag it, drop the date
- `R` `C-c C-w`: refile under another heading (org-agenda `C-c C-w`)
- `u`: undo the last write (org-agenda `C-_`)
- `y`: copy an Org link to the heading (org-agenda `C-c l`)
- `m`: mark or unmark the row (org-agenda `m`)
- `*`: mark every row shown (org-agenda `*`)
- `U`: unmark every row (org-agenda `U`)
- `B`: act on the marked rows, from a menu (org-agenda `B`)

### Go

- `T`: triage overdue, then inbox
- `A`: review agent changes
- `esc`: close peek, clear marks or filter, go back
- `q`: quit (org-agenda `q`)

## Bulk menu · B with rows marked

### On every marked row

- `.`: schedule today
- `>`: schedule tomorrow
- `w`: schedule in a week
- `s`: schedule on a date you type (org-agenda `s`)
- `d`: deadline on a date you type (org-agenda `d`)
- `S`: someday: tag it, drop the date
- `x`: mark done
- `X`: cancel: the file's last done state
- `esc` `q` `B`: close the menu

## Triage

### Decide

- `.`: schedule today
- `>`: schedule tomorrow
- `w`: schedule in a week
- `s`: schedule on a date you type (org-agenda `C-c C-s`)
- `d`: move a missed deadline (org-agenda `C-c C-d`)
- `x`: mark done
- `N`: make it NEXT
- `S`: someday: tag it, drop the date
- `X`: cancel: the file's last done state
- `R` `C-c C-w`: refile under another heading (org-agenda `C-c C-w`)
- `u`: undo a step; at the end, the whole pass (org-agenda `C-_`)

### Cards

- `n` `j` `↓`: skip to the next card (org-agenda `n`)
- `p` `k` `↑`: back to the previous card (org-agenda `p`)
- `C-e`: open the heading in $EDITOR (org-agenda `RET`)

### Switch

As on Today · Week · Inbox · views.

### Go

As on Today · Week · Inbox · views.

## Agents

### Review

- `a`: accept the marked changes, else this one
- `m`: mark or unmark the change (org-agenda `m`)
- `*`: mark every change shown (org-agenda `*`)
- `U`: unmark every change (org-agenda `U`)
- `u`: undo the change if its bytes are unchanged
- `SPC`: peek: the change's diff, then its task (org-agenda `SPC`)
- `⏎` `e`: edit the changed task in a popup
- `C-e`: open the changed heading in $EDITOR (org-agenda `RET`)
- `<`: show one agent's changes, cycling

### Move

As on Today · Week · Inbox · views.

### Switch

As on Today · Week · Inbox · views.

### Go

As on Today · Week · Inbox · views.

## Pane

### Glance

- `n` `j` `↓`: next row
- `p` `k` `↑`: previous row
- `x`: mark done
- `X`: cancel
- `u`: undo the last write
- `c`: capture a task
- `C-e`: open the heading in $EDITOR
- `⏎`: the full view, at 60+ columns
- `T`: triage, at 60+ columns
- `?`: these keys
- `q`: quit

## Pickers · views, templates, refile, edit fields

### Move in a picker

- `↓` `C-n` `C-j`: next choice or field
- `↑` `C-p` `C-k`: previous choice or field

## Popups · capture, edit, prompts, refile

### Type

- `M-⏎` `S-⏎`: in capture, add a checklist line
- `⏎`: save what the preview shows
- `x` `SPC`: check or uncheck a checklist line
- `H` `←`: previous State or Priority choice
- `L` `→`: next State or Priority choice
- `tab`: next field; in capture, the next template
- `S-tab`: previous field
- `esc` `C-g`: close without writing
- `C-o`: the heading, or capture's body, in $EDITOR
- `C-a` `Home`: start of the line
- `C-e` `End`: end of the line
- `C-d` `Delete`: delete the character at the cursor
- `C-w` `M-DEL`: delete the word before the cursor
- `C-u`: clear the line
