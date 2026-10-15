# Records docs/release/agd.gif from the sample tree in docs/demo.
#   cargo build --release && vhs docs/release/agd.tape
Output docs/release/agd.gif

Require python3
Set Shell "bash"
Set FontSize 15
Set Width 1360
Set Height 780
Set Padding 18
Set TypingSpeed 45ms
Set Theme { "name": "agd", "background": "#14171c", "foreground": "#dee2e8", "black": "#14171c", "red": "#f07178", "green": "#7ec68f", "yellow": "#e5b567", "blue": "#6ea8fe", "magenta": "#e879b0", "cyan": "#5ec4a0", "white": "#dee2e8", "brightBlack": "#808896", "brightRed": "#f07178", "brightGreen": "#7ec68f", "brightYellow": "#e5b567", "brightBlue": "#6ea8fe", "brightMagenta": "#e879b0", "brightCyan": "#5ec4a0", "brightWhite": "#ffffff", "cursor": "#6ea8fe", "selection": "#25344a" }

Hide
Env COLORTERM "truecolor"
Env AGD "target/release/agd"
Type "docs/demo/demo.sh" Enter
Sleep 3s
Show

# Today: sections you act on, overdue collapsed, inbox with provenance.
Sleep 3s
Down Down Down Down Down Down Down Sleep 700ms
Type "p" Sleep 3s
Type "p" Sleep 500ms

# Week: every day, classic org-agenda prefixes.
Tab Sleep 2.5s

# Agent review: what changed while you weren't looking.
Tab Tab Sleep 3s
Type "a" Sleep 1.5s

# Triage: one card, one key per decision.
Type "1" Sleep 500ms
Type "T" Sleep 3s
Type "s" Sleep 1.5s
Type "m" Sleep 1.5s
Escape Sleep 800ms

# Capture: tokens become chips, the preview is the exact org text.
Type "c" Sleep 400ms
Type "t" Sleep 300ms
Type "Renew passport due:fri on:+1d +personal #A" Sleep 3s
Enter Sleep 2s

# Saved views, like C-c a.
Type "a" Sleep 1.5s
Type "w" Sleep 3s
Type "q"
