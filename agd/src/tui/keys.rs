//! Every key binding, defined once. The tables below drive dispatch, the
//! footers, Triage's chips and the `?` help, so they cannot drift apart.
//!
//! Key rules, so that no key surprises:
//! - A key means the same on every screen that has it: `n j ↓` next and
//!   `p k ↑` previous (rows, cards, changes), `s` schedule on a typed
//!   date, `d` deadline, `S` someday, `x` done, `X` cancel, `R` or
//!   `C-c C-w` refile, `u` undo, `c` capture, `?` keys, `esc` back one
//!   level.
//! - Keys follow org-agenda wherever it has one (`t`, `C-c C-s`, `f b .`,
//!   `l`, `<`, `/`, `m * U B` for marks); the help shows its key beside
//!   ours.
//! - Triage and the bulk menu add one-key outcomes: `.` today (in the
//!   lists `.` goes to today), `>` tomorrow, `w` next week, `N` make NEXT.
//!   `m` only ever marks.
//! - Where letters are typed or name items (the Views popup, capture's
//!   templates, the refile picker, `?`'s filter, the edit popup's fields),
//!   only `↓ C-n C-j` and `↑ C-p C-k` move, as in fzf and Emacs.
//! - Footers show a key's first label; help shows every label.
//! - One notation: Emacs's for chords (`C-e`, `C-c C-w`, `S-tab`, `M-DEL`),
//!   words or glyphs for named keys (`⏎`, `esc`, `tab`, `SPC`, `PgDn`, `↓`).

use std::io::Stdout;
use std::time::Instant;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use super::actions::Prompt;
use super::input::Edit;
use super::{App, Overlay, PromptKind, Screen, capture, help, open_in_editor};

type Term = Terminal<CrosstermBackend<Stdout>>;

/// A file and line a key asks to open in $EDITOR.
pub(super) type EditorTarget = Option<(String, usize)>;

/// A key as a binding names it.
#[derive(Clone, Copy, Debug)]
pub(super) enum Key {
    /// A character typed without ctrl or alt; shift is in the character.
    Char(char),
    /// A named key: arrows, tab, enter, esc, the page keys.
    Code(KeyCode),
    /// ctrl and a letter.
    Ctrl(char),
    /// 1 to 9, for the tabs.
    Digit,
}

impl Key {
    fn matches(self, key: KeyEvent) -> bool {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match self {
            Key::Char(character) => !control && !alt && key.code == KeyCode::Char(character),
            Key::Code(code) => key.code == code,
            Key::Ctrl(character) => control && key.code == KeyCode::Char(character),
            Key::Digit => !control && matches!(key.code, KeyCode::Char('1'..='9')),
        }
    }
}

/// Everything a key can do. Each screen's table maps its keys to these, and
/// that screen's `run_*` function carries them out.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Action {
    Next,
    Previous,
    PageDown,
    PageUp,
    First,
    Last,
    NextTab,
    PreviousTab,
    TabNumber,
    Capture,
    Refresh,
    Help,
    Later,
    Earlier,
    GoToday,
    Peek,
    Views,
    Classic,
    LogMode,
    Context,
    Filter,
    Edit,
    Editor,
    State,
    Done,
    Cancel,
    Schedule,
    Deadline,
    Priority,
    PriorityUp,
    PriorityDown,
    Tags,
    Someday,
    Refile,
    Undo,
    CopyLink,
    Mark,
    MarkAll,
    UnmarkAll,
    Bulk,
    Triage,
    Agents,
    Back,
    Quit,
    Today,
    Tomorrow,
    NextWeek,
    MakeNext,
    Accept,
    Diff,
    FilterAgent,
    FullView,
}

/// One binding: the keys, what they do, and how footers and help name it.
pub(super) struct Binding {
    /// The keys as help prints them; footers print only the first.
    pub label: &'static str,
    keys: &'static [Key],
    pub action: Action,
    /// One or two words for footers and Triage's chips.
    pub short: &'static str,
    /// What it does, for help.
    pub help: &'static str,
    /// org-agenda's key for the same thing, if it has one.
    pub org: &'static str,
}

/// Bindings under one help heading.
pub(super) struct KeyGroup {
    pub title: &'static str,
    pub bindings: &'static [Binding],
}

/// A binding; the argument order matches the table columns below.
const fn bind(
    label: &'static str,
    keys: &'static [Key],
    action: Action,
    short: &'static str,
    help: &'static str,
    org: &'static str,
) -> Binding {
    Binding {
        label,
        keys,
        action,
        short,
        help,
        org,
    }
}

use Action as A;
use Key::{Char, Code, Ctrl, Digit};

// The tables are kept one binding per line, as a table reads.
// Columns: label, keys, action, footer word, help text, org-agenda key.

