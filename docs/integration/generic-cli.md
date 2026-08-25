# Any agent with a shell

Reify is a CLI. If your agent can run a command, it is integrated.

```bash
reify context "add a discount tier for strategic customers" --toon --budget 4000
reify why erpnext/selling/doctype/customer/customer.py:514 --json
reify impact "check_credit_limit" --json
```

Every command supports `--json` against a versioned schema and `--budget <tokens>`.

## The output contract

`--json` emits a `schema` field (`reify.context/1`, `reify.why/1`, …). Schemas are
versioned; a breaking change bumps the number. Full schemas: [`../json-schema/`](../json-schema/).

Three fields deserve attention:

| Field | Why it matters |
|---|---|
| `status` | On every item in every section. `INFERRED` means verify before acting |
| `next_reads` | Precise spans to open next, already inside the budget |
| `unknowns` | What Reify could not determine, stated so absence is not read as evidence |

## Exit codes

`0` on success. `1` with a message on stderr otherwise. Errors say what to do —
"no index at …; run `reify init && reify index` first" rather than a stack trace.

## Suggested agent instructions

```markdown
Before editing an unfamiliar area, run:
    reify context "<what you are about to do>" --toon --json
Read `rules` and `conflicts` first, then open the spans in `next_reads`.
Claims marked INFERRED are leads to verify, not facts.
If `conflicts` is non-empty, resolve the disagreement before changing behaviour.
```

## Codex, Cursor, Windsurf, Cline, Copilot, OpenCode, Aider, Pi

No adapter needed. `reify install` finds which of these this repository is configured for
and writes the block into each one's own file — a dedicated rule file where the tool has
a rules directory (`.cursor/rules/`, `.windsurf/rules/`, `.clinerules/`), an append where
it reads a single file (`AGENTS.md`, `.cursorrules`, `.github/copilot-instructions.md`,
`CONVENTIONS.md`). It shows the plan and stops unless `--yes`.

Detection needs evidence **in the repository**. A tool installed on your machine but not
configured here is listed and left alone: `~/.cursor` says you have Cursor, not that this
repository is worked on with it, and creating a `.cursorrules` on that basis would be a
guess. Where nothing is recognised, `install` prints the block for you to place.

Everything it writes is inside the repository, so `reify uninit` reverses all of it. That
is also why no machine-wide MCP config is touched: a per-repository uninstall cannot
safely undo a machine-wide registration.

The CLI is the interface; put the block above anywhere yourself if you prefer.
