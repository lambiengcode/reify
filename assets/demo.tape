# The Reify demo. Rendered by `vhs assets/demo.tape` from the repository root.
#
# Every command in it is real and runs against a real index of ERPNext (~5,000
# files, 15 years of history). Nothing is mocked: if the output in the GIF ever
# disagrees with what the tool prints, re-render the GIF, not the README.

Output assets/demo.gif

Set Theme "Catppuccin Mocha"
Set FontSize 19
Set Width 1180
Set Height 640
Set Padding 16
Set TypingSpeed 35ms

Hide
Type "cd $REIFY_DEMO_REPO && export PATH=$REIFY_DEMO_BIN:$PATH && clear"
Enter
Sleep 1s
Show

Type "# Your senior dev just quit. 11 years of business logic in their head."
Sleep 900ms
Enter
Type "# You ask: why does this credit-limit check exist? What breaks if I touch it?"
Sleep 1.2s
Enter
Enter

Type `grep -rn "check_credit_limit" erpnext/ --include="*.py" | wc -l`
Sleep 600ms
Enter
Sleep 1.4s
Type "# 49 raw matches. Zero answers. This is where your AI agent gives up too."
Sleep 2s
Enter
Enter

Type "# Same question, to reify:"
Sleep 800ms
Enter
Type "reify why erpnext/selling/doctype/customer/customer.py:514"
Sleep 500ms
Enter
Sleep 5.5s
Enter

Type "# Callers, data, co-changing files, the commits that explain it. In 200ms."
Sleep 2.4s
Enter
Enter

Type "# And the context your agent should start from — under a token budget:"
Sleep 900ms
Enter
Type `reify context "add a discount tier for strategic customers" --budget 1500`
Sleep 500ms
Enter
Sleep 6s
Enter

Type "# 1,290 tokens instead of forty open files. Every claim carries its evidence."
Sleep 2.2s
Enter
Type "# Indexed 5,000 files in 4.6s. Zero network calls. Claude Code / Cursor / Codex."
Sleep 3.5s