/// Moving and switching, shared by the lists and the Agents tab.
#[rustfmt::skip]
const MOVE: KeyGroup = KeyGroup {
    title: "Move and switch",
    bindings: &[
        bind("n j ↓", &[Char('n'), Char('j'), Code(KeyCode::Down)], A::Next, "next", "next row", "n"),
        bind("p k ↑", &[Char('p'), Char('k'), Code(KeyCode::Up)], A::Previous, "previous", "previous row", "p"),
        bind("PgDn", &[Code(KeyCode::PageDown)], A::PageDown, "page", "ten rows down", ""),
        bind("PgUp", &[Code(KeyCode::PageUp)], A::PageUp, "page", "ten rows up", ""),
        bind("Home", &[Code(KeyCode::Home)], A::First, "first", "first row", ""),
        bind("End", &[Code(KeyCode::End)], A::Last, "last", "last row", ""),
        bind("tab", &[Code(KeyCode::Tab)], A::NextTab, "tab", "next tab", ""),
        bind("S-tab", &[Code(KeyCode::BackTab)], A::PreviousTab, "tab", "previous tab", ""),
        bind("1–9", &[Digit], A::TabNumber, "tab", "tab by number, left to right", ""),
        bind("c", &[Char('c')], A::Capture, "capture", "capture a task", "k"),
        bind("r g", &[Char('r'), Char('g')], A::Refresh, "refresh", "re-read every file", "r / g"),
        bind("?", &[Char('?')], A::Help, "help", "these keys", ""),
    ],
};

/// What the lists show.
#[rustfmt::skip]
const SHOW: KeyGroup = KeyGroup {
    title: "Show",
    bindings: &[
        bind("f", &[Char('f')], A::Later, "later", "next day (week in Week)", "f"),
        bind("b", &[Char('b')], A::Earlier, "earlier", "previous day (week in Week)", "b"),
        bind(".", &[Char('.')], A::GoToday, "today", "back to today", "."),
        bind("SPC", &[Char(' ')], A::Peek, "peek", "peek at the entry", "SPC"),
        bind("a", &[Char('a')], A::Views, "views", "saved views", "C-c a"),
        bind("v", &[Char('v')], A::Classic, "classic", "classic org-agenda layout", ""),
        bind("l", &[Char('l')], A::LogMode, "log", "log mode: closed items", "l"),
        bind("<", &[Char('<')], A::Context, "context", "filter by category, cycling", "<"),
        bind("/", &[Char('/')], A::Filter, "filter", "filter by text or tag", "/"),
    ],
};

/// Writes; with rows marked, they act on every marked row.
#[rustfmt::skip]
const CHANGE: KeyGroup = KeyGroup {
    title: "Change (marked rows, else the cursor row)",
    bindings: &[
        bind("⏎ e", &[Code(KeyCode::Enter), Char('e')], A::Edit, "edit", "edit title, state, dates in a popup", ""),
        bind("C-e", &[Ctrl('e')], A::Editor, "$EDITOR", "open the heading in $EDITOR", "RET"),
        bind("t", &[Char('t')], A::State, "state", "next TODO state", "t"),
        bind("x", &[Char('x')], A::Done, "done", "mark done", ""),
        bind("X", &[Char('X')], A::Cancel, "cancel", "cancel: the file's last done state", ""),
        bind("s", &[Char('s')], A::Schedule, "schedule", "schedule", "C-c C-s"),
        bind("d", &[Char('d')], A::Deadline, "deadline", "deadline", "C-c C-d"),
        bind(",", &[Char(',')], A::Priority, "priority", "set priority", ","),
        bind("+", &[Char('+')], A::PriorityUp, "priority", "raise priority", "+"),
        bind("-", &[Char('-')], A::PriorityDown, "priority", "lower priority", "-"),
        bind(":", &[Char(':')], A::Tags, "tags", "add or remove tags", ":"),
        bind("S", &[Char('S')], A::Someday, "someday", "someday: tag it, drop the date", ""),
        // C-c C-w is a chord, which `chord_key` handles.
        bind("R C-c C-w", &[Char('R')], A::Refile, "refile", "refile under another heading", "C-c C-w"),
        bind("u", &[Char('u')], A::Undo, "undo", "undo the last write", "C-_"),
        bind("y", &[Char('y')], A::CopyLink, "copy link", "copy the entry's link", ""),
        bind("m", &[Char('m')], A::Mark, "mark", "mark or unmark the row", "m"),
        bind("*", &[Char('*')], A::MarkAll, "mark all", "mark every row shown", "*"),
        bind("U", &[Char('U')], A::UnmarkAll, "unmark all", "unmark every row", "U"),
        bind("B", &[Char('B')], A::Bulk, "bulk", "act on the marked rows, from a menu", "B"),
    ],
};

/// Leaving the lists.
#[rustfmt::skip]
const GO: KeyGroup = KeyGroup {
    title: "Go",
    bindings: &[
        bind("T", &[Char('T')], A::Triage, "triage", "triage overdue, then inbox", ""),
        bind("A", &[Char('A')], A::Agents, "agents", "review agent changes", ""),
        bind("esc", &[Code(KeyCode::Esc)], A::Back, "back", "close peek, clear marks or filter, leave a view", ""),
        bind("q", &[Char('q')], A::Quit, "quit", "quit", "q"),
    ],
};

/// Today, Week, Inbox and saved views.
pub(super) const LIST_KEYS: &[KeyGroup] = &[MOVE, SHOW, CHANGE, GO];

