# Architecture

The one-paragraph version: Reify walks a repository, extracts knowledge from code,
data, documents and history into a typed graph in a single SQLite file, and answers
questions by seeding from a lexical index, spreading across that graph, and selecting
what fits a token budget.

This document is the map.

## Layers, and their cost ordering

```
  LAYER 4  Synthesis      optional model, cached, always INFERRED        llm.rs
  ───────────────────────────────────────────────────────────────────────────────
  LAYER 3  Selection      seed → spread → budget knapsack → render       context.rs
  ───────────────────────────────────────────────────────────────────────────────
  LAYER 2  Semantics      concepts, rules, conflicts        concepts.rs  rules.rs
  ───────────────────────────────────────────────────────────────────────────────
  LAYER 1  Structure      symbols, calls, tables, sections, commits
                                              extract/*.rs  gitlog.rs
  ───────────────────────────────────────────────────────────────────────────────
  LAYER 0  Substrate      walk, classify, hash, store       discover.rs  store.rs
```

The ordering is enforced, not conventional. Layer 4 is unreachable without an
explicitly configured provider, and unreachable at all under `REIFY_OFFLINE=1`. Every
command produces a useful answer from layers 0–3 alone.

## Modules

| Module | Owns |
|---|---|
| `model` | Node and edge types, epistemic `Status`, stable uids. The vocabulary everything else speaks |
| `store` | SQLite schema, FTS5, stage-scoped invalidation, canonical dump |
| `discover` | Walk, classify, hash. Deliberately does not retain file text |
| `extract/code` | tree-sitter symbol and reference extraction; heuristic resolution |
| `extract/docs` | Section splitting and per-section language detection |
| `extract/richdoc` | HTML, DOCX and PDF, converted to the same Markdown-shaped intermediate |
| `extract/schema` | Structured entity metadata — the declared business vocabulary |
| `extract/sqlish` | Table access from `.sql` files and embedded queries |
| `gitlog` | History, co-change, commit classification, bounded line-range queries |
| `concepts` | The multilingual concept layer and its three bridges |
| `rules` | Rule candidates, corroboration, conflict detection |
| `index` | The pipeline, and the incremental contract |
| `query` | `why`, `impact`, `explain`, `flow`, `preflight`, `report` |
| `context` | Context compilation. The product |
| `llm` | The optional external-command provider |
| `tokens` | Budget estimation |

## The invariant that shapes the storage design

**An incremental index must be byte-identical to a full rebuild.**

Achieved by giving each stage a disjoint set of edge kinds and its own invalidation
trigger:

| Stage | Edge kinds | Invalidated when |
|---|---|---|
| content | `CALLS` `IMPORTS` `INHERITS` `READS` `WRITES` `DOCUMENTED_BY` `TESTED_BY` `IMPLEMENTS_RULE` | that file's hash changes |
| concepts | `MAPS_TO` | every run — rebuilt globally |
| rules | rule and conflict nodes | every run — rebuilt globally |
| history | `INTRODUCED_BY` `CHANGED_BY` `CO_CHANGES_WITH` | `HEAD` moves |

Stages whose output depends on the *whole* repository are rebuilt whole. They can do
that without re-parsing anything, because per-file facts are persisted in the `facts`
table and unresolved references in `refs`. Editing one file re-parses one file and
re-resolves every reference from a hash table.

This is why an unchanged reindex still costs real time, and it is a deliberate trade:
correctness of the invariant over speed of the common case. Making these stages
incremental without breaking the invariant is the open performance problem, not a
tuning knob.

Asserted by `index::tests::incremental_indexing_equals_a_full_rebuild`.

## Why SQLite and not a graph database

At Reify's scale — roughly 10^5 nodes and 10^6 edges — a covering index on
`edges(src, kind)` outperforms a graph engine's traversal machinery. SQLite also gives
transactions, FTS5 and a single copyable file. A graph database would add an
operational dependency to buy nothing measurable.

`query/impact` runs in ~200µs on the fixture. If traversal ever becomes the bottleneck,
that is when to revisit — and not before.

## Why tree-sitter and not language servers

Error tolerance. Mature repositories contain files that do not fully parse, and an
extractor that fails on them indexes nothing. tree-sitter recovers and yields what it
can, which is why `a_file_that_does_not_fully_parse_still_yields_what_it_can` is a test.

Call resolution is a *heuristic* and says so in the data: every produced edge carries a
confidence derived from how many candidates the name matched, and a call never crosses
a language boundary.

## Data flow for `reify context`

```
task
 └─▶ term extraction        identifier splitting, inflection folding, stopwords
 └─▶ seeding                FTS5 bm25 × question coverage × path affinity
 └─▶ spread                 2 hops, typed edge weights, decay
 └─▶ selection              per-kind count and token caps, then value-per-token
 └─▶ conflict overlay       contradictions are never dropped for budget reasons
 └─▶ reading plan           precise spans, funded from the same budget
```

The budget governs the whole answer: the context output **plus** every span the reading
plan recommends. Budgeting only the output would be a lie by omission.
