# ggpage demo tape — generate the README GIF with:
#     cargo build --release
#     vhs demo.tape            # produces demo.gif
#
# vhs: https://github.com/charmbracelet/vhs

Output demo.gif

Set Shell bash
Set FontSize 18
Set Width 1200
Set Height 720
Set Padding 12
Set Theme "Catppuccin Mocha"

# Launch ggpage on a self-contained sample result set (no DB needed).
Type "./target/release/ggpage demo/users.txt"
Enter
Sleep 1.5s

# Move the row cursor down a few rows.
Down@350ms 3
Sleep 700ms

# Walk the column cursor over to the JSON `meta` column (frozen id stays put).
Right@350ms 4
Sleep 700ms

# Pretty-print the JSON cell in a popup.
Type "v"
Sleep 2.2s
Escape
Sleep 400ms

# Column stats on `country`.
Left@350ms 2
Type "s"
Sleep 2.2s
Escape
Sleep 400ms

# Search (Unicode) and jump to the match.
Type "/日本"
Enter
Sleep 1.6s

# Toggle column chopping.
Type "-S"
Sleep 1.2s

# Show the key-bindings help overlay.
Type "?"
Sleep 2.8s
Escape
Sleep 400ms

# Quit.
Type "q"
Sleep 800ms