/// One key per decision on the current card.
#[rustfmt::skip]
pub(super) const TRIAGE_KEYS: &[KeyGroup] = &[KeyGroup {
    title: "Decide",
    bindings: &[
        bind(".", &[Char('.')], A::Today, "today", "schedule today", ""),
        bind(">", &[Char('>')], A::Tomorrow, "tomorrow", "schedule tomorrow", ""),
        bind("w", &[Char('w')], A::NextWeek, "next week", "schedule in a week", ""),
        bind("s", &[Char('s')], A::Schedule, "pick date", "schedule on a date you type", "C-c C-s"),
        bind("d", &[Char('d')], A::Deadline, "move deadline", "move a missed deadline", "C-c C-d"),
        bind("x", &[Char('x')], A::Done, "done", "mark done", ""),
        bind("N", &[Char('N')], A::MakeNext, "make NEXT", "make it NEXT", ""),
        bind("S", &[Char('S')], A::Someday, "someday", "someday: tag it, drop the date", ""),
        bind("X", &[Char('X')], A::Cancel, "cancel", "cancel: the file's last done state", ""),
        bind("R C-c C-w", &[Char('R')], A::Refile, "refile", "refile under another heading", "C-c C-w"),
        bind("u", &[Char('u')], A::Undo, "undo", "undo a step; at the end, the whole pass", "C-_"),
        bind("n j ↓ tab", &[Char('n'), Char('j'), Code(KeyCode::Down), Code(KeyCode::Tab)], A::Next, "next card", "skip to the next card", "n"),
        bind("p k ↑ S-tab", &[Char('p'), Char('k'), Code(KeyCode::Up), Code(KeyCode::BackTab)], A::Previous, "previous card", "back to the previous card", "p"),
        bind("esc q", &[Code(KeyCode::Esc), Char('q')], A::Back, "back", "leave triage", ""),
        bind("?", &[Char('?')], A::Help, "help", "these keys", ""),
    ],
}];

/// The Agents tab: judge each agent write.
#[rustfmt::skip]
pub(super) const AGENT_KEYS: &[KeyGroup] = &[
    KeyGroup {
        title: "Review",
        bindings: &[
            bind("a", &[Char('a')], A::Accept, "accept", "accept the marked changes, else this one", ""),
            bind("m", &[Char('m')], A::Mark, "mark", "mark or unmark the change", "m"),
            bind("*", &[Char('*')], A::MarkAll, "mark all", "mark every change shown", "*"),
            bind("U", &[Char('U')], A::UnmarkAll, "unmark all", "unmark every change", "U"),
            bind("u", &[Char('u')], A::Undo, "undo", "undo the change if its bytes are unchanged", ""),
            bind("d", &[Char('d')], A::Diff, "diff", "show the raw diff", ""),
            bind("SPC", &[Char(' ')], A::Peek, "peek", "peek at the changed task", ""),
            bind("⏎ C-e", &[Code(KeyCode::Enter), Ctrl('e')], A::Editor, "open", "open the changed heading in $EDITOR", ""),
            bind("f", &[Char('f')], A::FilterAgent, "filter by agent", "show one agent, cycling", ""),
            bind("esc q", &[Code(KeyCode::Esc), Char('q')], A::Back, "back", "close the diff or peek, clear marks, else back", ""),
        ],
    },
    MOVE,
];

/// `B` with rows marked: org-agenda's bulk menu, one key per action.
#[rustfmt::skip]
pub(super) const BULK_KEYS: &[KeyGroup] = &[KeyGroup {
    title: "On every marked row",
    bindings: &[
        bind(".", &[Char('.')], A::Today, "today", "schedule today", ""),
        bind(">", &[Char('>')], A::Tomorrow, "tomorrow", "schedule tomorrow", ""),
        bind("w", &[Char('w')], A::NextWeek, "next week", "schedule in a week", ""),
        bind("s", &[Char('s')], A::Schedule, "pick date", "schedule on a date you type", "s"),
        bind("d", &[Char('d')], A::Deadline, "deadline…", "deadline on a date you type", "d"),
        bind("S", &[Char('S')], A::Someday, "someday", "someday: tag it, drop the date", ""),
        bind("x", &[Char('x')], A::Done, "done", "mark done", ""),
        bind("X", &[Char('X')], A::Cancel, "cancel", "cancel: the file's last done state", ""),
        bind("esc q B", &[Code(KeyCode::Esc), Char('q'), Char('B')], A::Back, "close", "close the menu", ""),
    ],
}];

/// The narrow pane beside an agent or editor.
#[rustfmt::skip]
pub(super) const PANE_KEYS: &[KeyGroup] = &[KeyGroup {
    title: "Glance",
    bindings: &[
        bind("n j ↓", &[Char('n'), Char('j'), Code(KeyCode::Down)], A::Next, "next", "next row", ""),
        bind("p k ↑", &[Char('p'), Char('k'), Code(KeyCode::Up)], A::Previous, "previous", "previous row", ""),
        bind("x", &[Char('x')], A::Done, "done", "mark done", ""),
        bind("X", &[Char('X')], A::Cancel, "cancel", "cancel", ""),
        bind("u", &[Char('u')], A::Undo, "undo", "undo the last write", ""),
        bind("c", &[Char('c')], A::Capture, "capture", "capture a task", ""),
        bind("C-e", &[Ctrl('e')], A::Editor, "$EDITOR", "open the heading in $EDITOR", ""),
        bind("⏎", &[Code(KeyCode::Enter)], A::FullView, "full view", "the full view, at 60+ columns", ""),
        bind("T", &[Char('T')], A::Triage, "triage", "triage, at 60+ columns", ""),
        bind("?", &[Char('?')], A::Help, "help", "these keys", ""),
        bind("q", &[Char('q')], A::Quit, "quit", "quit", ""),
    ],
}];

