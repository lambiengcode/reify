# Reify

**A local knowledge engine that gives AI coding agents the smallest context they need
to change a ten-year-old business system correctly.**

Reify compiles your source, SQL, structured model metadata, documents and Git history
into a fast local knowledge graph, then answers three questions an agent cannot answer
by grepping:

```
reify why erpnext/selling/doctype/customer/customer.py:514
reify impact "move the credit limit check to the party level"
reify context "add a discount tier for strategic enterprise customers"
```

Everything runs offline. **This build makes no network call at all.**

---

## The problem

AI agents are excellent on greenfield code and unreliable on mature systems, because
the knowledge that decides correctness is not in any one file. It is spread across a
BRD nobody has opened since 2019, a `customer_group == 7` magic number, a guard clause
added to fix a production incident, and a Vietnamese requirements document the code
never referenced.

The agent cannot read all of it, so it reads the wrong subset — confidently.

## What Reify does about it

**Deterministic first. Semantic second. LLM last.** Symbols, call edges, SQL table
access, entity definitions, document structure, commit lineage and terminology
mappings are extracted deterministically. Retrieval is lexical and graph-based. This
build calls no model at all.

Every claim carries where it came from and how much to trust it:

| Status | Meaning |
|---|---|
| `CONFIRMED` | Read directly out of a source artifact |
| `OBSERVED` | Derived deterministically from confirmed facts |
| `INFERRED` | Heuristic. Verify against the citation before acting |
| `CONFLICTED` | Two sources disagree — resolve before changing anything |
| `UNKNOWN` | Explicitly unresolved, so absence is never read as evidence |

---

## Measured results

Run on **ERPNext at `2328e6da`** — 5,169 files, 60,946 commits, 18,900 symbols.
40 tasks derived from real merged commits: the prompt is the developer's own
description, the ground truth is the files they actually changed. Every condition is
held to the same 4,000-token budget.

| Metric | content grep | path grep | **Reify** |
|---|---:|---:|---:|
| Tasks where a changed file was surfaced | 9/40 (22%) | 7/40 (18%) | **16/40 (40%)** |
| Mean recall of changed files | 0.19 | 0.16 | **0.35** |
| Expected tokens to reach a changed file | 3,598 | 3,500 | 3,627 |
| Median files put in front of the agent | 4 | 86 | 14 |
| Median latency | 51 ms | 0 ms | 63 ms |

**Read that honestly.** At an equal token budget Reify roughly **doubles the chance of
surfacing the file that has to change** — and it does **not** reduce token cost.
Expected cost is a dead heat, and on the four tasks both approaches solved, plain grep
reached the answer for fewer tokens on three of them.

So half the product thesis is supported by this experiment and half is not. The report
names all 8 tasks where the baseline beat Reify, and lists five limitations that bound
what these numbers mean — starting with the fact that this measures *retrieval*, not
whether an agent then makes the change correctly.

Everything is reproducible:

```bash
reify-bench tasks  --repo <repo> --out benchmarks/tasks/erpnext.json
reify-bench run    --repo <repo> --tasks benchmarks/tasks/erpnext.json --out results/
reify-bench report --in results/ --out benchmarks/REPORT.md
```

Full report and raw per-task outcomes: [`benchmarks/REPORT.md`](benchmarks/REPORT.md).

### Measured performance

Same repository, 8-core M-series laptop. These are what the tool actually did, not
what the plan hoped for — three of them miss their target and say so.

| | Measured | Target | |
|---|---:|---:|---|
| Full index (5,169 files, no model) | 78 s | < 10 min | ✅ |
| `reify context` | 68 ms | < 100 ms | ✅ |
| `reify why` | 205 ms | < 20 ms | ❌ includes a `git log -L` subprocess; ~5 ms without it |
| Incremental index, one function edited | 5.9 s | < 500 ms | ❌ see below |
| Peak memory during full index | 224 MB | < 2 GB | ✅ |
| Store size | 80 MB (56% of the 144 MB working tree) | < 5% | ❌ |

**Why incremental indexing misses by 12×.** Only the changed file is re-parsed, but
three stages are rebuilt across the whole repository on every run: reference
resolution, the concept layer, and rule corroboration with conflict detection. That is
a deliberate correctness choice — each depends on what *other* files say, and a
property test asserts that an incremental index is byte-identical to a full rebuild.
Making those stages incremental without breaking that guarantee is the next
performance task, not a tuning knob.

**Why the store is large.** It keeps a full-text index and every symbol's search body.
Dropping the stored bodies in favour of on-demand re-reads is the obvious fix and is
not done yet.

---

## Install and use

```bash
cargo install --path crates/reify-cli

cd your-repo
reify init      # reports what it will and will not index, and why
reify index     # ~80s for a 5,000-file repository; incremental afterwards
```

Then tell your agent, in `AGENTS.md` or `CLAUDE.md`:

```markdown
Before changing code here, run `reify context "<what you are about to do>"`.
Run `reify why <file>:<line>` before modifying unfamiliar logic.
Treat `status: INFERRED` claims as leads to verify, not as facts.
```

Every command takes `--json` for agent consumption and `--budget <tokens>` to bound
the answer.

| Command | Answers |
|---|---|
| `reify context "<task>"` | The minimum knowledge needed, plus a reading plan |
| `reify why <file:line>` | What this is, what calls it, what data it touches, what changed it |
| `reify impact "<change>"` | What depends on this, including through the database |
| `reify conflicts` | Documentation that disagrees with the implementation |
| `reify rules` | Mined business rules, with evidence |
| `reify report` | A system-level scorecard |

---

## Multilingual

Concept ids are opaque and every label carries a language tag, including English — no
language is canonical. A Vietnamese requirement reaches English code through the
concept layer, not through a multilingual embedding model, which is why it is
deterministic and citable.

Three bridges, in precision order: a declared glossary (`.reify/glossary.toml`), the
product's own structured metadata and translation files, and co-occurrence between
document headings and code identifiers.

**Honest caveat:** ERPNext ships no translation files, so the translation bridge is
covered by tests but is *not* exercised by the benchmark above.

---

## Privacy

1. No network connection is made. A test runs the whole suite and fails if one is attempted.
2. Indexing and querying work entirely offline.
3. The store lives in `.reify/` and is gitignored by `reify init`.
4. Reify never executes anything in your repository. tree-sitter parses; it does not run.

## What Reify is not

Not an agent, not an editor, not a chatbot, not a vector database, not a cloud service.

**And not useful on small repositories.** Under roughly 20k LOC, `ripgrep` is genuinely
better and you should use it.

## Status

Early. The first vertical slice is complete and measured: Python, TypeScript,
JavaScript and SQL; Markdown and text documents; structured model metadata; Git
history; concepts; rules; conflicts; incremental indexing. See
[`docs/PLAN.md`](docs/PLAN.md) for the full plan, including the kill criteria this
project holds itself to.

Licensed under Apache-2.0.
