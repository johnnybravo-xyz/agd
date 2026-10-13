Output docs/release/agd.mp4

Set Shell "bash"
Set FontSize 18
Set Width 1280
Set Height 720
Set TypingSpeed 35ms
Set PlaybackSpeed 1.0
Set Padding 24
Set Theme { "name": "Agenda", "background": "#111318", "foreground": "#d8dee9", "black": "#111318", "red": "#ef6f6c", "green": "#7bd88f", "yellow": "#e5c07b", "blue": "#61afef", "magenta": "#c678dd", "cyan": "#56b6c2", "white": "#d8dee9", "brightBlack": "#5c6370", "brightRed": "#ef6f6c", "brightGreen": "#7bd88f", "brightYellow": "#e5c07b", "brightBlue": "#61afef", "brightMagenta": "#c678dd", "brightCyan": "#56b6c2", "brightWhite": "#ffffff" }

Hide
Type "demo=$(mktemp -d)"
Enter
Type "printf '[files]\\nroots = [\"%s\"]\\n' \"$demo\" > \"$demo/agd.toml\""
Enter
Type "printf '* TODO [#A] Ship release :work:\\nSCHEDULED: <2026-10-13 Tue>\\n* NEXT Verify Emacs oracle :work:\\nDEADLINE: <2026-10-15 Thu -3d>\\n* TODO Process inbox\\n' > \"$demo/tasks.org\""
Enter
Type "export AGD='target/release/agd --config '$demo'/agd.toml'"
Enter
Show

Type "$AGD agenda --span day --from 2026-10-13"
Enter
Sleep 7s

Type "$AGD agenda --span day --from 2026-10-13 --csv"
Enter
Sleep 6s

Type "$AGD agenda --match work --json"
Enter
Sleep 7s

Type "$AGD capture t 'Renew passport due:fri +personal' --dry-run"
Enter
Sleep 8s

Type "$AGD"
Enter
Sleep 5s
Type "T"
Sleep 5s
Type "q"
Sleep 2s
Type "q"
Sleep 3s

Type "# untouched bytes preserved; every real write is journaled and undoable"
Sleep 5s