/// Moving where letters are typed or name items: the Views popup,
/// capture's templates and fields, the refile picker, `?`'s filter and the
/// edit popup's fields. Letters there are text or keys, so only the arrows
/// and these ctrl keys move.
#[rustfmt::skip]
pub(super) const PICKER_KEYS: &[KeyGroup] = &[KeyGroup {
    title: "Move",
    bindings: &[
        bind("↓ C-n C-j", &[Code(KeyCode::Down), Ctrl('n'), Ctrl('j')], A::Next, "next", "next choice or field", ""),
        bind("↑ C-p C-k", &[Code(KeyCode::Up), Ctrl('p'), Ctrl('k')], A::Previous, "previous", "previous choice or field", ""),
    ],
}];

/// Help's sections, one per screen: (title, keys).
pub(super) const SCREENS: [(&str, &[KeyGroup]); 6] = [
    ("Today · Week · Inbox · views", LIST_KEYS),
    ("Bulk menu · B with rows marked", BULK_KEYS),
    ("Triage", TRIAGE_KEYS),
    ("Agents", AGENT_KEYS),
    ("Pane", PANE_KEYS),
    (
        "Pickers · views, templates, refile, edit fields",
        PICKER_KEYS,
    ),
];

/// One line under help's keys: how moving works everywhere.
pub(super) const MOVING: &str = "j/k move like n/p everywhere; C-n/C-p or C-j/C-k move in pickers";

/// +1 or -1 when `key` moves in a picker (PICKER_KEYS), else None.
pub(super) fn picker_step(key: KeyEvent) -> Option<isize> {
    match action_for(PICKER_KEYS, key)? {
        A::Next => Some(1),
        A::Previous => Some(-1),
        _ => None,
    }
}

/// `index` moved by `step` within `count` choices, stopping at the ends.
pub(super) fn stepped(index: usize, step: Option<isize>, count: usize) -> usize {
    let last = count.saturating_sub(1) as isize;
    (index as isize + step.unwrap_or(0)).clamp(0, last) as usize
}

/// Triage's two rows of decision chips under a card: an inbox card leads
/// with refiling it, a missed deadline with moving the deadline.
pub(super) fn triage_chips(row: &super::DisplayRow) -> [&'static [Action]; 2] {
    let missed_deadline = row.occurrence.as_ref().is_some_and(|occurrence| {
        occurrence.kind == agd_core::OccurrenceType::Deadline && occurrence.extra.ends_with("ago:")
    });
    if row.section == super::INBOX {
        [
            &[A::Refile, A::Today, A::Tomorrow, A::NextWeek, A::Schedule],
            &[A::Done, A::MakeNext, A::Someday, A::Cancel],
        ]
    } else if missed_deadline {
        [
            &[A::Deadline, A::Today, A::Tomorrow, A::NextWeek, A::Done],
            &[A::Schedule, A::MakeNext, A::Someday, A::Cancel, A::Refile],
        ]
    } else {
        [
            &[A::Today, A::Tomorrow, A::NextWeek, A::Schedule, A::Done],
            &[A::MakeNext, A::Someday, A::Cancel, A::Refile],
        ]
    }
}

/// A footer hint: an action and the word for it in this context.
type Hint = (Action, &'static str);

/// The footer's hints for what is under the cursor, four at most, as
/// (keys, word); `? keys` is drawn after them. `f/b` stands for both.
pub(super) fn footer_hints(app: &App) -> Vec<(String, &'static str)> {
    let (groups, hints): (&'static [KeyGroup], &[Hint]) = match app.screen {
        Screen::Triage => (
            TRIAGE_KEYS,
            &[
                (A::Next, "next card"),
                (A::Previous, "previous card"),
                (A::Undo, "undo"),
                (A::Back, "back"),
            ],
        ),
        Screen::Review if !app.marked_changes.is_empty() => (
            AGENT_KEYS,
            &[(A::Accept, "accept marked"), (A::UnmarkAll, "unmark all")],
        ),
        Screen::Review => (
            AGENT_KEYS,
            &[
                (A::Accept, "accept"),
                (A::Mark, "mark"),
                (A::Undo, "undo"),
                (A::Diff, "diff"),
            ],
        ),
        _ if !app.marks.is_empty() => (
            LIST_KEYS,
            &[
                (A::Bulk, "bulk"),
                (A::Done, "done"),
                (A::Schedule, "schedule"),
                (A::UnmarkAll, "unmark all"),
            ],
        ),
        _ => (LIST_KEYS, row_hints(app)),
    };
    let key = |action: Action| {
        groups
            .iter()
            .flat_map(|group| group.bindings.iter())
            .find(|binding| binding.action == action)
            .map(|binding| binding.label.split(' ').next().unwrap_or(binding.label))
            .unwrap_or("")
    };
    hints
        .iter()
        .map(|(action, word)| match action {
            A::Later => (format!("{}/{}", key(A::Later), key(A::Earlier)), *word),
            _ => (key(*action).to_string(), *word),
        })
        .collect()
}

/// What the selected row most likely needs next.
fn row_hints(app: &App) -> &'static [Hint] {
    let Some(row) = app.selected_row() else {
        return &[(A::Capture, "capture"), (A::Views, "views")];
    };
    let overdue = row.occurrence.as_ref().is_some_and(|occurrence| {
        occurrence.kind == agd_core::OccurrenceType::PastScheduled
            || occurrence.extra.ends_with("ago:")
    });
    if row.item.done {
        &[(A::State, "reopen"), (A::Undo, "undo")]
    } else if overdue {
        &[
            (A::Schedule, "reschedule"),
            (A::Done, "done"),
            (A::Triage, "triage"),
        ]
    } else if row.section == super::INBOX {
        &[(A::Refile, "refile"), (A::Done, "done")]
    } else if app.screen == Screen::Week {
        &[
            (A::Done, "done"),
            (A::Schedule, "schedule"),
            (A::Later, "week"),
            (A::GoToday, "today"),
        ]
    } else {
        &[
            (A::Done, "done"),
            (A::State, "state"),
            (A::Schedule, "schedule"),
            (A::Deadline, "deadline"),
        ]
    }
}

