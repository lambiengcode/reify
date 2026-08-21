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

## Codex, Cursor, OpenCode, Aider, Pi

No adapter needed. Put the block above in whatever instruction file the tool reads
(`AGENTS.md`, `.cursorrules`, `CONVENTIONS.md`). The CLI is the interface.
