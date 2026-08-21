#!/bin/bash
# The Reify demo, driven for terminalizer. Every command is real and runs against
# a real index of ERPNext (~5,000 files, 15 years of history). Nothing is mocked.
# Recorded by `terminalizer record` with assets/terminalizer.yml; see assets/README.md.
set -u
cd "$REIFY_DEMO_REPO"
export PATH="$REIFY_DEMO_BIN:$PATH"

P=$'\e[1;32m❯\e[0m '
type_cmd() {
  printf '%s' "$P"
  local s="$1"
  for ((i = 0; i < ${#s}; i++)); do
    printf '%s' "${s:i:1}"
    sleep 0.028
  done
  printf '\n'
}
say() { type_cmd "$1"; sleep "${2:-0.9}"; }

sleep 0.6
say "# Your senior dev just quit. 11 years of business logic in their head." 0.9
say "# You ask: why does this credit-limit check exist? What breaks if I touch it?" 1.2
echo

type_cmd 'grep -rn "check_credit_limit" erpnext/ --include="*.py" | wc -l'
sleep 0.4
grep -rn "check_credit_limit" erpnext/ --include="*.py" | wc -l
sleep 1.2
say "# 49 raw matches. Zero answers. This is where your AI agent gives up too." 1.8
echo

say "# Same question, to reify:" 0.7
type_cmd "reify why erpnext/selling/doctype/customer/customer.py:514"
sleep 0.4
reify why erpnext/selling/doctype/customer/customer.py:514
sleep 4.5
echo

say "# Callers, data, co-changing files, the commits that explain it. In 200ms." 2.2
echo

say "# And the context your agent should start from — under a token budget:" 0.9
type_cmd 'reify context "add a discount tier for strategic customers" --budget 1500'
sleep 0.4
reify context "add a discount tier for strategic customers" --budget 1500
sleep 5
echo

say "# 1,290 tokens instead of forty open files. Every claim carries its evidence." 2
say "# Indexed 5,000 files in 4.6s. Zero network calls. Claude Code / Cursor / Codex." 3
