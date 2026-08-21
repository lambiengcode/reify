# Roadmap — getting to numbers that are actually impressive

**Status:** plan. Nothing here is built. Written 2026-08-21.

[`PLAN.md`](PLAN.md) is the original product and engineering plan, and its kill criteria
still govern. This document is narrower: it asks what would take Reify from *measurably
better than grep on one repository* to *obviously worth installing on any repository*,
and it commits to targets before the work starts so the result can be judged rather
than narrated.

---

## 1. Where we actually are

Two repositories, leakage controlled, a real model in the loop.

| | ERPNext (Python/JS) | OpenMRS (Java) |
|---|--:|--:|
| no context (floor) | 22% | 0% |
| budget-matched grep | 32% | 41% |
| **reify** | **60%** | **45%** |
| perfect context (ceiling) | 100% | 100% |
| **share of headroom recovered** | **49%** | **45%** |

Retrieval on its own: hit rate 58% / 41%, **precision 0.06**, **MRR 0.23**.

Three things that table says, and one it does not:

1. **The ceiling is wide open.** Perfect context wins every task. Nothing about the
   thesis is exhausted; roughly half the available gap is still unclaimed.
2. **The right file is often already there and ranked badly.** MRR 0.23 means the first
   correct file sits around fourth on average, inside a list of thirteen. That is a
   ranking problem, not a recall problem, and ranking problems are usually cheaper to
   fix than recall problems.
3. **The result is not repo-independent.** On OpenMRS the margin over grep is four
   points and the intervals overlap. Until that closes, the honest claim stays narrow.
4. What it does *not* say: whether an agent given better context actually **completes
   the task**. Every number here measures file identification. That gap is addressed in
   §4.

## 2. What "impressive" means, pre-registered

Committed now, before the work, so nobody can move the goalposts afterwards.

| Metric | Today | Target | Why this number |
|---|--:|--:|---|
| Hit rate, **both** repositories | 60% / 45% | **≥ 80% each** | Above the point where an agent can rely on it rather than double-check it |
| Share of headroom recovered | 49% / 45% | **≥ 75% each** | Closes most of the distance to a perfect oracle |
| Gap between the two repositories | 15 pts | **≤ 8 pts** | The result generalises, or it is a property of ERPNext |
| MRR | 0.23 | **≥ 0.50** | The right file is in the top two |
| Precision | 0.06 | **≥ 0.20** | One useful file in five, not one in sixteen |
| Repositories measured | 2 | **≥ 4** | Two repositories cannot distinguish a real effect from a coincidence |
| End-to-end task completion | not measured | **measured, with a stated delta** | §4 |

A run that hits four of seven is a real improvement and should be reported as exactly
that, not as success.

## 3. The seven bets, ranked by expected value

Each states the mechanism, the reason to believe it, how it is measured, and what
result would mean it did not work.

---

### Bet 1 — Learn the retrieval prior from the repository's own history

**The biggest single opportunity, and it is sitting in `.git` already.**

Every merged commit is a labelled training example that nobody had to label:

```
commit message  ≈  a ticket description
files changed   =  the correct answer
```

A repository with 60,000 commits contains 60,000 examples of *"when someone described a
change like this, these are the files they touched."* That is precisely the question
`reify context` is asked, and Reify currently ignores the answer.

**Mechanism.** At index time, build a term → file association table from commit messages
and their changed files, scored by pointwise mutual information rather than raw
frequency, so a file that changes in every commit earns nothing. At query time the task
text produces a prior over files, blended with the existing lexical and graph signals.

**Why it should work.** It captures what no static analysis can: that this team touches
`pricing_rule.py` whenever anyone says "discount", regardless of what the identifiers
are called. It is deterministic, local, needs no model, and is fully citable — "seven
commits mentioning *credit limit* touched this file."

**Leakage.** The table must be built only from commits *before* the benchmark base, which
the existing leakage-free harness already enforces. Any run that builds it from the full
history is invalid and must be discarded, not adjusted.

**Measure.** Hit rate and MRR, with and without the prior, on both repositories.

**Falsified if.** MRR moves less than 0.05. That would mean commit messages do not
describe changes in the vocabulary tickets use — plausible in repositories whose commits
all say `fix: TRUNK-1234`, and worth knowing either way.

---

### Bet 2 — Make tests point at the code they test

`EdgeKind::TestedBy` exists in the model and **is never populated**. That is a gap, not a
design.

**Mechanism.** Resolve a test to the code under test through its imports, its fixture
setup, and the symbols it calls that are not test helpers. Emit `TESTED_BY`, and let
relevance spread across it in both directions.