/// Index into SCREENS of what is on view.
pub(super) fn current_screen(app: &App) -> usize {
    if app.pane {
        return 4;
    }
    match app.screen {
        Screen::Triage => 2,
        Screen::Review => 3,
        _ => 0,
    }
}

/// (first key, footer word) for each action, as footers and chips show them.
pub(super) fn hints(
    groups: &'static [KeyGroup],
    actions: &[Action],
) -> Vec<(&'static str, &'static str)> {
    actions
        .iter()
        .filter_map(|action| {
            groups
                .iter()
                .flat_map(|group| group.bindings.iter())
                .find(|binding| binding.action == *action)
        })
        .map(|binding| {
            let key = binding.label.split(' ').next().unwrap_or(binding.label);
            (key, binding.short)
        })
        .collect()
}

fn action_for(groups: &[KeyGroup], key: KeyEvent) -> Option<Action> {
    groups
        .iter()
        .flat_map(|group| group.bindings.iter())
        .find(|binding| binding.keys.iter().any(|candidate| candidate.matches(key)))
        .map(|binding| binding.action)
}

/// Handles one key, running $EDITOR when it asks for it.
pub(super) fn handle_key(terminal: &mut Term, app: &mut App, key: KeyEvent) -> Result<()> {
    if matches!(app.overlay, Overlay::Capture(_)) && !is_quit_key(key) {
        // The capture popup runs $EDITOR for the body itself.
        return capture::handle_key(terminal, app, key);
    }
    if let Some((file, line)) = dispatch(app, key)? {
        open_in_editor(terminal, app, &file, line)?;
    }
    Ok(())
}

fn is_quit_key(key: KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c')
}

/// Every key outside the capture popup, without a terminal, so tests can
/// press keys; returns where to open $EDITOR when the key asks for it.
pub(super) fn dispatch(app: &mut App, key: KeyEvent) -> Result<EditorTarget> {
    if key.kind == KeyEventKind::Release || chord_key(app, key) {
        return Ok(None);
    }
    match &app.overlay {
        Overlay::None => {}
        Overlay::Help(_) => return help_key(app, key),
        Overlay::Capture(_) => return Ok(None),
        Overlay::Edit(_) => {
            let editor = super::edit::handle_key(app, key)?;
            return Ok(if editor { selected_location(app) } else { None });
        }
        _ => {
            handle_overlay(app, key)?;
            return Ok(None);
        }
    }
    if app.pane {
        let Some(action) = action_for(PANE_KEYS, key) else {
            return Ok(None);
        };
        return run_pane(app, action);
    }
    match app.screen {
        Screen::Triage => {
            if app.rows.is_empty() {
                return finished_triage_key(app, key).map(|()| None);
            }
            let Some(action) = action_for(TRIAGE_KEYS, key) else {
                return Ok(None);
            };
            run_triage(app, action).map(|()| None)
        }
        Screen::Review => {
            let Some(action) = action_for(AGENT_KEYS, key) else {
                return Ok(None);
            };
            run_agents(app, action, key)
        }
        _ => {
            let Some(action) = action_for(LIST_KEYS, key) else {
                return Ok(None);
            };
            run_list(app, action, key)
        }
    }
}

/// What `C-c` waits for, said the same before and after a wrong key.
pub(super) const CHORD_HINT: &str = "C-c C-w refiles · C-c C-c quits";

/// `C-c` in the lists and Triage starts Org's `C-c C-w` refile chord;
/// anywhere else, or pressed twice, it quits. Returns true when the key
/// belonged to the chord.
fn chord_key(app: &mut App, key: KeyEvent) -> bool {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    if app.chord.take().is_some() {
        app.status = None;
        match key.code {
            KeyCode::Char('w') if control => app.open_refile(),
            KeyCode::Char('c') if control => app.quit = true,
            _ => app.set_status(CHORD_HINT),
        }
        return true;
    }
    if !is_quit_key(key) {
        return false;
    }
    let chord_screen =
        !app.pane && matches!(app.overlay, Overlay::None) && app.screen != Screen::Review;
    if chord_screen {
        app.chord = Some(Instant::now());
        app.set_status(CHORD_HINT);
    } else {
        app.quit = true;
    }
    true
}

