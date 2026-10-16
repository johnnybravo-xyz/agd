# Records docs/release/agd-story.gif: working in Claude Code, a glance at
# the agenda and two captures, then the day's tasks from you, Claude and
# Codex in agd, a task with a checklist checked off, and the Agents tab.
# It runs a real Claude Code session (with the agd plugin installed) on a
# throwaway agenda; see docs/demo/story/setup.sh.
#   cargo build --release && vhs docs/release/story.tape
Output docs/release/agd-story.gif

Require tmux
Require claude
Set Shell "bash"
Set FontSize 15
Set Width 1360
Set Height 800
Set Padding 18
Set TypingSpeed 35ms
Set Theme { "name": "agd", "background": "#14171c", "foreground": "#dee2e8", "black": "#14171c", "red": "#f07178", "green": "#7ec68f", "yellow": "#e5b567", "blue": "#6ea8fe", "magenta": "#e879b0", "cyan": "#5ec4a0", "white": "#dee2e8", "brightBlack": "#808896", "brightRed": "#f07178", "brightGreen": "#7ec68f", "brightYellow": "#e5b567", "brightBlue": "#6ea8fe", "brightMagenta": "#e879b0", "brightCyan": "#5ec4a0", "brightWhite": "#ffffff", "cursor": "#6ea8fe", "selection": "#25344a" }

Hide
Env COLORTERM "truecolor"
Type `R=$(docs/demo/story/setup.sh) && tmux -L agdstory -f docs/demo/story/tmux.conf new-session -s story -n claude -c ~/code/ingest "env PATH=$R/bin:$PATH claude --model sonnet --name ingest --allowedTools mcp__plugin_agd_agd__capture" \; new-window -d -n agd -c ~/code/ingest "env PATH=$R/bin:$PATH PS1='~/code/ingest \$ ' bash --norc"`
Enter
Wait+Screen@60s /for agents/
Sleep 1s
Show

# Deep in work with Claude.
Sleep 1.5s
Type "dedupe keeps the wrong record when two share updated_at — make the later one win, add a test, then suggest one follow-up"
Sleep 400ms Enter
Sleep 3s

# Something comes to mind: a glance at the agenda, then capture it.
Type "/agd" Sleep 400ms Enter
Sleep 3s
Type "a" Sleep 2.5s
Escape Sleep 800ms
Type "/capture t Renew the car insurance due:mon #personal !A" Sleep 600ms Enter
Sleep 2s

# Claude finishes with a suggestion worth keeping.
Hide
Wait+Screen@180s /for \d+s · done/
Sleep 1s
Show
Sleep 5s
Type "good catch — add that to my agenda, tagged ingest, with the +05:30 example as a note"
Sleep 400ms Enter
Hide
Wait+Screen@180s /Called plugin:agd[\s\S]*for \d+s · done/
Sleep 1s
Show
Sleep 4s

# Later, in another terminal: everything from you, Claude and Codex.
Ctrl+B Type "2" Sleep 1s
Type "agd" Sleep 300ms Enter
Sleep 3s
Type "4" Sleep 3.5s

# A task with steps: alt-return adds a checklist line under it.
Type "c" Sleep 700ms Type "t" Sleep 400ms
Type "Plan the team offsite due:fri #work" Sleep 500ms
Alt+Enter Type "book the venue" Sleep 300ms
Alt+Enter Type "send the invites" Sleep 300ms
Alt+Enter Type "order lunch" Sleep 1.5s
Enter Sleep 1.5s

# Check the steps off one by one in the edit popup: x, as for a task.
PageDown Sleep 600ms Enter Sleep 1.2s
Up Sleep 250ms Up Sleep 250ms Up Sleep 800ms
Type "x" Sleep 1s Down Sleep 400ms Type "x" Sleep 1.2s
Enter Sleep 1.5s
Space Sleep 2.5s Space Sleep 500ms

Type "5" Sleep 3s
Space Sleep 3.5s
Space Down Down Down Sleep 1.5s
Type "a" Sleep 2s
Type "q" Sleep 1s