**Why it should work.** A test named `test_corporate_order_requires_approval` is an
executable statement of intent, written in the vocabulary of the requirement rather than
the implementation. It is often the *only* place business language and code sit in the
same file. Reify already mines those names as rules; it does not yet use them to find
the code.

**Measure.** Hit rate on tasks whose ground truth has a corresponding test, versus those
that do not. If the gain is confined to the first group, the mechanism is real.

**Falsified if.** No measurable difference on either group.

---

### Bet 3 — Fit the ranking weights instead of choosing them

Every weight in `context.rs` — per-edge decay, per-kind seed weight, question coverage,
path affinity, hop decay — was chosen by hand and defended in a comment. Some are
certainly wrong.

**Mechanism.** Historical commits are labelled data (Bet 1). Fit the weights against
commits *before the benchmark base* with a small deterministic search — coordinate
descent over a bounded grid, no ML framework, no gradient, seeded and reproducible.
Ship the fitted weights as defaults, and keep the search reproducible in the repository.

**Why it should work.** MRR 0.23 with hand-picked weights is a floor, not a ceiling.

**The trap, named in advance.** This is the single easiest way to produce a dishonest
benchmark. The fit must use a *disjoint, earlier* commit range from the tasks, the task
set stays frozen, and the fitted weights are evaluated once. Anything else is fitting to
the test set with extra steps.

**Falsified if.** The fitted weights beat the hand-picked ones on the training range but
not on the held-out tasks. That result gets published too.

---

### Bet 4 — Let an agent come back for more

Reify answers once. Real agents iterate: they read, learn the answer is not there, and
look again. Right now the second look starts from nothing.

**Mechanism.** `reify context --seen <files> --exclude <files>`, plus a `--refine`
that takes the previous answer's id and returns the next-best set with the already-read
material suppressed. Stateless: the agent carries the state, Reify stays a pure
function.

**Why it should work.** Even at today's MRR, three cheap rounds put the right file in
front of the agent far more often than one round. It also matches how the MCP and CLI
integrations are actually used.

**Measure.** A new benchmark condition, `R-reify-iterative`, capped at three rounds and
charged the **cumulative** token cost across all of them — otherwise iteration is free
in the measurement and expensive in reality.

**Falsified if.** Three rounds cost more than three times a single round's tokens for
less than a ten-point hit-rate gain.

---

### Bet 5 — Expand the query before searching it, not after

Concepts are used to *spread* relevance after lexical seeding. They should also shape
what is searched in the first place.

**Mechanism.** Resolve task terms to concepts, then search using every surface form the
concept knows — including other languages, code identifiers and column names. A query
that says "approval" should search `phê duyệt`, `approval_status` and `tabApproval` too.

**Why it should work.** It is the cheapest fix for a specific failure: the analyst's
word and the code's word differ, which is the entire premise of the concept layer,
applied at the one stage that currently ignores it.

**Measure.** Hit rate on the multilingual matched pairs, and on tasks where the ground
truth shares no literal token with the task text — the subset where this can possibly
help.

**Falsified if.** No gain on that subset.

---

### Bet 6 — Take precise cross-references where a team already generates them

Deferred in [ADR 0003](adr/0003-no-lsif-scip-ingestion.md) for want of evidence. The
OpenMRS result is now that evidence: the typed-language repository is where Reify is
weakest, and typed languages are exactly where precise indexes exist.

**Mechanism.** Ingest SCIP when `index.scip` is present, overriding heuristic call edges
at high confidence. Consume, do not build.

**Why it should work — and why it might not.** Heuristic resolution drops names matching
more than five candidates, which in a large Java codebase is a lot of names. But
resolution precision has still not been *shown* to be the bottleneck, so this bet is
ranked below cheaper ones deliberately.

**Measure.** Generate a SCIP index for OpenMRS, then compare hit rate with and without.

**Falsified if.** Under a three-point gain, in which case ADR 0003 stands and gets a
second dated entry saying so.

---

### Bet 7 — A third and fourth repository, chosen to hurt

Two repositories cannot separate a real effect from a coincidence, and both current ones
were picked partly because they are convenient.

**Candidates**, chosen for the properties Reify is currently worst at:

| Repository | Why it is a hard case |
|---|---|
| **Odoo** | 5M+ LOC. Tests whether anything survives at that scale |
| **Metasfresh** or **Apache OFBiz** | Java ERP with heavy XML configuration — business logic outside code entirely |
| **Medusa** or **Saleor** | Modern TypeScript/Python commerce; young, well-documented — the *opposite* profile, so a gain there means the result is not confined to old systems |

