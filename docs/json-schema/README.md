# JSON output schemas

Generated from real command output by `docs/json-schema/regenerate.sh`, so they cannot
drift from the code. Each answer carries a `schema` field; a breaking change bumps its
version.

**Two fields are load-bearing for every consumer:**

- `status` appears on every item in every section. `CONFIRMED` and `OBSERVED` are
  deterministic; `INFERRED` and `CONFLICTED` must be verified against their citation
  before acting.
- `unknowns` states what could not be determined, so an agent does not read absence as
  evidence of absence.

## `reify context --json`

```json
{
  "schema": "string",
  "task": "string",
  "budget": {
    "requested": "integer",
    "context": "integer",
    "reads": "integer",
    "used": "integer",
    "unit": "string",
    "estimator": "string"
  },
  "concepts": [
    {
      "id": "string",
      "status": "string",
      "labels": {
        "eng": "..."
      },
      "code": [
        "..."
      ],
      "db": [
        "..."
      ],
      "bridge": "string"
    }
  ],
  "rules": [
    {
      "id": "string",
      "status": "string",
      "confidence": "number",
      "claim": "string",
      "subject": "string",
      "source": "string",
      "evidence": [
        "..."
      ]
    }
  ],
  "code": [
    {
      "path": "string",
      "symbol": "string",
      "lines": "string",
      "why": "string",
      "status": "string",
      "signature": "string"
    }
  ],
  "documents": [],
  "data": [
    {
      "table": "string",
      "why": "string",
      "status": "string"
    }
  ],
  "history": [
    {
      "commit": "string",
      "date": "string",
      "subject": "string",
      "class": "string",
      "why_relevant": "string"
    }
  ],
  "conflicts": [],
  "unknowns": [
    "string"
  ],
  "next_reads": [
    {
      "path": "string",
      "lines": "string",
      "est_tokens": "integer"
    }
  ]
}
```

## `reify why --json`

```json
{
  "schema": "string",
  "target": "string",
  "symbol": "string",
  "location": "string",
  "kind": "string",
  "signature": "string",
  "documentation": "null",
  "concepts": [],
  "documents": [],
  "calls": [
    {
      "location": "string",
      "what": "string",
      "status": "string"
    }
  ],
  "called_by": [
    {
      "location": "string",
      "what": "string",
      "status": "string"
    }
  ],
  "reads": [],
  "writes": [],
  "history": [
    {
      "sha": "string",
      "date": "string",
      "author": "string",
      "subject": "string",
      "class": "string"
    }
  ]
}
```

## `reify impact --json`

```json
{
  "schema": "string",
  "query": "string",
  "origins": [
    {
      "location": "string",
      "what": "string",
      "status": "string"
    }
  ],
  "affected": [
    {
      "location": "string",
      "what": "string",
      "kind": "string",
      "distance": "integer",
      "reason": "string",
      "status": "string",
      "confidence": "number"
    }
  ],
  "tables": [],
  "co_changing_files": [
    {
      "location": "string",
      "what": "string",
      "status": "string"
    }
  ],
  "unknowns": []
}
```

## `reify preflight --json`

```json
{
  "schema": "string",
  "path": "string",
  "symbols": "integer",
  "rules": "integer",
  "concepts": "integer",
  "tables": "integer",
  "dependants": "integer",
  "conflicts": "integer",
  "risk": "string",
  "reason": "string",
  "highest_risk_rules": [
    {
      "location": "string",
      "what": "string",
      "status": "string"
    }
  ],
  "suggested_command": "string"
}
```

## `reify doctor --json`

Answers before there is an index, so it reads the working tree and `git log` rather than
the store. `verdict` is one of `too_small`, `likely_worth_it`, `marginal`,
`unlikely_to_help`. `vocabulary` and `history` are `null` below the line floor and when
git history cannot be read — absent rather than defaulted, so a consumer cannot mistake
"not measured" for "measured zero". Metric definitions: [`../metrics.md`](../metrics.md).

```json
{
  "schema": "string",
  "root": "string",
  "scale": {
    "indexable_files": "integer",
    "code_files": "integer",
    "lines": "integer"
  },
  "git_repository": "boolean",
  "vocabulary": {
    "commits_considered": "integer",
    "commits_local": "integer",
    "locality": "number"
  },
  "history": {
    "commits_read": "integer",
    "truncated": "boolean",
    "usable_subjects": "integer",
    "usable_share": "number",
    "focused_commits": "integer",
    "focus": "number",
    "median_files_changed": "integer"
  },
  "documents": {
    "unreadable_by_grep": "integer",
    "examples": [
      "string"
    ]
  },
  "verdict": "string",
  "reason": "string",
  "what_would_change_it": [
    "string"
  ],
  "elapsed_ms": "integer"
}
```

## `reify install --json`

The plan, whether or not it was applied. `applied` is false unless `--yes` was passed.
`kind` is one of `instructions`, `rule_file`, `mcp`; `state` is one of `planned`,
`already_present`, `skipped`. `problem` is non-null only when `state` is `skipped`, and
says why the file was left alone. `evidence` is what each detection rests on, so a
consumer can check the claim rather than trust it. `instruction_block` is non-null only
when no agent was recognised — it is the text to paste by hand.

```json
{
  "schema": "string",
  "root": "string",
  "mcp": "boolean",
  "applied": "boolean",
  "steps": [
    {
      "path": "string",
      "kind": "string",
      "agents": [
        "string"
      ],
      "evidence": [
        "string"
      ],
      "state": "string",
      "problem": "null"
    }
  ],
  "instruction_block": "null",
  "detected_elsewhere": [
    "string"
  ]
}
```