/// Moving, tabs, capture, refresh and help, shared by the lists and the
/// Agents tab; returns false for any other action.
fn run_shared(app: &mut App, action: Action, key: KeyEvent) -> Result<bool> {
    match action {
        A::Next => app.move_selection(1),
        A::Previous => app.move_selection(-1),
        A::PageDown => app.move_selection(10),
        A::PageUp => app.move_selection(-10),
        A::First => app.selected = 0,
        A::Last => app.selected = app.row_count().saturating_sub(1),
        A::NextTab => next_tab(app, 1)?,
        A::PreviousTab => next_tab(app, -1)?,
        A::TabNumber => {
            if let KeyCode::Char(digit) = key.code {
                let index = digit as usize - '1' as usize;
                if let Some(screen) = app.tabs().get(index).cloned() {
                    app.review_diff = false;
                    app.switch_screen(screen)?;
                }
            }
        }
        A::Capture => app.overlay = Overlay::Capture(capture::Draft::default()),
        A::Refresh => {
            app.index = agd_core::WorkspaceIndex::load(app.settings.clone())?;
            app.reload_rows()?;
            app.set_status("refreshed");
        }
        A::Help => app.overlay = Overlay::Help(help::Palette::default()),
        _ => return Ok(false),
    }
    Ok(true)
}

fn next_tab(app: &mut App, delta: isize) -> Result<()> {
    let tabs = app.tabs();
    let current = tabs.iter().position(|tab| *tab == app.screen).unwrap_or(0) as isize;
    let next = (current + delta).rem_euclid(tabs.len() as isize) as usize;
    app.switch_screen(tabs[next].clone())
}

/// Today, Week, Inbox and saved views.
fn run_list(app: &mut App, action: Action, key: KeyEvent) -> Result<EditorTarget> {
    if run_shared(app, action, key)? {
        return Ok(None);
    }
    match action {
        A::Later => move_day(app, 1)?,
        A::Earlier => move_day(app, -1)?,
        A::GoToday => {
            app.day = app.today;
            app.reload_rows()?;
            app.cursor_to_today();
        }
        A::Peek => app.peek = !app.peek,
        A::Views => app.overlay = Overlay::Views { selected: 0 },
        A::Classic => {
            app.classic = !app.classic;
            app.reload_rows()?;
        }
        A::LogMode => {
            app.log_mode = !app.log_mode;
            app.reload_rows()?;
        }
        A::Context => cycle_context(app)?,
        A::Filter => app.overlay = Overlay::Prompt(Prompt::filter(&app.filter)),
        A::Edit => {
            if !app.expand_overdue()?
                && let Some(form) = super::edit::EditForm::open(app)
            {
                app.overlay = Overlay::Edit(Box::new(form));
            }
        }
        A::Editor => return Ok(selected_location(app)),
        _ => run_change(app, action)?,
    }
    Ok(None)
}

/// The change keys of the lists, and the keys that leave them.
fn run_change(app: &mut App, action: Action) -> Result<()> {
    match action {
        A::State if has_task(app, "change") => app.cycle_state()?,
        A::Done if has_task(app, "mark done") => app.mark_done()?,
        A::Cancel if has_task(app, "cancel") => app.cancel()?,
        A::Schedule => prompt(app, PromptKind::Schedule),
        A::Deadline => prompt(app, PromptKind::Deadline),
        A::Priority => prompt(app, PromptKind::Priority),
        A::PriorityUp => app.shift_priority(true)?,
        A::PriorityDown => app.shift_priority(false)?,
        A::Tags => prompt(app, PromptKind::Tags),
        A::Someday => app.someday()?,
        A::Refile => app.open_refile(),
        A::Undo => app.undo()?,
        A::CopyLink => app.copy_link()?,
        A::Mark => app.toggle_mark(),
        A::MarkAll => app.mark_all(),
        A::UnmarkAll => app.marks.clear(),
        A::Bulk if app.marks.is_empty() => app.set_status("mark rows with m first; B acts on them"),
        A::Bulk => app.overlay = Overlay::Bulk,
        A::Triage => start_triage(app)?,
        A::Agents => {
            app.review_diff = false;
            app.switch_screen(Screen::Review)?;
        }
        A::Back => list_back(app)?,
        A::Quit => app.quit = true,
        _ => {}
    }
    Ok(())
}

/// esc: close peek, then clear marks, then the filter, then leave the view.
fn list_back(app: &mut App) -> Result<()> {
    if app.peek {
        app.peek = false;
    } else if !app.marks.is_empty() {
        app.marks.clear();
    } else if !app.filter.is_empty() {
        app.filter.clear();
        app.reload_rows()?;
    } else if app.screen != Screen::Today {
        let back = app.back.clone();
        app.switch_screen(if back == app.screen {
            Screen::Today
        } else {
            back
        })?;
    }
    Ok(())
}

/// f and b: one day, or one week in Week.
fn move_day(app: &mut App, direction: i32) -> Result<()> {
    let period = if app.screen == Screen::Week {
        chrono::Duration::weeks(1)
    } else {
        chrono::Duration::days(1)
    };
    app.day += period * direction;
    app.reload_rows()
}

/// `<`: the next category, then all.
fn cycle_context(app: &mut App) -> Result<()> {
    let categories = app.categories();
    let index = app
        .context
        .as_ref()
        .and_then(|context| categories.iter().position(|category| category == context));
    app.context = match index {
        None => categories.first().cloned(),
        Some(index) => categories.get(index + 1).cloned(),
    };
    app.reload_rows()
}

/// One choice from the bulk menu, on every marked row.
fn run_bulk(app: &mut App, action: Action) -> Result<()> {
    match action {
        A::Today => app.schedule("today")?,
        A::Tomorrow => app.schedule("+1d")?,
        A::NextWeek => app.schedule("+1w")?,
        A::Schedule => prompt(app, PromptKind::Schedule),
        A::Deadline => prompt(app, PromptKind::Deadline),
        A::Someday => app.someday()?,
        A::Done => app.mark_done()?,
        A::Cancel => app.cancel()?,
        _ => {}
    }
    Ok(())
}

