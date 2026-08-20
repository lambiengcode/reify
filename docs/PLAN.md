# Reify — Master Product & Engineering Plan

**Status:** plan only. No code has been written. This document is the contract another
engineer or coding agent executes phase-by-phase.

**Document version:** 1.0 · **Date:** 2026-08-20

---

## Table of contents

| § | Section |
|---|---------|
| A | [Executive summary](#a--executive-summary) |
| B | [Pain points](#b--pain-points) |
| C | [Target users](#c--target-users) |
| D | [Product thesis and its falsifiable form](#d--product-thesis-and-its-falsifiable-form) |
| E | [Competitive analysis](#e--competitive-analysis) |
| F | [Product architecture](#f--product-architecture) |
| G | [Knowledge model](#g--knowledge-model) |
| H | [Indexing pipeline](#h--indexing-pipeline) |
| I | [Query engine](#i--query-engine) |
| J | [Agent integration](#j--agent-integration) |
| K | [Multilingual architecture](#k--multilingual-architecture) |
| L | [Incremental knowledge compilation](#l--incremental-knowledge-compilation) |
| M | [Privacy and security model](#m--privacy-and-security-model) |
| N | [Benchmark](#n--benchmark) |
| O | [MVP scope](#o--mvp-scope) |
| P | [Rust architecture](#p--rust-architecture) |
| Q | [Performance targets](#q--performance-targets) |
| R | [Testing strategy](#r--testing-strategy) |
| S | [Repository structure](#s--repository-structure) |
| T | [Implementation roadmap](#t--implementation-roadmap) |
| U | [First vertical slice](#u--first-vertical-slice) |
| V | [Benchmark execution plan](#v--benchmark-execution-plan) |
| W | [README strategy](#w--readme-strategy) |
| X | [Risks](#x--risks) |
| Y | [Kill criteria](#y--kill-criteria) |
| Z | [Open decisions for the product owner](#z--open-decisions-for-the-product-owner) |

---

## A — Executive summary

Reify is a local-first Rust knowledge engine that compiles a mature codebase — its source,
SQL, tests, configuration, business documents and Git history — into a compact, queryable
**system model**, and serves that model to AI coding agents as *minimum sufficient context*.

The problem it attacks is not model intelligence. On an isolated, well-scoped task a modern
coding agent is excellent. On a ten-year-old business system it degrades, because the
knowledge required to make a correct change is not in any one file. It is smeared across a
BRD nobody has opened since 2019, a `CUSTOMER_GROUP = 7` magic number, a conditional added
by a since-departed engineer, and a Vietnamese requirements document the code never
referenced. The agent cannot read all of it, so it reads the wrong subset, confidently.

Reify's answer is a compiled knowledge layer with a hard ordering: **deterministic first,
semantic second, LLM last.** Symbols, call edges, SQL table access, document structure,
commit lineage and terminology mappings are extracted deterministically and stored in a
single embedded database. Retrieval is lexical and graph-based. An LLM is invoked only for
genuine synthesis, and every LLM-derived artifact is cached, hashed, versioned, and labelled
as *inferred* rather than *confirmed*.

Its flagship command is not search. It is `reify context "<task>"`: given a natural-language
change request, emit the smallest set of business rules, code locations, workflows,
documents, historical decisions and known contradictions an agent needs — as *assertions with
precise citations*, not as pasted file contents. The agent then reads forty lines instead of
forty files.

Everything runs offline on the developer's machine. No source code leaves it.

The project lives or dies on one measurement: the **Reify Brownfield Benchmark**, run against
a real, large, business-heavy open-source system (ERPNext + Frappe), with budget-matched
baselines, an oracle-context ceiling, a negative control, and published raw results. If the
benchmark does not show it, the README does not claim it.

*(291 words)*

---

## B — Pain points

These are the concrete failures Reify targets. Each one is a benchmark task in §N.

**B1. The agent does not know which rule the code implements.**
`if (customer.group == 7 && amount > 50_000_000) requireApproval(2)` is meaningless without
knowing that group 7 is a strategic account and that the threshold came from BRD-42 §4.2.
An agent asked to "change the approval threshold" edits the number and misses the three
other places the same rule is duplicated.

**B2. Blast radius is invisible.**
A change to `OrderService.applyDiscount` may be read by a batch job, a scheduled report, an
ERP export and a stored procedure. `grep` finds the direct callers. It does not find the
SQL that reads the column the method writes, nor the integration whose contract depends on
the value's range.

**B3. Documentation and code disagree, silently.**
On a mature system the documentation is usually *partly* right. The dangerous mode is not
"docs are missing" — the agent handles that by reading code. It is "docs are confidently
wrong", because the agent trusts them and implements the documented behaviour, regressing
the actual behaviour.

**B4. The reason for a condition is in history, not in the file.**
A guard clause that looks redundant was added to fix a production incident. The commit
message says so. The code does not. Agents delete it.

**B5. Business vocabulary and code vocabulary are different languages — sometimes literally.**
The BA says *khách hàng chiến lược*; the code says `StrategicAccount`; the database says
`CUSTOMER_GROUP = 7`; the API says `tier: "S"`. Lexical search connects none of these. An
agent given a requirement in Vietnamese cannot find the code at all.

**B6. Context is spent on the wrong things.**
Left alone, an agent on a large repo burns most of its budget on exploration: listing
directories, opening files to discover they are irrelevant, re-reading the same file across
turns. The tokens that matter — the four business rules and the one historical decision —
are a rounding error in the transcript.

**B7. Onboarding is a six-week oral tradition.**
The same knowledge gap costs human engineers weeks. It is transmitted by interrupting the
one senior developer who remembers.

**B8. Repeated cost.**
Every agent session rediscovers the same structure from scratch. There is no artifact that
persists what the previous session learned about the *system* (as opposed to the task).

---

## C — Target users

**P1 — "The maintainer with an agent" (primary; the persona the product is optimised for).**
A developer on a 5–15 year old business system (ERP, banking, insurance, logistics,
healthcare, government, long-running SaaS) who already uses Claude Code, Codex, Cursor,
OpenCode or Pi daily. Their complaint is specific and repeated: *"it's brilliant on new code
and unreliable on ours."* They already pay for tokens and feel the waste. They will install
a CLI if it takes under five minutes and shows value on the first command.
**Acquisition wedge:** `reify why <line>` on their own repo, before they read the docs.

**P2 — The newcomer.** Joined an existing team; needs architecture, vocabulary, workflows,
dangerous areas and history. Uses `reify explain`, `reify flow`, `reify report`.
**Value:** onboarding time. **Note:** valuable but not the wedge — they do not choose tools.

**P3 — The staff engineer / architect.** Owns impact analysis and system archaeology, is
asked "what breaks if we change this" weekly, and currently answers it by reading code for
an afternoon. Uses `reify impact`, `reify conflicts`, `reify report`.
**Value:** they are the ones who will *champion* adoption internally.

**P4 — The team with poor documentation.** Reify must reconstruct knowledge from code, SQL,
tests and history when the documents are absent, stale or contradictory. This is the default
case, not the degraded case: **the design must never assume documentation exists.**

**P5 — Agent-runtime and tooling authors.** MCP client authors and custom agent builders who
want a knowledge backend rather than building their own. Secondary, but they are the path to
Reify becoming infrastructure rather than a tool.

**Explicit non-users (v1):** greenfield projects (nothing to reify), repositories under
~20k LOC (grep is genuinely sufficient — say so in the README), and teams that want a hosted
service (Reify is local-first by construction).

---

## D — Product thesis and its falsifiable form

### The thesis

> A deterministically-compiled semantic model of a software system can reduce the context an
> AI coding agent consumes **and** increase the correctness of the changes it makes on
> brownfield systems.

Two claims, joined by "and". The "and" is the hard part. Reducing context alone is trivial
(give the agent less). Increasing correctness alone is trivial (give the agent everything).
Doing both is the product.

### Why it might be true

1. Agent exploration on a large repo is mostly *negative* work — opening files to rule them
   out. That work is deterministically precomputable and reusable across sessions.
2. The knowledge that decides correctness (which rule, which invariant, which historical
   fix) is small and structured. It compresses far better than source code.
3. Citation-shaped context — "rule R184, evidence `ApprovalService.java:812` and `BRD-42 §4.2`"
   — lets the agent *verify* by reading a few precise lines, instead of *guessing* from a
   broad sample.

### Why it might be false (take this seriously)

1. **Frontier models may already be good enough at search.** Agents with strong agentic
   search may find the right files nearly as often, and the marginal value of precompiled
   knowledge shrinks each model generation. If so, Reify's value is cost/latency, not
   correctness — a smaller product.
2. **Extraction quality may cap the value.** If business-rule extraction is 60% precise, the
   agent may be *worse off* than with raw code, because a wrong rule stated confidently is
   more damaging than no rule.
3. **The bottleneck may be reasoning, not retrieval.** If a perfect hand-written context
   still yields failures, context engineering has a low ceiling on these tasks.
4. **Selection may be the hard part.** Compiling knowledge is worthless if we cannot pick the
   right subset for an unseen task.

### The falsification experiments (designed *before* building)

These are first-class benchmark conditions, not afterthoughts. Each has a pre-registered
outcome that would weaken or kill the thesis.

| Experiment | What it tests | Result that damages the thesis |
|---|---|---|
| **E1 — Budget-matched grep** | Baseline B gets `ripgrep` + read-file with a token budget *equal to Reify's* | Baseline B matches Reify success rate → the win was budget discipline, not knowledge |
| **E2 — Oracle context ceiling** | Hand-written perfect context per task, given to the same agent | Oracle success rate barely exceeds Baseline A → context is not the bottleneck; **kill or pivot** |
| **E3 — Negative control** | Reify context with claims shuffled between tasks | Reify-shuffled ≈ Reify-real → the agent is not using the content, only the framing |
| **E4 — Model sweep** | Same conditions on ≥2 model families, incl. one weaker/cheaper | Gain vanishes on the stronger model → Reify is a crutch for weak models, shrinking market |
| **E5 — Extraction ablation** | Reify with rules removed, keeping only graph + history | Ablated ≈ full → the LLM-assisted rule extraction is not earning its cost; simplify |
| **E6 — Memorisation control** | Ask the agent the task with **no repository access at all** | High success from memory alone → task is contaminated; discard and rebuild from post-cutoff commits |

E2 and E6 are the two that can end the project early, which is exactly why they run in the
first vertical slice (§U) rather than at the end.

---

## E — Competitive analysis

Reify sits in a crowded neighbourhood. The honest position is that **most of the components
already exist and should be reused**; almost nothing assembles them into a *task-scoped,
budget-aware, citation-carrying context compiler for agents.*

### E.1 Landscape

| Category | Representative work | What it does well | What it does not do | Reify's stance |
|---|---|---|---|---|
| **Lexical search** | ripgrep, Zoekt, ast-grep, Semgrep | Extremely fast, exact, zero setup, trusted | No cross-file semantics, no docs, no history, no ranking for relevance-to-a-task | **Reuse the idea, not the tool.** Lexical retrieval is a *stage*, never the answer |
| **Code intelligence / xrefs** | LSIF, SCIP (`scip-java`, `scip-typescript`, `scip-python`), Kythe, Glean, Sourcegraph, stack-graphs | Precise, type-aware cross-references at scale | Code-only. No business rules, no documents, no task scoping. Heavy toolchains | **Consume, don't rebuild.** Ingest SCIP when present as a precision upgrade over heuristic resolution (§H.4) |
| **Repository maps for agents** | Aider's repo-map, `repomix`, `gitingest`, `code2prompt`, DeepWiki | Cheap, effective, model-agnostic; proved that *structure beats raw text* | Uniform, task-agnostic compression. No provenance, no documents, no history, no conflicts. Grows with the repo | **Nearest prior art and the real baseline.** Reify is the task-scoped, evidence-carrying successor. Benchmark against it |
| **Code RAG / semantic search** | Cursor codebase indexing, Cody, Continue, generic embed-and-retrieve stacks | Recall on vague queries; cheap to build | Chunk-level, unciteable, non-deterministic, embedding-drift, weak on identifiers/numbers, no relationships | **Explicit Baseline C.** Reify uses embeddings only as a fallback ranker, never as the store |
| **Agent-facing code servers** | Serena, `mcp-language-server`, GitMCP, various MCP code servers | Give agents LSP-grade navigation through a protocol | Navigation primitives, not knowledge; agent still drives exploration and pays for it | Same primitives, but **precompiled and pre-selected** — one call, not thirty |
| **Software archaeology / rule mining** | `git-of-theseus`, CodeScene, academic business-rule extraction & program-comprehension literature | Hotspots, coupling, decay, rule-mining formalisms | Research tools or dashboards for humans; not agent-consumable, not local-first CLIs | Reuse the *metrics* (co-change, hotspot, blame lineage). Reject the dashboard framing |
| **Legacy modernisation suites** | Vendor "AI mainframe/legacy" platforms | Deep domain services, real enterprise value | Cloud-only, source upload, expensive, closed, consultant-led | **Direct inverse positioning:** local-first, open-source, free, ten-minute install |
| **Coding agents** | Claude Code, Codex, Cursor, OpenCode, Pi, Aider | Everything about editing, planning, executing | The knowledge substrate | **Never compete. Always feed.** Reify has no editor, no agent loop, no model |
| **Compact agent CLIs (AXI-style)** | `gh-axi`, `lavish-axi`, `quota-axi` and similar | Terse, composable, low-token CLI surfaces that agents call directly from the shell — no tool schemas to load, no protocol tax | Domain-specific by design | **Adopt the interface philosophy wholesale** (§J): CLI-first, agent-shaped output, ≤3 verbs surfaced by default |

### E.2 What Reify explicitly will NOT build

- A vector database. (SQLite blobs + brute-force SIMD scan covers our scale; §P.2)
- A graph database. (An edge table with the right indexes is faster at this size; §P.2)
- Language servers, type checkers, or a general program analyser. (tree-sitter + optional SCIP)
- Its own PDF/DOCX rendering stack. (Delegate; degrade gracefully; §H.2)
- An agent, an editor, a chat UI, or a hosted service.
- A general-purpose RAG framework.

### E.3 The one-sentence differentiator

Everything above answers *"where is the code?"*.
Reify answers **"what must you know before you change this, why do you believe it, and what is the smallest form of that answer?"**

---

## F — Product architecture

### F.1 The shape of the system

```
  SOURCES                    COMPILER                 STORE              SERVING
  ───────                    ────────                 ─────              ───────

  source code ──┐      ┌──────────────────┐
  SQL / DDL     │      │  extractors      │
  tests         ├─────▶│  (deterministic) │──┐
  configs       │      └──────────────────┘  │
                │                            │
  markdown      │      ┌──────────────────┐  │      ┌──────────┐     ┌───────────┐
  PDF / DOCX    ├─────▶│  doc pipeline    │──┼─────▶│  facts   │     │  CLI      │
  HTML / CSV    │      └──────────────────┘  │      │  (nodes  │     │  reify …  │
  Confluence    │                            │      │   edges  │────▶│  --json   │
  Jira export   │      ┌──────────────────┐  │      │   evid.) │     └───────────┘
                │      │  git archaeology │──┤      │          │           │
  git history ──┘      └──────────────────┘  │      │ SQLite   │     ┌───────────┐
                                             │      │ + FTS    │────▶│  MCP      │
                       ┌──────────────────┐  │      └────┬─────┘     │  (thin)   │
                       │  concept linker  │◀─┘           │           └───────────┘
                       │  (glossary +     │──────────────┘                 │
                       │   heuristics +   │                          ┌───────────┐
                       │   optional LLM)  │                          │  hooks    │
                       └──────────────────┘                          │ preflight │
                                │                                    └───────────┘
                       ┌────────┴─────────┐
                       │  rule + conflict │
                       │  synthesis       │
                       └──────────────────┘
                                │
                                ▼
                    ┌───────────────────────┐
                    │  CONTEXT COMPILER     │  ← the product
                    │  task → minimal,      │
                    │  cited, budgeted set  │
                    └───────────────────────┘
```

### F.2 Layered model, with a strict cost ordering

```
  LAYER 4   Synthesis        LLM, optional, cached, always labelled INFERRED
            ────────────────────────────────────────────────────────────────
  LAYER 3   Selection        context compilation: seed → spread → knapsack
            ────────────────────────────────────────────────────────────────
  LAYER 2   Semantics        concepts, terminology, rules, conflicts, workflows
            ────────────────────────────────────────────────────────────────
  LAYER 1   Structure        symbols, calls, imports, SQL access, doc sections,
                             commits, blame, co-change
            ────────────────────────────────────────────────────────────────
  LAYER 0   Substrate        content-addressed files, hashes, spans, SQLite
```

**The cost ordering is enforced in code, not by convention.** The query planner (§I) has an
explicit escalation gate: a query may only reach Layer 4 if Layers 1–3 returned below a
confidence/coverage threshold, and only if the user has enabled LLM use. `REIFY_OFFLINE=1`
makes Layer 4 unreachable and every command still works.

### F.3 Data-flow for the flagship command

```
reify context "Add a 15% discount for strategic enterprise customers"
   │
   ├─▶ 1. TERM EXTRACTION      tokenise, split identifiers, strip stopwords (per language)
   │
   ├─▶ 2. CONCEPT RESOLUTION   glossary lookup → concept ids (deterministic, multilingual)
   │                           fallback: lexical index → candidate concepts
   │
   ├─▶ 3. SEED SET             concepts + directly-matching symbols/docs/rules, with scores
   │
   ├─▶ 4. GRAPH SPREAD         bounded personalised-PageRank over typed edges
   │                           (edge weights per type; depth cap; visit cap)
   │
   ├─▶ 5. EVIDENCE JOIN        attach citations + epistemic status + confidence to each node
   │
   ├─▶ 6. BUDGET SELECTION     knapsack: maximise Σ(relevance × confidence) s.t. Σtokens ≤ B
   │
   ├─▶ 7. CONFLICT OVERLAY     any CONTRADICTS edge touching the selected set is forced in
   │                           (a conflict is never dropped for budget reasons)
   │
   └─▶ 8. RENDER               human (terminal) or --json (agent), assertions + citations
```

Step 7 is a deliberate safety asymmetry: budget pressure may drop useful context, but never
a known contradiction.

---

## G — Knowledge model

### G.1 Design decision: one node table, one edge table

Rejected: a property-graph database; a per-entity-type table schema; RDF/triples.
Chosen: a **typed node table + typed edge table in SQLite**, with type-specific payloads in
a JSON column and hot fields promoted to real columns for indexing.

Rationale: at our scale (an 8k-file repo yields roughly 300k–800k nodes and 1–3M edges) a
covering index on `(src, kind)` beats a graph database's traversal overhead, gives us
transactions and a single-file store for free, and keeps the whole thing `rsync`-able.
Adding a graph DB would add an operational dependency to buy nothing measurable. Revisit
only if a benchmark shows traversal is the bottleneck.

### G.2 Node kinds

```
STRUCTURAL          SEMANTIC              NARRATIVE          SOURCE
──────────          ────────              ─────────          ──────
File                Concept               Decision           Document
Module              Term                  Change             DocSection
Symbol              BusinessRule          Commit             Evidence
  (fn/class/        Invariant             Conflict
   method/type/     Workflow
   interface/       WorkflowStep
   field)
DatabaseObject      Integration
  (table/column/
   view/procedure)
ApiEndpoint
ConfigKey
Test
```

`Evidence` is a node, not an attribute. It is shared, deduplicated, and independently
addressable — so `reify why` can answer "which claims rest on BRD-42 §4.2?" and, when a
document changes, we invalidate exactly the claims it supports (§L).

### G.3 Edge kinds

```
CODE STRUCTURE         KNOWLEDGE                  HISTORY            LANGUAGE
──────────────         ─────────                  ───────            ────────
CALLS                  IMPLEMENTS_RULE            INTRODUCED_BY      MAPS_TO
IMPORTS                DOCUMENTED_BY              CHANGED_BY         ALIAS_OF
INHERITS               PART_OF_WORKFLOW           REVERTED_BY        TRANSLATES_TO
REFERENCES             EXCEPTION_TO               CO_CHANGES_WITH
READS                  CONTRADICTS
WRITES                 CONSTRAINS   (invariant → symbol)
EXPOSES  (sym→api)     AFFECTS      (change → anything)
DEPENDS_ON             RELATED_TO   (weakest; requires a reason string)
TESTED_BY
```

Every edge carries `confidence`, `status`, and `evidence_ids`. `RELATED_TO` requires a
non-empty `reason` — without that constraint it becomes a dumping ground and destroys
selection precision.

### G.4 Epistemic status — the safety core

Every node and edge carries exactly one status. This is the single most important field in
the model and it is **non-nullable**.

| Status | Meaning | Produced by | May an agent act on it unverified? |
|---|---|---|---|
| `CONFIRMED` | Directly present in a source artifact | Parser, git, DDL, human confirmation | Yes |
| `OBSERVED` | Derived by deterministic analysis from confirmed facts | Call resolution, co-change, SQL access | Yes, with the cited location |
| `INFERRED` | Produced by heuristic or LLM synthesis | Rule extraction, concept proposal | **No — verify against evidence first** |
| `ASSUMED` | Default applied in absence of evidence | Fallback resolvers | No — flagged in output |
| `CONFLICTED` | Two ≥threshold sources disagree | Conflict detector | **No — resolve first, and say so loudly** |
| `UNKNOWN` | Explicitly unresolved; recorded so it is not silently omitted | Any stage | No |

Output renderers **must** surface status. A CLI that prints an `INFERRED` rule as bare prose
is a bug with a regression test attached (§R). The agent-facing JSON puts `status` before
the claim text, because models attend to what comes first.

### G.5 Provenance and confidence

```json
{
  "id": "rule:R184",
  "kind": "BusinessRule",
  "status": "INFERRED",
  "confidence": 0.93,
  "title": "Corporate orders above 50M VND require L2 approval",
  "conditions": [
    {"subject": "order.customer.group", "op": "==", "value": "CORPORATE"},
    {"subject": "order.total", "op": ">", "value": 50000000, "unit": "VND"}
  ],
  "actions": [{"verb": "require", "object": "approval", "level": 2}],
  "exceptions": ["rule:R227"],
  "concepts": ["concept:CORPORATE_CUSTOMER", "concept:L2_APPROVAL"],
  "evidence": [
    {"id": "ev:8f1a", "source": "docs/BRD-42.pdf", "locator": "§4.2 p.17",
     "lang": "en", "kind": "document", "quote_hash": "sha256:…"},
    {"id": "ev:2c07", "source": "erpnext/selling/order.py", "locator": "812-829",
     "lang": null, "kind": "code", "blob_hash": "sha256:…"}
  ],
  "provenance": {
    "extractor": "rules/llm@v3",
    "model": "claude-opus-5",
    "prompt_version": "rules-extract-v3",
    "input_hash": "sha256:…",
    "created_at": "2026-08-20T09:12:44Z"
  }
}
```

Invariants enforced by schema + tests:

1. `status != CONFIRMED` ⟹ `evidence` is non-empty. Nothing inferred exists without a trail.
2. `confidence` is a **calibrated** number, not a vibe. It is produced by an explicit,
   documented formula per extractor (§H.6), and its calibration is *measured* against the
   hand-labelled benchmark set. An uncalibrated confidence field is worse than none.
3. `provenance.input_hash` is what makes the artifact cacheable and invalidatable (§L).
4. Evidence stores a `quote_hash`, not the quoted text, unless the user opts into quote
   storage — so the store never becomes a second copy of proprietary documents.

### G.6 Conflicts, modelled conservatively

A `Conflict` node is created only when **all** of the following hold:

1. Two claims resolve to the **same concept** and the **same action verb**.
2. Their polarity is opposite (`require` vs `bypass`, `allow` vs `deny`).
3. Both claims are ≥ `conflict_min_confidence` (default 0.75).
4. They come from **different source kinds** (document vs code, or code vs code in
   different modules) — same-file disagreement is usually a branch, not a conflict.
5. No `EXCEPTION_TO` edge already explains the divergence.

Anything failing these becomes a `Divergence` observation, kept in the store but **not**
surfaced by `reify conflicts` unless `--include-weak` is passed.

This is deliberately biased toward silence. A conflict detector that cries wolf gets
disabled in week two, and then its true positives are lost too. Precision target and
measurement procedure: §N.6.

---

## H — Indexing pipeline

Ten stages. Stages 1–7 are deterministic and run always. Stages 8–9 are optional. Stage 10
is always deterministic.

```
 1 DISCOVER    2 CLASSIFY    3 PARSE      4 RESOLVE     5 SQL
   walk +        lang/type     tree-sitter  imports+      queries,
   .reifyignore  detect        → symbols    calls, refs   tables, DDL
      │             │             │            │            │
      └─────────────┴─────────────┴────────────┴────────────┘
                              │
 6 DOCUMENTS   7 GIT          │      8 CONCEPTS      9 RULES
   extract →     commits,     │        glossary +      candidate
   sections      blame,       │        heuristics      mining →
      │          co-change    │        (+LLM opt)      (+LLM opt)
      └──────────────┴────────┴────────────┴──────────────┘
                              │
                        10 LINK + INDEX
                        edges, conflicts, FTS, stats, manifest
```

### H.1 Stage 1–2: discover and classify

Parallel `ignore`-crate walk honouring `.gitignore` and `.reifyignore`. Each file gets a
content hash (blake3 — fast, and we need hashing on the hot path for §L). Classification by
extension, shebang, and a cheap content sniff. Binary and generated files (`node_modules`,
`dist`, lockfiles, minified bundles, vendored trees) are excluded by default; the default
exclusion list is *printed at `reify init`* so it is visible rather than magic.

### H.2 Stage 3: parse

**tree-sitter**, one grammar per language, extraction driven by declarative `.scm` query
files. This choice is load-bearing: it is error-tolerant (mature codebases do not always
parse cleanly), incremental, needs no per-language toolchain, and adding a language becomes
"add a grammar + write a query file" rather than "write a compiler front-end".

MVP languages, ordered by benchmark need: **Python, JavaScript/TypeScript, SQL**, then
**Java**, then **Go, C#, Rust**.

Documents:

| Format | Approach | Dependency |
|---|---|---|
| Markdown, text | `pulldown-cmark` + heading tree | pure Rust |
| HTML (incl. Confluence export) | `scraper`/`html5ever`, heading tree | pure Rust |
| CSV / JSON / YAML / XML | native parsers; treat as structured records | pure Rust |
| DOCX | unzip + parse `document.xml` (headings, paragraphs, tables) | `zip` + `quick-xml` |
| PDF | shell out to `pdftotext -layout` when present; else `pdf-extract` fallback; else **skip and report loudly** | external, optional |
| Jira/Confluence exports | format-specific adapters over JSON/HTML | — |

PDF is the weak link and the plan says so. `reify status` reports "N PDFs skipped: pdftotext
not found" rather than silently indexing nothing. Scanned/OCR PDFs are out of scope for v1.

### H.3 Stage 4: resolve

Import graph first, then name resolution scoped by it. This is a **heuristic** resolver and
is labelled as such:

- Unique name in scope → `CALLS` edge, `status=OBSERVED`, confidence 0.95.
- Ambiguous (N candidates) → N edges at confidence `1/N`, capped at `N ≤ 5`; above that,
  record `UNKNOWN` rather than N low-value edges.
- Dynamic dispatch / reflection / string-built calls → recorded as `UNKNOWN` with the site.

**Precision upgrade path:** if `index.scip` (or `dump.lsif`) exists, ingest it and let its
edges *override* heuristic ones at confidence 0.99. We do not build a type checker; we
consume one when the team already runs it. This is the single highest-leverage "don't
reinvent" decision in the plan, and it is cheap: SCIP is a documented protobuf schema.

### H.4 Stage 5: SQL and data

Parse SQL literals in source, `.sql` files, migrations, ORM model definitions and stored
procedures. Extract `READS` / `WRITES` edges between symbols and `DatabaseObject` nodes, plus
DDL-derived tables, columns, constraints and enum-like value sets.

Enum/constant value sets matter disproportionately: they are how `CUSTOMER_GROUP = 7` becomes
attachable to a concept (§K).

### H.5 Stage 6–7: documents and git

Documents: section tree, per-section language detection (§K.1), stable section ids
(`doc:BRD-42#4.2`), and a `DocSection` node per leaf with a byte span.

Git (via `gix`, pure Rust, fast history walking):

- `INTRODUCED_BY` per symbol via first-appearance in diff hunks touching its span.
- `CHANGED_BY` per symbol via subsequent hunk overlap.
- Blame at line granularity, computed lazily and cached (blame is the expensive operation;
  a full-repo eager blame is a non-goal).
- Co-change: files/symbols appearing together in commits, with lift scoring; keeps only
  pairs above a support threshold.
- Commit classification: fix / feature / refactor / revert, from conventional-commit
  prefixes, issue-reference patterns, and revert detection. Deterministic, no LLM.
- `Decision` nodes are mined from commits whose message exceeds a length threshold and
  matches decision language, plus ADR files if present.

**Bounded by default:** `--since` defaults to the full history for repos under a commit
threshold and to the last 5 years above it, with the cutoff printed. A 15-year monorepo must
not make `reify index` a 40-minute operation on first run.

### H.6 Stage 8–9: concepts and rules (the optional, LLM-touching stages)

**Concepts (Stage 8)** — three tiers, in precision order:

1. **Declared** (`CONFIRMED`): `.reify/glossary.toml`, human-authored. Highest value per
   byte in the entire system; `reify init` scaffolds it and `reify concepts --suggest`
   feeds it.
2. **Mined** (`OBSERVED`): identifier splitting, enum/constant literals, doc heading terms,
   co-occurrence between doc sections and symbols, translation-file pairs (§K.2).
3. **Proposed** (`INFERRED`): optional LLM pass over unresolved high-frequency terms.
   Cached by input hash. Never auto-promoted; `reify concept confirm <id>` promotes it and
   writes it back into the glossary — so the human's effort is durable and version-controlled.

**Rules (Stage 9)** — candidate generation is deterministic; *phrasing* is where an LLM may help.

Deterministic candidate sources:
- Guard clauses and validation branches that reference concept-mapped symbols.
- Explicit exception/error raises with domain messages.
- Constraint DDL (`CHECK`, `NOT NULL`, unique), and enum domains.
- Test names and assertions (`test_corporate_order_requires_l2_approval` is a rule, spelled out).
- Configuration keys with domain names and thresholds.
- Document sentences matching modal patterns (`must`, `shall`, `phải`, `không được`, `する必要がある`, …).

Confidence formula (documented, deterministic, calibrated against the labelled set):

```
confidence = w_src · source_strength      (test 0.9, DDL 0.9, code 0.8, doc 0.7, commit 0.5)
           × w_agr · agreement_factor      (1.0 single source; 1.15 capped, per corroborating
                                            independent source kind)
           × w_cpt · concept_resolution    (1.0 all terms resolved → 0.6 none resolved)
           × w_rec · recency_factor        (code touched in last 2y: 1.0; >5y untouched: 0.9)
```

Calibration is checked, not assumed: on the labelled benchmark set, bucket predictions by
confidence decile and plot observed precision. If the curve is not monotone, the formula is
wrong and gets fixed — this is a required deliverable of Phase 4, not a nicety.

### H.7 Stage 10: link, index, manifest

Materialise cross-source edges, run the conflict detector (§G.6), build the FTS index, write
per-node token-cost estimates (needed by the knapsack in §I), compute `reify report` stats,
and write a manifest recording tool version, schema version, source commit, extractor
versions, and per-stage input hashes.

**Determinism requirement:** two `reify index` runs on the same commit with the same config
and no LLM produce byte-identical stores modulo timestamps. A CI test asserts this by hashing
a canonical dump. Without it, incremental correctness (§L) cannot be tested.

---

## I — Query engine

### I.1 Three paths, explicit escalation

```
                 ┌──────────────────────────────────────────────┐
   query ───────▶│ PLANNER: classify intent, extract terms      │
                 └───────────────┬──────────────────────────────┘
                                 │
        ┌────────────────────────┼────────────────────────┐
        ▼                        ▼                        ▼
  ┌───────────┐           ┌────────────┐           ┌────────────┐
  │ PATH 1    │           │ PATH 2     │           │ PATH 3     │
  │ DETERMIN. │  ── miss ▶│ SEMANTIC   │  ── miss ▶│ LLM        │
  │ <20 ms    │           │ <100 ms    │           │ opt-in     │
  └───────────┘           └────────────┘           └────────────┘
   exact symbol            lexical BM25             synthesis of
   glossary term           fuzzy identifier         retrieved facts
   file:line               concept expansion        ambiguity resolution
   graph traversal         graph spread             narrative explanation
   git lookup              embedding rerank (opt)
```

**Escalation is gated, logged, and refusable.** Path 2 runs only if Path 1's top result
scores below `t1`. Path 3 runs only if Path 2 scores below `t2` **and** `llm.enabled = true`
**and** the command is not in offline mode. Every escalation appends to `.reify/llm.log` with
the input hash, so a user can audit exactly when and why a model was called.

Crucially, **Path 3 never invents; it only phrases.** Its prompt receives retrieved facts and
is constrained to synthesise over them, and its output is stored as `INFERRED` with the
retrieved facts as evidence. If Path 3 produces a claim with no supporting retrieved fact,
that is a bug caught by a test in §R.

### I.2 Command surface

Design principle taken from AXI-style tools: **few verbs, machine-readable by default, dense
output.** Every command supports `--json` (stable schema, versioned) and `--budget <tokens>`.

| Command | Purpose | Path |
|---|---|---|
| `reify init` | Create `.reify/`, detect languages/docs, scaffold `glossary.toml`, print what will be indexed | — |
| `reify index [--since] [--llm]` | Compile the system model | 1–2 |
| `reify status` | Freshness, coverage, skipped files and *why*, staleness vs HEAD | 1 |
| `reify context "<task>"` | **The product.** Minimal cited context for a change | 1–2 (3 opt) |
| `reify why <file:line \| symbol>` | Rule, evidence, history, blast radius for a location | 1 |
| `reify impact "<change>"` | Affected symbols, workflows, integrations, invariants, tests | 1 |
| `reify explain "<concept>"` | Concept across languages/code/DB/docs | 1–2 |
| `reify flow "<process>"` | Workflow steps with implementing symbols | 1 |
| `reify conflicts [--include-weak]` | Documented vs observed divergences | 1 |
| `reify report` | System-level scorecard (§W) | 1 |
| `reify serve --mcp` | Thin MCP wrapper over the same core | — |

Deliberately **not** in v1: `reify chat`, `reify ask`, `reify fix`, `reify search`.
`search` is omitted on purpose — `ripgrep` exists and is better. Reify should not offer a
worse version of a tool the user already has, and offering one invites the comparison
"Reify's search is slower than rg", which is both true and irrelevant.

### I.3 Output contract for agents

The agent-facing JSON is not a document dump. It is **assertions with coordinates**:

```json
{
  "schema": "reify.context/1",
  "task": "Add a 15% discount for strategic enterprise customers",
  "budget": {"requested": 4000, "used": 1420, "unit": "tokens", "estimator": "heuristic-v1"},
  "concepts": [
    {"id": "concept:STRATEGIC_ACCOUNT", "status": "CONFIRMED",
     "labels": {"en": "strategic account", "vi": "khách hàng chiến lược"},
     "code": ["StrategicAccount"], "db": ["CUSTOMER_GROUP=7"]}
  ],
  "rules": [
    {"id": "rule:R227", "status": "INFERRED", "confidence": 0.88,
     "claim": "Strategic accounts bypass L2 approval",
     "evidence": ["erpnext/selling/order.py:812", "docs/BRD-42.pdf#4.2"]}
  ],
  "code": [
    {"path": "erpnext/accounts/pricing_rule.py", "symbol": "apply_pricing_rule",
     "lines": "88-146", "why": "writes the discount field R227 constrains",
     "status": "OBSERVED"}
  ],
  "history": [
    {"commit": "8a31c2f", "date": "2019-04-17", "subject": "Fix enterprise approval flow",
     "why_relevant": "introduced the bypass branch at order.py:812"}
  ],
  "conflicts": [
    {"id": "conflict:C7", "status": "CONFLICTED",
     "documented": "Corporate customers require approval  (BRD-42 §4.2)",
     "observed": "Corporate customers bypass approval  (order.py:812)",
     "resolution": "UNRESOLVED"}
  ],
  "unknowns": ["No document found describing discount stacking order"],
  "next_reads": [
    {"path": "erpnext/accounts/pricing_rule.py", "lines": "88-146", "est_tokens": 640}
  ]
}
```

Three details that matter more than they look:

- **`next_reads`** turns Reify into a *reading plan*. The agent's next tool call is a
  targeted read of 58 lines, not a directory listing. This is where token savings come from.
- **`unknowns`** is populated deliberately. Stating what we could not determine prevents the
  agent from treating absence as evidence of absence.
- **`budget.estimator`** is named so benchmark numbers can be traced to how they were counted.

---

## J — Agent integration

### J.1 Design stance

Study of compact agent CLIs (`gh-axi`, `lavish-axi` and peers) yields one lesson worth more
than the rest: **a CLI the agent invokes from the shell costs zero tokens until it is used,
while an MCP server costs its tool schemas on every single turn of every session.** A
fifteen-tool MCP server can burn more context than it saves.

Therefore: **CLI-first. MCP is a thin optional wrapper, not the primary surface.**

### J.2 The four integration levels, cheapest first

**Level 0 — Zero integration.** The agent runs `reify context "..."` in Bash because the
project's `AGENTS.md` / `CLAUDE.md` tells it to. `reify init` offers to append a 6-line
snippet. Works with Claude Code, Codex, Cursor, OpenCode, Pi, Aider and anything with a
shell — today, with no protocol work at all.

```markdown
<!-- appended by `reify init` -->
## Before changing code in this repo
Run `reify context "<what you are about to do>"` and read its output first.
Run `reify why <file>:<line>` before modifying unfamiliar logic.
Treat `status: INFERRED` claims as leads to verify, not as facts.
```

**Level 1 — MCP, three tools maximum.** `reify_context`, `reify_why`, `reify_impact`.
That is the entire surface. Three tools is roughly 400 tokens of schema; fifteen is 2000+ and
would undercut the product's own thesis.

**Level 2 — Preflight hook.** A `PreToolUse` hook on `Edit`/`Write` that runs
`reify preflight <path>` and injects a compact risk header:

```
PREFLIGHT  erpnext/selling/doctype/sales_order/sales_order.py
  rules 7  ·  workflows 4  ·  integrations 3  ·  invariants 2  ·  conflicts 1
  highest risk: R184 R227 R311
  suggested: reify context "<your task>"   (~1.2k tokens)
  RISK: HIGH — documentation and implementation disagree on approval bypass
```

Non-blocking by default; `--block-on-conflict` is opt-in for teams that want it. Hooks that
block edits get uninstalled, so blocking must be a choice the user makes deliberately.

**Level 3 — Git hook.** `post-merge` / `post-checkout` running `reify index --incremental`
so the store is never stale. Offered by `reify init`, never installed silently.

### J.3 What we do not do

No agent loop. No file editing. No model calls on the agent's behalf. No background daemon
in v1 (a file watcher is Phase 7 at the earliest, and only if incremental indexing proves too
slow to run on demand — measure first).

---

## K — Multilingual architecture

Treated as core, not localisation. The design rule: **no language is canonical.** Concept
ids are opaque (`concept:STRATEGIC_ACCOUNT` is a symbol, not English), and every label
carries a language tag including English.

### K.1 Language detection

Per document *section*, not per document — mixed-language files are the norm in Asian
enterprise codebases. `whatlang` (pure Rust) for detection, with a script-based fast path for
CJK. Sections below a length threshold inherit the document's dominant language.

### K.2 The four bridges from business term to code

```
   BUSINESS TERM                                             CODE / DATA
   "khách hàng chiến lược"                                  StrategicAccount
        │                                                    CUSTOMER_GROUP=7
        │                                                          ▲
        ├──▶ BRIDGE 1  glossary.toml (declared)  ─────────────────▶│  CONFIRMED
        │                                                          │
        ├──▶ BRIDGE 2  translation files (i18n .po/.csv/.json) ───▶│  OBSERVED
        │              vi.csv: "Strategic Account" → "khách hàng chiến lược"
        │              and "Strategic Account" ↔ StrategicAccount by identifier split
        │                                                          │
        ├──▶ BRIDGE 3  co-occurrence mining  ────────────────────▶│  OBSERVED
        │              doc section ↔ symbol ↔ enum literal, lift-scored
        │                                                          │
        └──▶ BRIDGE 4  LLM proposal (optional, cached)  ─────────▶│  INFERRED
```

**Bridge 2 is the sleeper.** Any application that has been localised already contains a
professionally-produced, human-verified bilingual dictionary of its own domain vocabulary,
sitting in its i18n files. ERPNext ships `vi.csv`, `ja.csv`, `ko.csv`, `zh.csv` and dozens
more, each mapping an English UI string to its translation — and the English string is
usually derived from the DocType/field name, i.e. from an identifier. Mining that yields a
high-quality multilingual concept layer **with no LLM and no embeddings.** It is also the
reason ERPNext is the right benchmark repo (§N.1): the multilingual task is *genuine*, not
synthesised for the demo.

### K.3 Tokenisation and search

Vietnamese: diacritics preserved, plus a folded form indexed alongside (users type both);
syllable-based tokenisation — Vietnamese words are multi-syllable, so bigram indexing over
syllables is required or `khách hàng` never matches as a unit.
CJK: character bigrams (no word segmentation dependency in v1).
Latin: standard unicode tokenisation + identifier splitting (`camelCase`, `snake_case`,
`PascalCase`, `SCREAMING_SNAKE`, digit boundaries).

**Storage decision, deliberately lazy:** start with **SQLite FTS5** using a custom tokenizer
configuration (`unicode61 remove_diacritics 2` plus a trigram index for CJK). Do **not** pull
in Tantivy on day one. Add Tantivy only if a measured recall test on the multilingual
benchmark task shows FTS5 below target — with a written decision record either way. This is
a ~2-day migration if needed and zero cost if not; adopting a second full search engine
speculatively is exactly the over-engineering this plan is meant to avoid.

Cross-lingual retrieval works through the **concept layer**, not through multilingual
embeddings: the Vietnamese query resolves to `concept:STRATEGIC_ACCOUNT` via bridges 1–3, and
retrieval then proceeds in concept space, which is language-neutral. This is why it is fast,
deterministic, and citable — and why a multilingual embedding model is not required.

---

## L — Incremental knowledge compilation

### L.1 The mechanism: hash-keyed derived artifacts

No `salsa`, no incremental-computation framework. A derived-artifact table with input hashes
is enough, and it is inspectable — which matters when incremental results diverge from full
results and you need to find out why.

```
derived(kind, input_hash) → output_blob, output_hash, created_at
```

Each pipeline stage declares its inputs explicitly. `input_hash = blake3(stage_version ‖
config_hash ‖ sorted(input_hashes))`. Cache hit ⟹ skip the stage and reuse the output.

### L.2 Propagation

```
  file changed  (blake3 differs)
        │
        ▼
  reparse file ──▶ symbol hashes recomputed (hash of the symbol's own span + signature)
        │
        ├─ symbol hash unchanged  ──▶ STOP.  (a comment edit propagates nowhere)
        │
        └─ symbol hash changed
                │
                ├──▶ edges FROM symbol      recompute
                ├──▶ edges TO symbol        recompute only if signature changed
                ├──▶ rules citing symbol    re-derive (or re-run extractor if LLM-derived
                │                            and input hash changed)
                ├──▶ concepts via symbol    re-link
                ├──▶ conflicts touching     re-evaluate
                └──▶ FTS entries            update
```

Symbol-level hashing (rather than file-level) is what makes this pay: in a mature repo most
edits touch one function in a 2000-line file, and everything else in that file should be a
cache hit.

Documents propagate the same way at section granularity. Git propagates by walking only the
commits since the last indexed `HEAD`.

### L.3 The correctness guarantee, and how it is tested

**Invariant:** `incremental_index(repo @ commit_N)` ≡ `full_index(repo @ commit_N)`, modulo
timestamps and provenance metadata.

This is enforced by a property test (§R), not by hope:

```
prop_incremental_equivalence:
  given a fixture repo and a random sequence of 1..30 realistic edits
  (add file, delete file, rename file, edit body, edit signature, edit doc,
   add commit, revert commit)
  apply them incrementally, then index the final state fully,
  assert canonical_dump(incremental) == canonical_dump(full)
```

Any divergence is a P0. Without this test, incremental indexing becomes a source of
silent, irreproducible wrongness — which for a tool whose whole pitch is trustworthy
knowledge is fatal.

**Escape hatch:** `reify index --force` does a full rebuild, and `reify status` warns when
the store's schema/extractor versions predate the binary's.

### L.4 Targets

| Operation | Target | Measured in |
|---|---|---|
| Full index, ERPNext+Frappe (~1M LOC) | ≤ 10 min, 8 cores, no LLM | `bench/index` |
| Incremental, 1 file / 1 function edited | ≤ 500 ms | `bench/incremental` |
| Incremental, 50-file merge | ≤ 5 s | `bench/incremental` |
| Rebuild after `git pull` of 200 commits | ≤ 30 s | `bench/incremental` |
| Store size | ≤ 5% of repo working-tree size | `bench/index` |

These are hypotheses to be validated in Phase 1, not promises. If full index lands at 40
minutes, the plan changes (see §X R6) — it does not get reported as 10.

---

## M — Privacy and security model

### M.1 Threat model

| # | Adversary / risk | Mitigation |
|---|---|---|
| T1 | Proprietary source or documents leaked to a model provider | LLM stages **off by default**. `REIFY_OFFLINE=1` makes them unreachable. `reify llm preview <cmd>` prints the exact payload before any call. Every call logged to `.reify/llm.log` with input hash and byte count |
| T2 | Reify silently phones home (telemetry, update checks) | **Zero network calls except explicitly-configured LLM providers.** Enforced by a CI test that runs the full suite with network egress blocked; any connection attempt fails the build |
| T3 | Secrets swept into the knowledge store | Secret-pattern detection at ingest (high-entropy strings, known key formats, `.env`); matched spans stored as `REDACTED` with location only. `reify audit --secrets` lists what was redacted |
| T4 | `.reify/` committed and leaking internals to a public repo | `reify init` writes `.reify/` into `.gitignore` by default and says so. Sharing a store is opt-in via `reify export --team` |
| T5 | Malicious repo content executing during indexing | Reify never executes repo code. No plugin `eval`, no build steps, no test running. tree-sitter parses; it does not run. Grammar `.so` loading is from the binary's own bundled set, not from the repo |
| T6 | Prompt injection via repo content reaching the agent | Knowledge output is **structurally typed** (claims, not free prose) and every claim carries `status` + evidence. A `.md` file saying "ignore previous instructions" enters as a `DocSection`, is not privileged, and cannot become an instruction field. `reify audit --injection` flags imperative-to-assistant patterns in indexed docs |
| T7 | Store tampering (an attacker edits `.reify/db` to feed an agent false claims) | Manifest records schema/tool/extractor versions and a store checksum; `reify status` verifies it. Full integrity against a local attacker is out of scope in v1 and stated as such |
| T8 | Cross-tenant leakage in a monorepo with restricted directories | `.reifyignore` respected; `reify index --scope <path>` limits compilation. No cross-scope edges are emitted out of scope |

### M.2 Guarantees stated plainly (README-grade)

1. Reify makes **no network connection** unless you configure an LLM provider and enable it.
2. Indexing, querying and context compilation work **fully offline**.
3. `reify llm preview` shows exactly what would be sent, before it is sent.
4. LLM-derived artifacts are cached, so the same knowledge is never paid for twice.
5. The store lives in `.reify/` in your repo and never leaves your machine.
6. LLM provider is configurable, including local models (Ollama/llama.cpp-compatible endpoints).

### M.3 The one thing we will not compromise

**Deterministic mode is a supported first-class configuration, not a degraded one.** Every
command must produce useful output with `REIFY_OFFLINE=1`, and the benchmark reports a
column for offline mode. If offline Reify is useless, Reify is an LLM wrapper wearing a Rust
costume, and the product thesis has already failed.

---

## N — Benchmark

### N.1 Repository selection

| Candidate | Size | Domain depth | Docs | History | Multilingual | Local runnability | Verdict |
|---|---|---|---|---|---|---|---|
| **ERPNext + Frappe** | ~700k–1M LOC (Py/JS) | Very high: accounting, inventory, manufacturing, CRM, HR, payroll, taxes | Good user + dev docs, DocType metadata | 15 yrs, very active, GPLv3 | **Yes — real, shipped translations for vi/ja/ko/zh** | Easy (Python, Docker) | ✅ **PRIMARY** |
| Odoo | ~5M+ LOC | Highest | Extensive | 20 yrs | Yes | Heavy; enterprise/community split complicates licensing | Stretch goal (Phase 8) |
| OpenMRS Core | ~200k LOC Java | High (clinical) | Good | 15 yrs | Limited | Moderate (JVM) | ✅ **SECONDARY** — typed-language check |

**Choice: ERPNext + Frappe as the primary benchmark repository.**

Reasons, in order of weight:
1. **Real multilingual assets.** The shipped translation files make the multilingual task
   genuine rather than staged. This is decisive — a fabricated multilingual test would be
   exactly the benchmark dishonesty §22 of the spec forbids.
2. **Business-rule density.** Approval workflows, pricing rules, tax templates, stock
   valuation, credit limits — the exact rule shapes Reify claims to extract.
3. **Tractable size.** Big enough that agents genuinely struggle; small enough to iterate on
   a laptop. Odoo at 5M LOC would make each benchmark iteration a multi-hour affair and slow
   the falsification loop, which is the thing that matters most early.
4. **Rich, well-formed history** with issue references, enabling retro-PR task construction.
5. GPLv3 is fine for benchmarking — we neither redistribute nor link against it.

**OpenMRS Core is the secondary repo**, added in Phase 6, to check that results are not an
artefact of dynamic-language analysis. If Reify only helps on Python, that is a finding worth
publishing and worth changing the roadmap over.

### N.2 Task construction — "retro-PR", to avoid a hand-made benchmark

Tasks are derived from **real merged PRs**, not invented. For each candidate PR that changed
business behaviour:

1. Check out the parent commit — the world before the change.
2. Derive the task prompt from the issue/PR *description* (the user-facing ask), never from
   the diff.
3. Ground truth = the actual diff + the tests it added/changed + the files it touched.
4. Grade against ground truth (§N.5).

This yields tasks that are real, verifiable, and hard to game. Target: **60 tasks**, sampled
across the categories below, with sampling done *before* anyone looks at how Reify performs
on them. Cherry-picking after the fact is the single easiest way to produce a dishonest
benchmark, so the task set is frozen and committed (`benchmarks/tasks/*.yaml`) at the start
of each measurement round.

### N.3 Task categories

| ID | Category | Example | Graded on |
|---|---|---|---|
| A | Locate business behaviour | "Where is the rule deciding whether a Sales Order needs approval?" | Correct file+symbol in top-3 |
| B | Explain legacy behaviour | "Why is this customer type treated differently at `x.py:812`?" | Rubric vs. ground-truth explanation + citation validity |
| C | Change behaviour safely | "Add a discount tier for strategic customers without altering approval" | Tests pass; existing tests still pass; diff-similarity to reference |
| D | Impact analysis | "What is affected if the credit-limit check moves to the party level?" | Recall/precision vs. reference touched-file set |
| E | Doc/code contradiction | "Does the documented approval flow match the implementation?" | Correct verdict + correct citations for both sides |
| F | Multilingual | "Khách hàng chiến lược có được miễn phê duyệt không?" vs. English equivalent | Same correct answer in both languages |
| G | Historical reasoning | "Why was this condition introduced?" | Identifies the correct commit + reason |

Category F is run as **matched pairs** (the same question in Vietnamese and English) so the
multilingual claim is measured as a *gap*, not asserted.

### N.4 Conditions

| Condition | Setup |
|---|---|
| **A — Raw** | Repository + agent's native tools, unrestricted |
| **B — Search** | Repository + `ripgrep`/`fd`, **token budget matched to Reify's median** (experiment E1) |
| **C — RAG** | Repository + a conventional chunk-embed-retrieve index (top-k, standard settings) |
| **D — Repo-map** | Repository + a repo-map style structural summary (Aider-style / `repomix`) — the strongest honest baseline |
| **R — Reify** | Repository + `reify context` and friends |
| **R-off** | Reify with `REIFY_OFFLINE=1` — no LLM stages at all |
| **R-shuf** | Reify with claims shuffled across tasks (negative control, E3) |
| **O — Oracle** | Hand-written ideal context (ceiling, E2) |
| **N — Memory** | No repository access at all (contamination control, E6) |

Every condition runs on ≥2 model families (E4), including one materially cheaper model.

### N.5 Metrics

**Primary**
- **Task success rate** — automated where tests exist (C), rubric-graded by a *blind* grader
  otherwise (B, E, G). Grading rubrics are committed; a sample is double-graded by a human
  and inter-rater agreement is reported. A benchmark whose grader is an unaudited LLM is not
  a benchmark.
- **Regression rate** — for category C: existing tests that pass before and fail after.

**Efficiency**
- Input / output / total tokens (from provider `usage` fields — real counts, never estimates)
- Tool calls; files opened; wall-clock; USD cost at published list prices (prices recorded
  in the results file, since they change)

**Retrieval quality**
- **Useful Context Ratio** = tokens of context that appear in the ground-truth touched set ÷
  total context tokens
- **Knowledge Retrieval Precision / Recall** vs. the reference evidence set
- **Wrong-file reads** = files opened that appear in no ground-truth set

**Composite (report all three; no single headline number)**
```
Context Efficiency        = successes ÷ (total input tokens / 1000)
Cost per Successful Task  = total USD ÷ successes
Safety-Adjusted Success   = (successes − regressions) ÷ tasks
```
`Safety-Adjusted Success` is the one to lead with, because a tool that raises success while
raising regressions is not an improvement for a brownfield maintainer — it is a liability.

### N.6 Extraction-quality benchmark (separate, and prerequisite)

Independent of agent performance, we must know whether the knowledge is *right*. On a
hand-labelled sample of 200 items from ERPNext:

| Measure | Target for MVP | Consequence of missing it |
|---|---|---|
| Business-rule precision (`INFERRED` ≥0.8 conf) | ≥ 0.80 | Below 0.7: do not ship rules; ship graph+history only |
| Business-rule recall (of labelled rules) | ≥ 0.50 | Low recall is acceptable; low precision is not |
| Concept-link precision | ≥ 0.90 | Concepts are the retrieval backbone; errors cascade |
| Conflict precision | ≥ 0.90 | Below 0.9: `reify conflicts` ships behind a flag |
| Confidence calibration | monotone by decile | Non-monotone: the formula is wrong; fix before publishing |

**Precision is weighted far above recall throughout.** A missing rule costs the agent a
search. A wrong rule stated confidently costs a production incident.

### N.7 Reproducibility artifacts (all committed)

`benchmarks/` contains: task definitions with pinned repo commit SHAs; verbatim prompts for
every condition; harness source; model ids and sampling params; Reify version + store
manifest hashes; raw per-run transcripts and token counts; analysis notebooks/scripts; and
the generated report. Re-running is one command (§V).

### N.8 Honesty rules, binding

1. Report every metric measured, including the ones where Reify loses.
2. No cherry-picked tasks; the task set is frozen and hash-committed before results are seen.
3. Confidence intervals on every rate; with 60 tasks, a 5-point difference is noise and will
   be labelled as such.
4. Any number in the README links to the raw result file that produced it.
5. If E2 (oracle) shows a low ceiling, that goes in the README too.
6. No claim of the form "Nx smarter/better" ever. The report shows measurements.

---

## O — MVP scope

### MUST HAVE — the thesis is untestable without these

- `reify init` / `index` / `status` — local, offline, incremental
- tree-sitter extraction for **Python, JavaScript/TypeScript, SQL**
- Symbol graph: symbols, imports, calls (heuristic), references, inheritance
- SQL/data layer: tables, columns, ORM models, read/write edges
- Document ingestion: Markdown, text, HTML, CSV/JSON/YAML
- Git archaeology: introduce/change, blame (lazy), co-change, commit classification
- Concept layer: glossary + identifier mining + **translation-file mining** (bridges 1–3)
- Deterministic business-rule candidates from code guards, DDL constraints and test names
- SQLite store + FTS5 retrieval; blake3-keyed incremental compilation
- `reify context` with `--json` and `--budget` — **the product**
- `reify why`, `reify impact`
- Provenance, evidence, epistemic status, confidence on every derived artifact
- Benchmark harness + 60 frozen tasks + conditions A, B, D, R, R-off, O, N
- Full offline operation

### SHOULD HAVE — needed for the product to be *good*, not just testable

- Java support (unlocks OpenMRS, and typed-language validation)
- Conflict detection + `reify conflicts`
- `reify explain`, `reify flow`, `reify report`
- Optional LLM stages (concept proposal, rule phrasing) with caching + preview
- MCP server (3 tools) and the Claude Code preflight hook
- DOCX + PDF (via `pdftotext`) ingestion
- SCIP ingestion for precise cross-references
- Conditions C (RAG) and R-shuf; second model family

### FUTURE — explicitly deferred, and why

| Deferred | Why |
|---|---|
| Go, C#, Rust, PHP, Ruby, COBOL | Add on demand; the grammar+query pattern makes each ~2 days |
| File-watch daemon | Only if on-demand incremental indexing measures too slow. Measure first |
| Embedding-based reranking | Only if lexical+concept recall measures insufficient |
| Team store sharing / CI-built stores | Real value, but not on the thesis-critical path |
| Web UI / visualisation | Would consume disproportionate effort for a CLI-first product |
| Odoo benchmark | After ERPNext + OpenMRS results are in |
| Jira/Confluence live connectors | Exports only in v1; live connectors break the no-network guarantee's simplicity |
| Cross-repo / microservice-fleet indexing | Big and interesting. Not v1 |

---

## P — Rust architecture

### P.1 Crate layout — start at three, not fifteen

The spec proposes fifteen crates. **That is premature.** Fifteen crates before the first
benchmark result means fifteen `Cargo.toml`s, a public API surface between modules whose
boundaries nobody has validated yet, and a refactor tax on every design change during exactly
the phase when the design changes most.

**Start with three crates. Split when a real force demands it** (compile time > 90s, a
genuine second consumer, or a dependency that must not leak into the library).

```
reify/
├── crates/
│   ├── reify/          lib — everything, as modules
│   │   └── src/
│   │       ├── store/     schema, migrations, queries, manifest
│   │       ├── discover/  walk, classify, hash
│   │       ├── parse/     tree-sitter drivers + per-language .scm queries
│   │       ├── docs/      markdown, html, docx, pdf, csv/json/yaml
│   │       ├── git/       history, blame, co-change, classification
│   │       ├── sql/       DDL, queries, ORM models
│   │       ├── graph/     nodes, edges, traversal, PPR
│   │       ├── concepts/  glossary, mining, i18n bridge, language detect
│   │       ├── rules/     candidate mining, confidence, conflicts
│   │       ├── query/     planner, deterministic/semantic/LLM paths
│   │       ├── context/   selection, budget knapsack, renderers
│   │       ├── llm/       provider trait, cache, preview, log
│   │       └── incr/      derived-artifact cache, invalidation
│   ├── reify-cli/      bin — arg parsing, terminal + JSON rendering
│   └── reify-bench/    bin — benchmark harness, graders, report generator
```

**Pre-committed split points** (so the modules are designed with clean seams from day one,
without paying the crate tax now): `parse` and `docs` split out when parser compile time
dominates; `llm` splits out to keep provider SDKs out of the core dependency tree; `store`
splits out if a second consumer appears.

### P.2 Dependency decisions, with the reasoning

| Need | Choice | Rejected | Why |
|---|---|---|---|
| Store | `rusqlite` (bundled SQLite) | RocksDB, redb, sled, any graph DB | Transactions, single file, FTS5, JSON1, recursive CTEs, universal tooling. Our graph fits in an indexed edge table; a graph DB buys nothing measurable at this scale |
| Full-text | **SQLite FTS5** first | Tantivy | Zero extra engine. Tantivy only if measured multilingual recall falls short (§K.3) — decision recorded either way |
| Parsing | `tree-sitter` + grammars | LSP, per-language compilers | Error-tolerant, incremental, uniform, no per-language toolchain |
| Precise xrefs | SCIP protobuf ingest (optional) | building a type checker | Consume what teams already generate |
| Git | `gix` (gitoxide) | `git2`/libgit2 | Pure Rust, fast history walking, no C build. **Risk:** blame maturity — fallback is shelling out to `git blame --porcelain` (§X R4) |
| Hashing | `blake3` | sha256, xxhash | Fast, parallel, cryptographic-grade; on the hot path |
| Parallelism | `rayon` | tokio | Indexing is CPU-bound and embarrassingly parallel. No async runtime in the core — async would be pure ceremony here |
| CLI | `clap` (derive) | — | Standard; good `--help`, which is the agent's discovery surface |
| Serialisation | `serde` + `serde_json` | — | JSON output is a public API |
| Markdown | `pulldown-cmark` | comrak | Lighter, sufficient for heading trees |
| HTML | `scraper` / `html5ever` | — | Mature |
| DOCX | `zip` + `quick-xml` | `docx-rs` | A DOCX is a zip of XML; a dedicated crate is more surface than the job needs |
| PDF | shell `pdftotext`, fallback `pdf-extract` | pdfium-render | Avoids a native lib dependency for a format we degrade gracefully on |
| Language detect | `whatlang` | — | Pure Rust, good enough at section granularity |
| Errors | `thiserror` (lib) + `anyhow` (bin) | — | Conventional split |
| Terminal | `owo-colors` + hand-rolled tables | `comfy-table`, `ratatui` | Output must stay copy-pasteable and pipe-safe; a TUI is off-thesis |
| Tokens | heuristic estimator; `tiktoken-rs` optional | mandatory tokenizer | Budgeting tolerates ±10%. Benchmark uses **real** provider counts, never estimates |
| Vectors (if ever) | `f32` blobs in SQLite + SIMD brute force | any vector DB | 50k concepts × 384 dims ≈ 76 MB; brute-force cosine is single-digit ms. A vector DB here is pure ceremony |

### P.3 Non-negotiable internal contracts

1. `reify` the library **never** performs I/O to a network except through the `llm::Provider`
   trait, which is behind a feature flag and an explicit runtime enable.
2. Every public type that crosses the JSON boundary is `#[non_exhaustive]` and versioned by a
   `schema` string. Agent integrations depend on this; breaking it silently breaks users.
3. No `unwrap()` outside tests. Enforced by clippy in CI.
4. Every extractor declares `EXTRACTOR_VERSION`; bumping it invalidates its cached outputs.

---

## Q — Performance targets

Targets are hypotheses. Each has a benchmark that measures it, and the measured value is what
gets published — even when it is worse than the target.

| # | Operation | Target | Hard ceiling (else redesign) |
|---|---|---|---|
| Q1 | `reify why <file:line>` | < 20 ms p50 | 100 ms p95 |
| Q2 | `reify context "<task>"` deterministic path | < 100 ms p50 | 300 ms p95 |
| Q3 | `reify impact` | < 50 ms p50 | 200 ms p95 |
| Q4 | Cold start (process spawn → first byte) | < 30 ms | 80 ms |
| Q5 | Full index, ERPNext+Frappe, 8 cores, no LLM | < 10 min | 25 min |
| Q6 | Incremental, single-function edit | < 500 ms | 2 s |
| Q7 | Incremental, 50-file merge | < 5 s | 20 s |
| Q8 | Peak RSS during full index | < 2 GB | 6 GB |
| Q9 | Resident memory for queries | < 150 MB | 400 MB |
| Q10 | Store size | < 5% of working tree | 15% |
| Q11 | `reify context` output size | median < 2000 tokens | 4000 |

Q4 matters more than it appears: an agent may call Reify several times per task, and a 400 ms
cold start turns the tool into something the agent learns to avoid. Q11 is the product
constraint — if Reify's own output is large, it has reinvented the problem.

---

## R — Testing strategy

| Layer | Approach | Gate |
|---|---|---|
| **Unit** | Pure functions: identifier splitting, confidence formula, budget knapsack, language detection, tokenisation | Every module; fast |
| **Parser golden tests** | Per language, a fixture file + committed expected symbol/edge JSON. Diffs are reviewable | Blocks merge |
| **Fixture repositories** | `fixtures/minierp/` — a small synthetic business system (~40 files) with *deliberately planted* knowledge: a documented rule, a code rule that contradicts it, a Vietnamese BRD, an i18n file, a magic-number enum, a historical fix commit, a workflow. Every claim Reify makes about it has a known right answer | Blocks merge |
| **Multilingual fixture** | `fixtures/minierp-vi/` — matched VI/EN document pair over identical code | Blocks merge |
| **Graph tests** | Traversal, PPR determinism, cycle handling, edge-weight effects | Blocks merge |
| **Property tests** (`proptest`) | ① incremental ≡ full (§L.3) ② budget knapsack never exceeds budget ③ selection is deterministic under input permutation ④ every non-`CONFIRMED` node has ≥1 evidence ⑤ round-trip store serialisation | Blocks merge |
| **Invariant tests** | `status` is rendered in every output path; no LLM call occurs under `REIFY_OFFLINE=1`; **no network syscall in the whole suite** (run under an egress-blocked sandbox in CI) | Blocks merge — these are the safety claims |
| **Integration** | Full `init → index → context` on `fixtures/minierp`, asserting exact JSON | Blocks merge |
| **Real-repo smoke** | Index ERPNext at a pinned SHA in CI nightly; assert no panic, and that counts stay within ±10% of the recorded baseline (catches silent extraction regressions) | Nightly |
| **Performance regression** | `criterion` benches for Q1–Q4, Q6; CI fails on >20% regression | Blocks merge |
| **Extraction quality** | Precision/recall/calibration vs. the 200-item labelled set (§N.6) | Nightly + release gate |
| **Agent benchmark** | Full Brownfield Benchmark | Per release; never a merge gate (too slow, too costly, and it must never be tuned against per-commit) |

Two deliberate choices: (a) the labelled quality set is versioned and its *labels* are
reviewed by a second person, because a benchmark graded against one person's opinions is a
mirror; (b) the agent benchmark is explicitly **not** a merge gate, so nobody is tempted to
overfit to it commit-by-commit.

---

## S — Repository structure

```
reify/
├── README.md                  30-second pitch (§W)
├── LICENSE                    Apache-2.0 (permissive → adoption; see §Z)
├── CONTRIBUTING.md
├── CHANGELOG.md
├── Cargo.toml                 workspace
├── rust-toolchain.toml        pinned
├── deny.toml                  cargo-deny: licences + advisories
│
├── crates/
│   ├── reify/                 the library (§P.1)
│   ├── reify-cli/             the `reify` binary
│   └── reify-bench/           benchmark harness + graders + report generator
│
├── queries/                   tree-sitter extraction queries, per language
│   ├── python/{symbols,calls,sql,rules}.scm
│   ├── typescript/…
│   ├── java/…
│   └── sql/…
│
├── fixtures/
│   ├── minierp/               synthetic business system with planted knowledge
│   ├── minierp-vi/            matched Vietnamese/English documents
│   └── parsers/               per-language golden files + expected JSON
│
├── benchmarks/
│   ├── README.md              how to reproduce, exactly
│   ├── tasks/                 60 frozen task definitions (+ SHA-256 manifest)
│   ├── prompts/               verbatim prompts per condition
│   ├── conditions/            baseline harness configs A,B,C,D,R,R-off,R-shuf,O,N
│   ├── labels/                200-item hand-labelled extraction-quality set
│   ├── results/               raw runs, committed: transcripts, tokens, timings
│   ├── analysis/              scripts producing the report from raw results
│   └── REPORT.md              generated; every number traceable to results/
│
├── docs/
│   ├── PLAN.md                this document
│   ├── architecture.md
│   ├── knowledge-model.md     schema reference
│   ├── json-schema/           versioned output schemas
│   ├── integration/           claude-code.md, codex.md, mcp.md, cursor.md, generic-cli.md
│   ├── privacy.md             the threat model, in user language
│   └── adr/                   decision records (FTS5-vs-Tantivy, gix-vs-git2, …)
│
├── examples/
│   └── erpnext/               walkthrough used by the demo (§25)
│
└── .github/workflows/         ci.yml, nightly.yml, release.yml
```

---

## T — Implementation roadmap

Ten phases. Phases 0–4 constitute the first vertical slice (§U) and end in a **go/no-go**.
Each phase lists goal, modules, tests and acceptance — no phase is "done" without its tests.

---

### Phase 0 — Skeleton and store *(≈3 days)*

**Goal.** A binary that walks a repo, hashes files, writes a SQLite store, and reports.
**Modules.** `store/{schema,migrations}`, `discover/`, `reify-cli` shell, `manifest`.
**APIs.** `Store::open/create`, `Store::upsert_node/edge`, `Walker::walk`, `reify init|status`.
**Tests.** Schema migration round-trip; walker honours ignore files; determinism of the file
manifest across runs.
**Acceptance.** `reify init && reify status` on ERPNext lists file counts by language and
document type in < 20 s, and identifies what it will skip **and why**.
**Benchmark impact.** Establishes the indexing timing harness.

---

### Phase 1 — Code extraction *(≈1.5 weeks)*

**Goal.** Symbols and structural edges for Python and TypeScript.
**Modules.** `parse/`, `queries/python`, `queries/typescript`, `graph/` (nodes/edges), `incr/` v1.
**APIs.** `Extractor::extract(file) -> Vec<Node|Edge>`, `Resolver::resolve_calls`.
**Tests.** Parser golden tests for both languages; `minierp` integration; incremental-≡-full
property test (first appearance — it must exist before incremental complexity accumulates).
**Acceptance.** Full index of ERPNext+Frappe completes without panic; symbol count within
±10% of a `ctags`-derived reference; **incremental single-function edit < 1 s**.
**Benchmark impact.** Enables `bench/index` and `bench/incremental` (Q5, Q6).

---

### Phase 2 — Documents, SQL, and Git *(≈1.5 weeks)*

**Goal.** The other two knowledge sources land, and evidence becomes real.
**Modules.** `docs/`, `sql/`, `git/`, `store` evidence tables.
**APIs.** `DocIngestor::ingest`, `SqlExtractor::extract`, `GitArchaeologist::{history,blame,cochange}`.
**Tests.** Section-tree goldens per format; SQL read/write edge goldens; git fixture repo with
a scripted history; blame correctness against `git blame` output.
**Acceptance.** `reify why <file:line>` returns introducing commit, message, authors,
co-changing files and containing symbol in **< 20 ms** on the ERPNext store.
**Benchmark impact.** Q1 measurable. Task category G becomes answerable.

---

### Phase 3 — Concepts and the multilingual bridge *(≈1.5 weeks)*

**Goal.** A Vietnamese business term reaches the right code. **No LLM.**
**Modules.** `concepts/{glossary,mining,i18n,lang}`, FTS5 tokenisation config.
**APIs.** `ConceptResolver::resolve(term, lang) -> Vec<ConceptId>`, `Glossary::{load,suggest,confirm}`.
**Tests.** `minierp-vi` matched-pair retrieval; identifier-splitting unit tests; i18n-file
mining goldens against real ERPNext `vi.csv`; **recall measurement for FTS5 vs. target →
this is the ADR gate for Tantivy**.
**Acceptance.** ≥80% of matched VI/EN benchmark query pairs resolve to the same concept set;
concept-link precision ≥0.90 on a 100-item hand-labelled sample.
**Benchmark impact.** Task category F becomes answerable. Multilingual claim becomes testable.

---

### Phase 4 — Context compilation + first benchmark *(≈2 weeks)* → **GO / NO-GO**

**Goal.** `reify context` exists, and we find out whether the thesis survives.
**Modules.** `query/planner`, `context/{select,budget,render}`, `reify-bench/`.
**APIs.** `ContextCompiler::compile(task, budget) -> Context`, `Renderer::{human,json}`.
**Tests.** Knapsack never exceeds budget (property); selection deterministic under input
permutation (property); golden context output for `minierp` tasks.
**Acceptance.** 20-task pilot benchmark executed with conditions **A, B(budget-matched), D,
R, O, N**, results committed raw, and the decision gate in §U evaluated honestly.
**Benchmark impact.** *This is the phase the project exists to reach.*

---

### Phase 5 — Rules, conflicts, and calibration *(≈2 weeks)*

**Goal.** Deterministic rule candidates; conservative conflict detection; measured calibration.
**Modules.** `rules/{mine,confidence,conflict}`.
**Tests.** Planted-conflict detection in `minierp`; **planted non-conflicts must NOT fire**
(false-positive test, weighted more heavily than the positive test); calibration monotonicity.
**Acceptance.** Rule precision ≥0.80 and conflict precision ≥0.90 on the 200-item labelled
set. **If conflict precision < 0.90, `reify conflicts` ships behind `--experimental`.**
**Benchmark impact.** Task category E becomes answerable.

---

### Phase 6 — Java, OpenMRS, and full benchmark *(≈2 weeks)*

**Goal.** Prove the results are not a Python artefact; run the full 60-task benchmark.
**Modules.** `queries/java`, SCIP ingestion.
**Acceptance.** Full benchmark on both repos, all conditions, ≥2 model families; `REPORT.md`
generated with confidence intervals; every README number traceable to `results/`.

---

### Phase 7 — Agent integration *(≈1.5 weeks)*

**Goal.** The integrations developers actually install.
**Modules.** `reify-cli` MCP mode (3 tools), `reify preflight`, `reify init` snippet writer.
**Tests.** MCP protocol conformance; preflight output size cap; hook installs and uninstalls
cleanly.
**Acceptance.** Claude Code, Codex and one MCP client each complete a benchmark task through
their native integration path.

---

### Phase 8 — LLM stages, optional and audited *(≈1.5 weeks)*

**Goal.** Concept proposal and rule phrasing, cached, previewable, off by default.
**Modules.** `llm/{provider,cache,preview,log}`.
**Tests.** No network under `REIFY_OFFLINE=1`; cache hit on identical input hash; every
LLM-derived claim carries evidence drawn from retrieved facts.
**Acceptance.** Ablation E5 run: does the LLM stage measurably improve benchmark outcomes?
**If not, it ships disabled and the README says so.**

---

### Phase 9 — Polish, `reify report`, docs, launch *(≈1.5 weeks)*

**Goal.** The 60-second demo, the README, the report card, the integration docs.
**Acceptance.** A developer clones ERPNext and reaches a useful `reify why` in under
5 minutes on a clean machine, following only the README.

---

**Total to public launch: ≈16 weeks of focused work.** The go/no-go at week ~6 is the point
of the whole schedule — most of the risk is retired before most of the cost is spent.

---

## U — First vertical slice

### U.1 The rule

**Do not build infrastructure for six weeks and then find out.** The slice is the shortest
path from an empty repo to a number that can kill the project.

```
  ERPNext ──▶ index (py/ts/sql, md docs, git) ──▶ concepts (glossary + i18n)
                                                          │
                                                          ▼
                                            reify context "<task>" --json
                                                          │
                                                          ▼
                                     20-task benchmark, conditions A · B · D · R · O · N
                                                          │
                                                          ▼
                                                    GO / NO-GO
```

### U.2 In the slice

Phases 0–4. Python + TypeScript + SQL. Markdown + text documents. Git history, blame,
co-change. Glossary + identifier mining + i18n bridge. SQLite + FTS5. `reify index`,
`reify why`, `reify context --json`. Benchmark harness with 20 tasks.

### U.3 Explicitly NOT in the slice

Java. PDF/DOCX. LLM stages of any kind. MCP. Hooks. Conflict detection. `reify report`.
`reify flow`. `reify explain`. Embeddings. Tantivy. Pretty terminal output beyond the
minimum. Any crate beyond the three.

Every one of these is defensible *later* and indefensible *before the go/no-go*.

### U.4 The decision gate

Run at the end of Phase 4, against the 20-task pilot. Pre-registered thresholds — written
down now, before any result exists:

| Observation | Reading | Action |
|---|---|---|
| **R > B(budget-matched) on safety-adjusted success by ≥10 points, AND R uses ≤50% of A's input tokens** | Thesis supported | **GO.** Continue to Phase 5 |
| R ≈ B but R uses far fewer tokens | Efficiency win only; no correctness win | **Pivot.** Reposition as a cost/latency tool; drop rule extraction; simplify hard |
| **O (oracle) ≈ A** | Context is not the bottleneck on these tasks | **STOP.** The thesis is wrong as stated. Either the tasks are wrong or the premise is. Rebuild tasks once; if it repeats, kill |
| N (no-repo) success is high | Tasks are contaminated by training data | **Rebuild the task set** from post-cutoff commits and re-run. Not a product signal |
| R-shuf ≈ R | The agent is not reading our content | **Fix the output format** before believing any other result |
| R > A but regressions also rise | Dangerous improvement | **Fix before continuing.** Safety-adjusted success is the metric that governs |

The oracle row is the one to respect. If a perfect hand-written context does not beat raw
agent performance, no amount of Rust makes it beat raw agent performance.

---

## V — Benchmark execution plan

### V.1 Commands

```bash
# One-time: fetch and pin the benchmark repositories
cargo run -p reify-bench -- setup            # clones ERPNext/Frappe at pinned SHAs

# Index under test
cargo run --release -p reify-cli -- index --path .bench/erpnext

# Micro-benchmarks (latency, indexing, incremental) — criterion
cargo bench -p reify                          # Q1-Q4, Q6, Q7

# Extraction quality against the labelled set
cargo run -p reify-bench -- quality --labels benchmarks/labels/

# The agent benchmark
cargo run -p reify-bench -- run \
    --tasks benchmarks/tasks/ \
    --conditions A,B,D,R,R-off,O,N \
    --models claude-opus-5,claude-sonnet-5 \
    --repeats 3 \
    --out benchmarks/results/$(date +%Y%m%d)/

# Report
cargo run -p reify-bench -- report \
    --in benchmarks/results/20260901/ \
    --out benchmarks/REPORT.md
```

### V.2 How the harness works

For each (task, condition, model, repeat):

1. Restore the repo to the task's pinned parent commit in a fresh worktree.
2. Build the condition's environment (tools available, context injected, budget applied).
3. Run the agent with the verbatim prompt from `benchmarks/prompts/`.
4. Capture: full transcript, provider `usage` token counts, tool calls, files opened,
   wall-clock, resulting diff.
5. Grade: run tests for category C; blind rubric grading otherwise (grader sees the answer,
   never the condition label — this is what makes rubric grading usable at all).
6. Write one JSON record per run into `results/`. **Raw records are committed.**

`--repeats 3` with reported variance, because single-run agent benchmarks are noise
generators. Any headline difference is reported with a confidence interval, and differences
inside the interval are described as "no measured difference", not as a win.

### V.3 Cost control

Full matrix: 60 tasks × 7 conditions × 2 models × 3 repeats = 2,520 runs. At a rough
$0.30–1.50 per run that is roughly $750–$3,800 per full round. Therefore:

- The **pilot** (Phase 4) is 20 tasks × 6 conditions × 1 model × 2 repeats ≈ 240 runs.
- Full matrix runs at **release boundaries only**, not per commit.
- `--conditions` and `--tasks` subsetting is first-class, and every published result records
  exactly which subset produced it.

### V.4 The report

`benchmarks/REPORT.md` is generated, never hand-written. Structure:

```
              BROWNFIELD BENCHMARK — ERPNext @ <sha> — <date>
              60 tasks · 3 repeats · claude-opus-5 · reify <version>

                          A(raw)   B(grep*)  D(map)   R(reify)   O(oracle)
Safety-adj. success        __%       __%      __%       __%        __%
  95% CI                 ±__       ±__      ±__       ±__        ±__
Input tokens (median)      __k       __k      __k       __k        __k
Tool calls (median)        __        __       __        __         __
Files opened (median)      __        __       __        __         __
Useful context ratio       __        __       __        __         __
Regressions (count)        __        __       __        __         __
Cost / successful task    $__       $__      $__       $__        $__
Wall clock (median)        __        __       __        __         __

  * B is token-budget-matched to R's median input tokens.
  Every cell links to the raw runs in results/<date>/.
  Where R does not beat a baseline, the cell is left as measured and discussed below.
```

Plus per-category breakdown, the VI/EN matched-pair gap, the calibration curve, incremental
vs. full index timings, and a **"where Reify lost"** section that is a required part of the
document, not an optional one.

---

## W — README strategy

The README has 30 seconds. It spends them in this order.

**1. One sentence (above the fold).**
> Reify compiles your codebase, documents, SQL and Git history into a fast local knowledge
> layer, then hands your AI coding agent the smallest context it needs to change a
> ten-year-old system correctly.

**2. The problem, in two lines.**
> AI agents are great at greenfield code. In brownfield systems the knowledge that decides
> correctness is spread across code, stale docs, SQL, and commits from 2019 — so the agent
> reads the wrong 40 files and changes the wrong thing.

**3. A terminal recording (asciinema, real, unedited).** `reify why` on ERPNext. Under
20 seconds. This is the single highest-leverage asset in the entire launch.

**4. Three commands, real output.**
```
$ reify why erpnext/selling/doctype/sales_order/sales_order.py:812
$ reify impact "move the credit limit check to the party level"
$ reify context "add a 15% discount for strategic enterprise customers"
```

**5. The benchmark table** — measured numbers, with the link to raw results directly beneath
it, and one line naming where Reify did not help. That line buys more credibility than the
table above it.

**6. Install + integrate, four lines.**
```
brew install reify        # or: cargo install reify-cli
cd your-repo && reify init && reify index
# then tell your agent:  run `reify context "<task>"` before editing
```

**7. Privacy, stated as a promise, in bold.**
> Nothing leaves your machine. LLM calls are off by default. `REIFY_OFFLINE=1` guarantees it.

**8. What Reify is not.** A short, honest list: not an agent, not an IDE, not a chatbot, not
a vector database, not a cloud service — and *not useful on repositories under ~20k LOC,
where `ripgrep` is genuinely better.* Telling people when not to use it is the cheapest
trust you will ever buy.

**Viral hook — `reify report`,** near the top, because it is the thing people screenshot:

```
╭──────────────────────────────────────────────╮
│              REIFY SYSTEM REPORT             │
│              erpnext @ a3f91c2               │
├──────────────────────────────────────────────┤
│  Repository age                    14.2 yrs  │
│  Source files                         8,421  │
│  Symbols                            142,908  │
│  Documented symbols                     18%  │
│  Business rule candidates             3,821  │
│  ...of which corroborated by tests      412  │
│  Concepts (multilingual)              1,204  │
│  Doc/code contradictions                 27  │
│  Hidden dependencies (non-call)         382  │
│  Highest-churn modules                   47  │
│  Knowledge coverage                     68%  │
├──────────────────────────────────────────────┤
│  Indexed in 6m12s · store 41 MB · offline    │
╰──────────────────────────────────────────────╯
```

Every metric must have a written, defensible definition in `docs/metrics.md`.
"Knowledge coverage" = share of symbols reachable from ≥1 concept or ≥1 rule or ≥1 document
section. If a metric cannot be defined precisely, it does not go on the card — a viral
screenshot full of made-up numbers is a reputational liability, not marketing.

---

## X — Risks

| # | Risk | Likelihood | Impact | Mitigation / trigger |
|---|---|---|---|---|
| **R1** | **Business-rule extraction precision is too low to be useful** | High | Critical | Precision-first thresholds; deterministic candidates before LLM; ship graph+history only if precision <0.70. **Measured in Phase 5, not assumed** |
| **R2** | **Frontier models get good enough at agentic search that the gain shrinks** | Medium-High | Critical | E4 model sweep every release. If gain shrinks with model strength, reposition on cost/latency/privacy — which do not decay. Track this as a *trend*, not a snapshot |
| **R3** | Heuristic call resolution is too imprecise in dynamic languages (Python especially) | High | High | Confidence-weighted edges; `UNKNOWN` over guessing; SCIP ingestion where available; measure edge precision against a labelled sample in Phase 1 |
| **R4** | `gix` blame is immature or slow | Medium | Medium | Fallback to shelling out to `git blame --porcelain`; blame is lazy and cached either way. Decide in Phase 2 |
| **R5** | Conflict detection false positives destroy trust | Medium | High | Five-condition conservative gate (§G.6); false-positive tests weighted above true-positive tests; ships behind `--experimental` if precision <0.90 |
| **R6** | Full index too slow on very large repos | Medium | Medium | Parallel by construction; `--since` history bounding; `--scope` partial indexing; profile in Phase 1 when it is still cheap to change the design |
| **R7** | Benchmark contamination — models have memorised ERPNext | **High** | High | Condition N (no-repo) as a standing control; prefer tasks from post-cutoff commits; report the contamination estimate alongside every result |
| **R8** | Benchmark is too expensive to run often | Medium | Medium | Pilot subset for iteration; full matrix at release boundaries; caching of deterministic conditions |
| **R9** | Multilingual value is narrower than believed (few repos ship good i18n) | Medium | Medium | Glossary bridge works without i18n files; measure how much of the concept layer each bridge contributes and report it |
| **R10** | PDF extraction quality is poor on real BA documents | High | Low-Medium | Degrade loudly, never silently; `reify status` names skipped files; DOCX/Markdown are the primary paths |
| **R11** | Adoption friction: nobody installs a second tool | Medium | Critical | Level-0 integration (a shell command + 6 lines in AGENTS.md); value visible on the first command; no daemon, no config, no account |
| **R12** | Scope creep toward "build an agent" | **Medium-High** | Critical | §4 non-goals are binding. Any PR adding an agent loop, editor or chat UI is closed on sight. This risk is cultural and is the one that most often kills projects like this |
| **R13** | The store schema churns and breaks users' indexes | Medium | Low | Versioned schema + migrations; `reify status` warns and offers `--force` rebuild; rebuild is cheap by design |
| **R14** | Reify's own output becomes bloated, reinventing the problem | Medium | High | Q11 is a hard budget with a regression test on median output size |
| **R15** | Licence friction (GPL benchmark repo, grammar licences) | Low | Medium | `cargo-deny` in CI; benchmark repos are fetched at run time, never vendored |

---

## Y — Kill criteria

Pre-registered. If any of these holds after an honest attempt, the thesis as stated is wrong
and the project should change or stop — not quietly acquire more features.

**K1 — The oracle ceiling is low.**
Hand-written perfect context (condition O) does not beat raw agent performance (condition A)
by ≥15 points of safety-adjusted success, on two independent task sets.
⟹ Context is not the bottleneck on brownfield tasks. **Kill or pivot to a different bottleneck.**

**K2 — Budget-matched search matches Reify.**
Condition B (ripgrep, budget-matched) is within the confidence interval of condition R on
safety-adjusted success, across two model families.
⟹ The win was budget discipline, not compiled knowledge. **Pivot to a context-budgeting tool
— a much smaller product — or stop.**

**K3 — Extraction precision cannot reach 0.80.**
After two serious iterations, business-rule precision stays below 0.70 on the labelled set.
⟹ Ship graph + history + concepts only, drop the rule layer, and rewrite the product claim.
If the remainder does not beat condition D (repo-map), **stop.**

**K4 — The gain inverts with model strength.**
Across three model generations, the Reify advantage shrinks monotonically and is within noise
on the strongest model.
⟹ Reify is a crutch for weaker models. Reposition explicitly on cost, latency and privacy, or
**stop.**

**K5 — Regressions rise.**
Condition R produces more regressions than condition A at equal or higher success.
⟹ Reify is making agents confidently wrong. **Stop shipping until fixed; this is worse than
no product.**

**K6 — Nobody keeps it.**
Six months post-launch, fewer than 10% of installers run `reify index` more than once.
⟹ It is a demo, not a tool. Find out why before writing another line of code.

**K7 — Maintenance exceeds value.**
Keeping parsers, grammars and extractors working consumes more effort than the measured
benefit justifies.
⟹ Narrow to the one or two languages where the benefit is provable.

**The meta-rule:** these are evaluated on measurements, published whether flattering or not.
A project that cannot state the conditions under which it should stop is not being engineered;
it is being believed in.

---

## Z — Open decisions for the product owner

Small set. Each has a default so implementation is never blocked on an answer.

| # | Decision | Default if unanswered |
|---|---|---|
| Z1 | **Licence** — Apache-2.0 vs MIT vs AGPL | **Apache-2.0.** Permissive maximises agent-vendor adoption (the infrastructure play); AGPL would block exactly the integrations that make Reify infrastructure |
| Z2 | **Name / crate name** — `reify` may be taken on crates.io | Publish binary as `reify-cli`, invoked as `reify`. Check availability in Phase 0 |
| Z3 | **Benchmark repo** — ERPNext primary, OpenMRS secondary | As stated in §N.1 |
| Z4 | **Which agents to validate against first** | Claude Code (Level 0 + hook), then Codex, then one MCP client |
| Z5 | **Public repo from day one, or after the go/no-go?** | Public from day one; the honest go/no-go is itself credible content |
| Z6 | **Budget for the benchmark** (~$750–3,800 per full round) | Pilot only until go/no-go; full matrix at releases |
| Z7 | **Language priority after Python/TS/SQL** | Java (unlocks OpenMRS + typed-language validation), then C#, then Go |

---

## Appendix — What an executing agent should read first

1. §U (first vertical slice) — what to build.
2. §T Phases 0–4 — how to build it, in order.
3. §G (knowledge model) — the schema everything else depends on.
4. §N.2–N.5 (benchmark) — build the harness alongside, not afterwards.
5. §Y (kill criteria) — what the work is trying to prove, and what would disprove it.

**The rule that governs every other decision in this plan:** deterministic first, semantic
second, LLM last — and no claim in the README that the benchmark did not measure.
