# Claude Code

Four levels, cheapest first. **Start at level 0** — it works today, costs nothing until
used, and is what the benchmark measured.

## Level 0 — a shell command (recommended)

`reify init` finds your `AGENTS.md` or `CLAUDE.md` and tells you what to add.
`reify init --write-agent-instructions` appends it for you:

```markdown
## Before changing code in this repo
Run `reify context "<what you are about to do>" --toon` and read its output first.
Run `reify why <file>:<line>` before modifying unfamiliar logic.
Run `reify impact "<symbol>"` before changing anything shared.
Treat `status: INFERRED` claims as leads to verify, not as facts.
```

That is the whole integration. No protocol, no server, no schema cost per turn.

**Why this rather than MCP:** an MCP server's tool schemas are re-sent on every turn of
every session. A CLI costs nothing until it is called. For a tool whose entire purpose
is reducing context, paying a per-turn tax to deliver it would be self-defeating.

## Level 1 — MCP, if your client cannot run a shell command

```bash
reify serve --mcp
```

Six tools — `reify_context`, `reify_why`, `reify_impact`, `reify_explain`,
`reify_flow`, `reify_conflicts` — and that is the whole surface, deliberately.
`mcp::tests::the_tool_schemas_stay_small_enough_to_be_worth_sending` asserts the
schemas cost under 600 tokens, which all six still fit inside.

## Level 2 — a preflight hook

Inject a risk header whenever the agent is about to edit a file:

```json
{
  "hooks": {
    "PreToolUse": [{
      "matcher": "Edit|Write",
      "hooks": [{ "type": "command", "command": "reify preflight \"$CLAUDE_FILE_PATH\"" }]
    }]
  }
}
```

Output is one dense block, asserted under 300 tokens because it runs on every edit:

```
PREFLIGHT  erpnext/selling/doctype/sales_order/sales_order.py
  rules 7 · concepts 4 · tables 3 · dependants 22 · conflicts 1
  · Corporate customers must require approval before an order is confirmed
  RISK: HIGH — documentation and implementation disagree about this file
  next: reify context "<your change to erpnext/.../sales_order.py>"
```

Non-blocking. A hook that blocks edits gets uninstalled, and then its warnings are lost
too — so blocking is a choice you make deliberately, not a default.

## Level 3 — keep the index fresh

```bash
printf '#!/bin/sh\nreify index >/dev/null 2>&1 &\n' > .git/hooks/post-merge
chmod +x .git/hooks/post-merge
cp .git/hooks/post-merge .git/hooks/post-checkout
```

`reify status` tells you when the index is behind `HEAD`.

## Reading the output

Every claim carries a status. The two that matter:

- `CONFIRMED` / `OBSERVED` — parsed or derived deterministically. Act on it.
- `INFERRED` / `CONFLICTED` — a heuristic, or a disagreement. **Read the citation first.**

`next_reads` is a reading plan: precise line spans, already inside the token budget.
Reading those spans instead of whole files is where the saving comes from.