**Measure.** The same leakage-free protocol. Report each separately; never average across
repositories, because averaging is how a bad result on one gets hidden by a good one on
another.

## 4. The measurement that would settle it

Everything above measures **file identification**. The claim people actually care about
is task completion.

**Reify SWE-bench.** Take tasks whose reference commit changed code covered by tests.
Give an agent the repository at the parent commit, let it make a change, and run the
tests the reference commit added or modified. Report resolve rate with and without
Reify, plus regressions in previously-passing tests.

This is the number worth putting at the top of the README, and the only one that can
support a claim about correctness rather than retrieval. It is also expensive — a full
matrix is thousands of agent runs — so it belongs at a release boundary, not in the
iteration loop.

**Pre-registered:** if end-to-end resolve rate does not improve while file identification
does, that finding is published as prominently as any gain. It would mean Reify helps
agents *find* code without helping them *change* it correctly, which is a materially
smaller product and the README would have to say so.

## 5. What this roadmap will not do

Named so they can be argued with, rather than quietly avoided:

- **No embedding index.** Not because embeddings are bad, but because every deterministic
  signal above is unexhausted, and an embedding index costs the citation that makes a
  Reify answer checkable. Revisit only when Bets 1–5 are measured out.
- **No cloud service, no telemetry, no upload.** The audience is companies whose source
  code cannot leave the building. That constraint is the product, not a limitation of it.
- **No agent loop, no editor, no chat UI.** Reify feeds agents. Scope creep toward
  becoming one is the risk [`PLAN.md`](PLAN.md) §X names as most likely to kill it.
- **No tuning against the benchmark task set.** Weights are fitted on an earlier,
  disjoint commit range or not at all (Bet 3).

## 6. Order of work

Ranked by expected value per unit of effort, not by interest.

| Phase | Bets | Why here |
|---|---|---|
| 1 | 1, 2 | Highest expected gain, no new dependencies, both fully deterministic |
| 2 | 3, 5 | Ranking and query expansion, once there is history data to fit against |
| 3 | 4 | Iteration, which multiplies whatever the earlier phases achieve |
| 4 | 7 | Two more repositories, before believing any of it |
| 5 | 6 | SCIP, only if typed-language weakness survives phases 1–4 |
| 6 | §4 | End-to-end completion, the measurement that settles it |

## Outcomes so far — updated 2026-08-21, after phases 1–4

Retrieval numbers are final (frozen tasks, leakage-free, one binary). Model-in-the-loop
numbers follow the same protocol and are reported in the README.

### What was built

| Bet | Status | Outcome |
|---|---|---|
| 1 — history prior | **shipped** | The single largest ranking gain. MRR roughly doubled on every repository where commits carry real messages |
| 2 — TESTED_BY | **shipped** | Emitted for every call from a test file into production code, across all eleven languages' test conventions |
| 3 — fitted weights | **shipped, then partly reverted** | See below — the fit's falsification clause fired |
| 4 — iteration | **shipped** | +9 to +17 points of hit rate for ~2.8× tokens, passing its pre-registered bar on three of four repositories |
| 5 — concept expansion | **shipped, honest verdict: no measurable effect** | Identical training scores with it on and off. Its mechanism is multilingual and is covered by tests; on English task sets it does nothing, and saying otherwise would be decoration |
| 7 — two more repositories | **shipped** | OFBiz (Java + XML config) and Medusa (modern TS monorepo) |
| 6 — SCIP | not started | The OFBiz result (below) removed its premise |
| §4 — end-to-end | not started | Requires runnable test environments per repository; still the measurement that settles the real claim |

### Retrieval, frozen tasks, all four repositories

| | grep | reify | reify ×3 rounds |
|---|--:|--:|--:|
| ERPNext (Python/JS) | 10% | 55% | **72%** |
| OFBiz (Java + XML) | 12% | 45% | **62%** |
| OpenMRS (Java) | 32% | 41% | **50%** |
| Medusa (modern TS) | 18% | 18% | **28%** |

Three findings that were not in the plan:

1. **The Java weakness was OpenMRS-specific, not Java-specific.** OFBiz — Java, with
   its business logic largely in XML — shows one of the largest margins of any
   repository. K-R2's premise ("typed languages are where Reify is weak") was wrong;
   what OpenMRS lacks is *history that speaks the tasks' vocabulary*, not types.
2. **Modern, well-factored TS is the hard case**, inverting the expectation. Medusa's
   tasks describe UI behaviour ("remove duplicate cloud auth button") whose vocabulary
   barely intersects the code's. Iteration lifts it from 18% to 28%; nothing else
   moved it. This is now the open problem.
