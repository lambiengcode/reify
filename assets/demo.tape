# The Reify feature tour. Recorded by vhs (github.com/charmbracelet/vhs):
#
#   cd /path/to/an/indexed/repository
#   REIFY_ASSETS=/path/to/reify/assets \
#     PATH=/path/to/reify/target/release:$PATH \
#     vhs $REIFY_ASSETS/demo.tape -o $REIFY_ASSETS/demo.gif
#
# vhs runs each command in a real terminal, which is why the output is coloured:
# reify prints colour only when a terminal is attached. The shell configuration in
# assets/demo-shell is sourced explicitly rather than through ZDOTDIR, because vhs
# sets up its own prompt after the shell starts and would otherwise overwrite it.
#
# Every command runs for real against a real ERPNext index (~5,000 files, 15 years of
# history). Nothing is staged: if the GIF ever disagrees with the tool, re-record it.

Output demo.gif
Require reify

Set Shell "zsh"
Set Theme "Dracula"
Set FontFamily "JetBrains Mono"
Set FontSize 14
Set Width 1040
Set Height 640
Set Padding 20
Set Framerate 10
Set TypingSpeed 25ms

Hide
Type "source ${REIFY_ASSETS:?set REIFY_ASSETS to reify/assets}/demo-shell/.zshrc" Enter
Type "clear" Enter
Show

Sleep 600ms

# Build the graph.
Type "reify index" Enter
Sleep 1800ms

# The flagship: the minimum context for a task, under a token budget.
Type "reify context 'add a discount tier for strategic customers' --budget 1500" Enter
Sleep 2800ms

# Provenance for one line: callers, data, co-change, the commits that explain it.
Type "reify why erpnext/selling/doctype/customer/customer.py:514" Enter
Sleep 2800ms

# Blast radius, including hops no single grep would follow.
Type "reify impact 'check_credit_limit'" Enter
Sleep 2400ms

# One business concept across every language, table and file it appears in.
Type "reify explain 'credit limit'" Enter
Sleep 2400ms

# The agent-facing format: columns declared once, one row per record.
Type "reify context 'block orders above the approval cap' --budget 1200 --toon" Enter
Sleep 2800ms
