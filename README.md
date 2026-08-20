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

Run on **ERPNext**, 5,284 files and 60,946 commits, with 40 tasks derived from real
merged commits: the prompt is the developer's own description of the change, the ground
truth is the files they actually changed.

**The index is built at a commit before any of those changes were made**, so the code
being asked for is genuinely absent. Every condition is held to the same 4,000-token
budget.

### With a model in the loop

The model is given the task and one context block, and asked which files must change.
Three of the five conditions exist to try to falsify the result rather than support it.

| Condition | | Hit rate | 95% CI | Recall |
|---|---|---:|---:|---:|
| No context at all | *memorisation control* | 22% | 12–38% | 0.15 |
| Content grep, budget-matched | *baseline* | 25% | 14–40% | 0.20 |
| **Reify** | | **65%** | **50–78%** | **0.59** |
| Reify, context from a *different* task | *negative control* | 30% | 18–45% | 0.21 |
| Perfect context | *ceiling* | 100% | 91–100% | 1.00 |

**What the controls establish:**

- **Context is the bottleneck.** Perfect context scores 100% where none scores 22%.
  That 78-point gap is the entire space any retrieval system can compete in — and it is
  wide, which is the single most important thing this benchmark had to determine.
- **Reify recovers 55% of that gap. Grep recovers 3%.**
- **The content is doing the work, not the framing.** A decoy context of identical
  shape and size scores 30% against Reify's 65%.
- **Contamination is modest.** With no repository access the model still scores 22%,
  and that floor is subtracted above rather than ignored.

Reify's confidence interval does not overlap the baseline's. It costs about 1.8× the
prompt tokens for about 2.6× the hit rate — the gain is bought with tokens, not free.

### Retrieval quality, without a model

Does the tool put the right file in front of the agent at all?

| | content grep | path grep | **Reify** |
|---|---:|---:|---:|
| Tasks where a changed file was surfaced | 4/40 (10%) | 7/40 (18%) | **24/40 (60%)** |
| Mean recall | 0.08 | 0.16 | **0.53** |
| MRR of the first correct file | 0.07 | 0.12 | **0.20** |
| Expected tokens to reach a changed file | 3,876 | 3,451 | **3,381** |
| Median files put in front of the agent | 3 | 88 | 13 |
| Median latency | 43 ms | 0 ms | 57 ms |

### Does it generalise? A second repository, in a typed language

**OpenMRS** — Java, 1,325 files, 13,182 commits, 22 tasks, same leakage-free method.

| Condition | Hit rate |
|---|---:|
| No context at all | **0%** |
| Content grep, budget-matched | 41% |
| **Reify** | **50%** |
| Decoy context | 5% |
| Perfect context | 100% |

The direction holds and the controls are cleaner than ERPNext's — zero memorisation, and
a decoy scores 5% against Reify's 50%. **But the margin is far smaller: 9 points over
grep, against 40 points on ERPNext.**

The cause is measurable rather than mysterious. Reify builds **579 concepts** on ERPNext
and **10** on OpenMRS, because ERPNext declares its domain vocabulary in structured
entity metadata and OpenMRS does not — in the form Reify currently reads. OpenMRS *does*
ship 20 Hibernate mappings and 27 message bundles, which are the same shape and are not
yet ingested.

So the honest summary is: **Reify's advantage scales with how much declared vocabulary a
repository hands it.** On a repository that declares a lot, it is transformative. On one
that declares little, it is a modest improvement over grep. Closing that gap is the
clearest item on the roadmap.

### Measured performance

Same repository, 8-core M-series laptop. Three targets are still missed, and say so.

| | Measured | Target | |
|---|---:|---:|---|
| Full index (5,284 files, no model) | 78 s | < 10 min | ✅ |
| `reify context` | 57 ms | < 100 ms | ✅ |
| `reify impact` | 0.2 ms | < 50 ms | ✅ |
| `reify why` | 205 ms | < 20 ms | ❌ includes a `git log -L` subprocess; ~5 ms without |
| Reindex, nothing changed | 0.6 s | — | ✅ early exit |
| Reindex, one function edited | 5.9 s | < 500 ms | ❌ see below |
| Peak memory, full index | 224 MB | < 2 GB | ✅ |
| Store size | 45 MB (31% of the 144 MB working tree) | < 5% | ❌ |

**Why a one-file edit still costs 5.9 s.** Only the changed file is re-parsed, but three
stages are rebuilt across the whole repository every run: reference resolution, the
concept layer, and rule corroboration with conflict detection. Each depends on what
*other* files say, and a property test asserts that an incremental index is
byte-identical to a full rebuild. Making those stages incremental without breaking that
guarantee is the next performance task, not a tuning knob.

## Install and use

```bash
# From a release build (recommended once tagged):
#   curl -sSL https://github.com/lambiengcode/reify/releases/latest/download/... | tar xz
cargo install --path crates/reify-cli

cd your-repo
reify init      # reports what it will and will not index, and why
reify index     # ~80s for a 5,000-file repository; 0.6s when nothing changed
```

`reify init --write-agent-instructions` appends Reify's usage block to your `AGENTS.md`
or `CLAUDE.md`. `reify completions <shell>` prints a completion script.

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