/// Says so when a change key has no task to act on; true when it has one.
fn has_task(app: &mut App, what: &str) -> bool {
    if app.targets().is_empty() {
        app.set_status(format!("nothing to {what}"));
        return false;
    }
    true
}

fn prompt(app: &mut App, kind: PromptKind) {
    if app.selected_row().is_some() || !app.marks.is_empty() {
        app.overlay = Overlay::Prompt(Prompt::new(kind, String::new()));
    }
}

fn start_triage(app: &mut App) -> Result<()> {
    app.start_triage()
}

/// The selected row's heading, for $EDITOR.
fn selected_location(app: &App) -> EditorTarget {
    app.selected_row()
        .map(|row| (row.item.file.clone(), row.item.line))
}

/// A finished pass: `u` undoes all of it, `k` goes back to the last card,
/// and ⏎, esc or q return to Today, as its summary card says.
fn finished_triage_key(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Char('u') => triage_undo(app),
        KeyCode::Char('k' | 'p') | KeyCode::Up | KeyCode::BackTab
            if !app.triage.queue.is_empty() =>
        {
            triage_move(app, -1)
        }
        KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q') => app.switch_screen(Screen::Today),
        KeyCode::Char('?') => {
            app.overlay = Overlay::Help(help::Palette::default());
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Moves through the frozen queue; past the last card the pass is done.
fn triage_move(app: &mut App, delta: isize) -> Result<()> {
    let position = app.triage.position as isize + delta;
    app.triage.position = position.clamp(0, app.triage.queue.len() as isize) as usize;
    app.reload_rows()
}

/// `u` in Triage: undoes the last decision and returns to its card; at the
/// end of a pass, undoes all of it and starts the pass over. Only the
/// pass's own writes, never an older, unrelated one.
fn triage_undo(app: &mut App) -> Result<()> {
    if app.triage.undo.is_empty() {
        app.set_status("nothing to undo in this pass");
        return Ok(());
    }
    let whole = app.triage.finished();
    app.undo()?;
    if whole {
        app.triage.decided.clear();
        app.triage.position = 0;
    } else if let Some(position) = app.triage.decided.pop() {
        app.triage.position = position;
    }
    app.reload_rows()
}

fn run_triage(app: &mut App, action: Action) -> Result<()> {
    match action {
        A::Today => app.schedule("today")?,
        A::Tomorrow => app.schedule("+1d")?,
        A::NextWeek => app.schedule("+1w")?,
        A::Schedule => prompt(app, PromptKind::Schedule),
        A::Deadline => prompt(app, PromptKind::Deadline),
        A::Done => app.mark_done()?,
        A::MakeNext => app.set_state("NEXT")?,
        A::Someday => app.someday()?,
        A::Cancel => app.cancel()?,
        A::Refile => app.open_refile(),
        A::Undo => triage_undo(app)?,
        A::Next => triage_move(app, 1)?,
        A::Previous => triage_move(app, -1)?,
        A::Help => app.overlay = Overlay::Help(help::Palette::default()),
        A::Back => {
            let back = app.back.clone();
            app.switch_screen(back)?;
        }
        _ => {}
    }
    Ok(())
}

fn run_agents(app: &mut App, action: Action, key: KeyEvent) -> Result<EditorTarget> {
    if run_shared(app, action, key)? {
        return Ok(None);
    }
    match action {
        A::Accept => app.accept_changes()?,
        A::Mark => app.toggle_change_mark(),
        A::MarkAll => app.mark_all_changes(),
        A::UnmarkAll => app.marked_changes.clear(),
        A::Undo => app.undo_selected_change()?,
        A::Diff => app.review_diff = !app.review_diff,
        A::Peek => app.peek = !app.peek,
        A::FilterAgent => app.cycle_agent_filter()?,
        A::Editor => return Ok(changed_location(app)),
        A::Back => agents_back(app)?,
        _ => {}
    }
    Ok(None)
}

/// The heading the selected agent change touched, for $EDITOR.
fn changed_location(app: &App) -> EditorTarget {
    let record = app.journals.get(app.selected)?;
    Some(super::review::changed_place(app, record))
}

/// esc or q on the Agents tab: close the diff, then peek, then clear the
/// marks, then go back.
fn agents_back(app: &mut App) -> Result<()> {
    if app.review_diff {
        app.review_diff = false;
    } else if app.peek {
        app.peek = false;
    } else if !app.marked_changes.is_empty() {
        app.marked_changes.clear();
    } else {
        let back = app.back.clone();
        app.switch_screen(if back == Screen::Review {
            Screen::Today
        } else {
            back
        })?;
    }
    Ok(())
}

/// The pane: glance, mark done, capture, or open the full TUI.
fn run_pane(app: &mut App, action: Action) -> Result<EditorTarget> {
    match action {
        A::Next => app.move_selection(1),
        A::Previous => app.move_selection(-1),
        A::Done if has_task(app, "mark done") => app.mark_done()?,
        A::Cancel if has_task(app, "cancel") => app.cancel()?,
        A::Undo => app.undo()?,
        A::Capture => app.overlay = Overlay::Capture(capture::Draft::default()),
        A::Editor => return Ok(selected_location(app)),
        A::Triage => {
            if app.leave_pane()? {
                start_triage(app)?;
            }
        }
        A::FullView => {
            app.leave_pane()?;
        }
        A::Help => app.overlay = Overlay::Help(help::Palette::default()),
        A::Quit => app.quit = true,
        _ => {}
    }
    Ok(None)
}

/// `?`'s keys: typing filters, the picker keys (and the page keys) choose,
/// ⏎ runs the chosen action as its key would, and esc clears the filter,
/// then closes. An action of another screen says where it runs instead.
fn help_key(app: &mut App, key: KeyEvent) -> Result<EditorTarget> {
    let current = current_screen(app);
    let Overlay::Help(palette) = &mut app.overlay else {
        return Ok(None);
    };
    let entries = help::entries(current, palette.query.text());
    match key.code {
        KeyCode::Esc if palette.query.text().is_empty() => app.overlay = Overlay::None,
        KeyCode::Esc => *palette = help::Palette::default(),
        KeyCode::Enter => {
            let Some(entry) = entries.get(palette.selected) else {
                return Ok(None);
            };
            if entry.screen != current {
                let (title, _) = SCREENS[entry.screen];
                let label = entry.binding.label;
                app.set_status(format!("{label} works in {title}, not here"));
                return Ok(None);
            }
            let Some(pressed) = first_key(entry.binding) else {
                return Ok(None);
            };
            app.overlay = Overlay::None;
            return dispatch(app, pressed);
        }
        KeyCode::PageDown => palette.selected = stepped(palette.selected, Some(10), entries.len()),
        KeyCode::PageUp => palette.selected = stepped(palette.selected, Some(-10), entries.len()),
        _ if picker_step(key).is_some() => {
            palette.selected = stepped(palette.selected, picker_step(key), entries.len());
        }
        _ => match palette.query.handle(key) {
            Edit::Cancel => app.overlay = Overlay::None,
            Edit::Changed => palette.selected = 0,
            Edit::Other => {}
        },
    }
    Ok(None)
}

/// The key event a binding's first key sends, for `?` to run it.
fn first_key(binding: &Binding) -> Option<KeyEvent> {
    Some(match binding.keys.first()? {
        Key::Char(character) => KeyEvent::from(KeyCode::Char(*character)),
        Key::Code(code) => KeyEvent::from(*code),
        Key::Ctrl(character) => KeyEvent::new(KeyCode::Char(*character), KeyModifiers::CONTROL),
        Key::Digit => KeyEvent::from(KeyCode::Char('1')),
    })
}

/// A prompt's keys. The filter is live: rows narrow as you type, and esc
/// or C-g puts back the filter it started with.
fn prompt_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let Overlay::Prompt(prompt) = &mut app.overlay else {
        return Ok(());
    };
    let filter = prompt.kind == PromptKind::Filter;
    let edit = match key.code {
        KeyCode::Esc => Edit::Cancel,
        KeyCode::Enter => return app.commit_prompt(),
        _ => prompt.input.handle(key),
    };
    let typed = prompt.input.text().to_string();
    let restore = prompt.restore.clone();
    match edit {
        Edit::Cancel => {
            app.overlay = Overlay::None;
            if filter {
                app.filter = restore;
                app.reload_rows()?;
            }
        }
        Edit::Changed if filter => {
            app.filter = typed.trim().to_string();
            app.reload_rows()?;
        }
        Edit::Changed => prompt.stale = true,
        Edit::Other => {}
    }
    Ok(())
}

fn handle_overlay(app: &mut App, key: KeyEvent) -> Result<()> {
    match &mut app.overlay {
        Overlay::Prompt(_) => prompt_key(app, key)?,
        Overlay::Refile(picker) => match key.code {
            KeyCode::Esc => app.overlay = Overlay::None,
            KeyCode::Enter => app.commit_refile()?,
            _ if picker_step(key).is_some() => {
                let count = picker.matches().len();
                picker.selected = stepped(picker.selected, picker_step(key), count);
            }
            _ => match picker.input.handle(key) {
                Edit::Cancel => app.overlay = Overlay::None,
                Edit::Changed => picker.selected = 0,
                Edit::Other => {}
            },
        },
        Overlay::Bulk => {
            if let Some(action) = action_for(BULK_KEYS, key) {
                app.overlay = Overlay::None;
                run_bulk(app, action)?;
            }
        }
        Overlay::Views { selected } => {
            let keys: Vec<String> = app
                .settings
                .all_views()
                .iter()
                .map(|view| view.key.clone())
                .collect();
            // Letters are view keys (a view may be `j` or `k`), so only the
            // arrows and ctrl keys move; q closes, as in org-agenda's
            // dispatcher.
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => app.overlay = Overlay::None,
                _ if picker_step(key).is_some() => {
                    *selected = stepped(*selected, picker_step(key), keys.len());
                }
                KeyCode::Enter => {
                    if let Some(key) = keys.get(*selected).cloned() {
                        app.overlay = Overlay::None;
                        app.switch_screen(Screen::View(key))?;
                    }
                }
                KeyCode::Char(character) => {
                    if let Some(key) = keys.iter().find(|key| **key == character.to_string()) {
                        let key = key.clone();
                        app.overlay = Overlay::None;
                        app.switch_screen(Screen::View(key))?;
                    }
                }
                _ => {}
            }
        }
        Overlay::None | Overlay::Capture(_) | Overlay::Edit(_) | Overlay::Help(_) => {}
    }
    Ok(())
}