3. **A file whose *path* matches the task used to contribute nothing** — file nodes had
   no edges to their contents and were not rendered. Fixing that fan-out was worth
   more than any weight: on training data it added 3–12 points of hit rate and up to
   0.19 MRR on top of everything else.

### Model-in-the-loop, final (seven arms; 7 of ~1,000 calls failed, all Medusa, excluded)

| hit rate | none | grep | grep ×3 | reify | **reify ×3** | decoy | oracle |
|---|--:|--:|--:|--:|--:|--:|--:|
| ERPNext, n=40 | 22% | 30% | 50% | 55% | **75%** | 12% | 100% |
| OFBiz, n=40 | 0% | 12% | 28% | 68% | **78%** | 2% | 100% |
| OpenMRS, n=22 | 0% | 41% | 32% | 41% | **59%** | 9% | 100% |
| Medusa, n=40 | 0% | 21% | 24% | 15% | **26%** | 0% | 100% |

Subtracting each memorisation floor, reify ×3 recovers 68% of the oracle gap on
ERPNext, 78% on OFBiz, 59% on OpenMRS, 26% on Medusa.

### The §2 scorecard: one of seven, with every number closer than the last look

| Target | Bar | Landed | |
|---|---|---|---|
| Hit rate, ERPNext & OpenMRS | ≥ 80% each | 75% / 59% | ✗ |
| Headroom recovered | ≥ 75% each | 68% / 59% | ✗ |
| Cross-repo gap (those two) | ≤ 8 pts | 16 pts | ✗ |
| MRR | ≥ 0.50 | 0.45–0.46 on three repos | ✗ |
| Precision | ≥ 0.20 | 0.11–0.13 at the default cutoff; the 0.45 cutoff reaches 0.24–0.29 at a cost of 2–3 tasks in 40, a documented caller's trade | ✗ |
| Repositories | ≥ 4 | 4 | ✓ |
| End-to-end completion | measured | not measured — needs per-repo runnable test environments | ✗ |

Per §2's own rule: a real improvement reported as exactly that, not success. The
matched-cost margins (+25, +50, +27, +2) are the product's honest pitch. What the
ranking-precision phase added on top of the first pass: verbatim identifier lookup,
stemmed prefix search, file-aggregate ordering and the offer cutoff — worth +5 to +25
points of single-shot hit rate and roughly +0.15 MRR on the three repositories where
vocabulary connects at all.

### The fit's falsification clause fired, as designed

Bet 3 pre-registered: *"Falsified if the fitted weights beat the hand-picked ones on
the training range but not on the held-out tasks. That result gets published too."*

That is what happened. Training preferred a history-prior weight of 2.2–5.5; every
value in that range scored worse than the pre-fit default (0.9) on the frozen tasks —
the commit-vocabulary association is real but nonstationary, and a weight tuned on one
window overshoots the next. The default reverted to 0.9, the value chosen before any
evaluation was seen, and the full training surface is committed in
`benchmarks/weights/`. K-R3 is *not* triggered — the prior works (it carries much of
the MRR gain at 0.9) — but its strength cannot be chosen by fitting to history alone.

### Multiplicity, stated plainly

During this phase the frozen tasks were evaluated more than once while diagnosing the
fit failure and the Medusa collapse. The final numbers above come from the last run,
after all decisions were locked, but a skeptical reader should treat the *margins* as
softer than a single-look protocol would justify. The decisions themselves — reverting
the prior, adding the file fan-out — were made on training data or from structural
diagnosis, not by choosing whatever maximised the frozen scores.

## 8. Kill criteria for this roadmap

Distinct from the product kill criteria in [`PLAN.md`](PLAN.md) §Y. These govern the
roadmap only.

**K-R1 — the ranking ceiling is low.** If Bets 1, 2, 3 and 5 together move MRR under
0.10, the retrieval architecture is at its limit and further ranking work is a bad
investment. Move to iteration (Bet 4) and end-to-end measurement, and say plainly that
single-shot retrieval plateaued.

**K-R2 — the repository gap does not close.** If, after phases 1–3, OpenMRS still shows
no distinguishable win over grep, the honest claim narrows permanently to *"for
repositories that declare their domain vocabulary"* — and the README says so in the
first paragraph rather than in a caveat.

**K-R3 — history is not predictive.** If Bet 1 fails on both repositories, the largest
assumption in this roadmap is wrong, and everything downstream of it should be
re-estimated before being built.

**K-R4 — finding is not changing.** If §4 shows no completion gain despite retrieval
gains, stop claiming Reify makes agents *better* and start claiming it makes them
*cheaper and faster*, which the token numbers would still support.
