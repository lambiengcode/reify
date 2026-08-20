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

**ERPNext** — Python/JS, 5,064 files, 40 tasks.

| Condition | | Hit rate | 95% CI | Recall |
|---|---|---:|---:|---:|
| No context at all | *memorisation control* | 22% | 12–38% | 0.16 |
| Content grep, budget-matched | *baseline* | 32% | 20–48% | 0.27 |
| **Reify** | | **60%** | **45–74%** | **0.54** |
| Reify with another task's context | *negative control* | 25% | 14–40% | 0.17 |
| Perfect context | *ceiling* | 100% | 91–100% | 1.00 |

**What the controls establish:**

- **Context is the bottleneck.** Perfect context scores 100% where none scores 22%.
  That 78-point gap is the entire space any retrieval system can compete in, and it is
  wide — the single most important thing this benchmark had to determine.
- **Reify recovers 49% of that gap. Grep recovers 13%.**
- **The content does the work, not the framing.** A decoy context of identical shape and
  size scores 25% against Reify's 60%.
- **Contamination is modest** and subtracted above rather than ignored.

### Retrieval quality, without a model

Does the tool put the right file in front of the agent at all?

| | content grep | path grep | **Reify** |
|---|---:|---:|---:|
| Tasks where a changed file was surfaced | 4/40 (10%) | 7/40 (18%) | **23/40 (57%)** |
| Mean recall | 0.08 | 0.16 | **0.50** |
| MRR of the first correct file | 0.07 | 0.12 | **0.23** |
| Median files put in front of the agent | 3 | 88 | 13 |
| Median latency | 43 ms | 0 ms | 57 ms |

### Does it generalise? A second repository, in a typed language

**OpenMRS** — Java, 1,603 files, 13,182 commits, 22 tasks, same leakage-free method.

| Condition | Hit rate | 95% CI |
|---|---:|---:|
| No context at all | **0%** | 0–15% |
| Content grep, budget-matched | 41% | 23–61% |
| **Reify** | **45%** | 27–65% |
| Decoy context | 14% | 5–33% |
| Perfect context | 100% | 85–100% |

The controls are *cleaner* than ERPNext's — zero memorisation, and a decoy at 14%
against 45%. **But the margin over grep is 4 points and the intervals overlap almost
entirely. On this repository Reify is not measurably better than grep.**

That difference between the two repositories is the most useful thing this benchmark
found, and it is not mysterious. Reify builds **948 concepts on ERPNext and 568 on
OpenMRS**, but ERPNext *declares* 528 of its own in entity metadata while OpenMRS
declares 41. The rest are inferred, and inferred vocabulary is weaker evidence.

**So the honest claim is:** Reify's advantage scales with how much vocabulary a
repository declares. Where a team has written its domain down — entity metadata, ORM
mappings, a glossary, translation files — the gain is large. Where it has not, Reify is
roughly grep with better structure. `reify concepts --suggest` exists precisely to move
a repository from the second case toward the first.


### Measured performance

ERPNext, 5,064 files, 8-core M-series laptop.

| | Measured | Target | |
|---|---:|---:|---|
| Full index, no model | 4.6 s | < 10 min | ✅ |
| Reindex, nothing changed | 0.6 s | — | ✅ |
| Reindex, one file edited | 0.7 s | < 500 ms | ~ |
| `reify context` | 57 ms | < 100 ms | ✅ |
| `reify impact` | 0.2 ms | < 50 ms | ✅ |
| `reify why` | 205 ms | < 20 ms | ❌ includes a `git log -L` subprocess; ~5 ms without |
| Peak memory, full index | 224 MB | < 2 GB | ✅ |
| Store size | 47 MB (33% of the 144 MB working tree) | < 5% | ❌ |

A full index took 78 seconds until the full-text index was keyed by node id. `uid` is
`UNINDEXED` in FTS5, so `DELETE ... WHERE uid = ?` scanned the whole table once per
node — quadratic, and invisible until it was timed per stage. Editing one file took
5.9 seconds until the repository-wide stages learned to skip when their inputs are
provably unchanged.

---

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

## What it reads

**Code — 11 languages.** Python, TypeScript, JavaScript, Java, Go, C#, Rust, Ruby, PHP,
C/C++, Kotlin, plus SQL. Each has a test asserting it yields containers, callables and
calls, because a missing grammar node produces an index that looks healthy while holding
one symbol per file.

**Documents — whatever the analyst wrote it in.** Markdown, plain text, HTML (including
Confluence exports), DOCX, legacy binary DOC, ODT, RTF, XLSX, PPTX and PDF. Formats with
no usable pure-Rust reader are delegated to an external converter, and when none is
installed Reify **says so loudly** rather than indexing nothing quietly.

**Structured metadata.** Frappe/ERPNext DocType JSON, Hibernate ORM mappings, Java and
Spring `.properties` message bundles, and i18n CSV translation tables. These are the
highest-precision vocabulary a repository can offer: a team declared them, and the
application reads them, so they stay current.

## Multilingual

Concept ids are opaque and every label carries a language tag, including English — no
language is canonical. A Vietnamese, Thai, Korean or German requirement reaches English
code through the concept layer, not through a multilingual embedding model, which is why
it is deterministic and citable.

Around sixty locales are recognised on translation files and message bundles. Obligation
and exemption language is detected in English, Vietnamese, German, Spanish, French,
Portuguese, Indonesian, Thai, Korean, Japanese and Chinese, so a rule written in any of
them is mined as a rule.

Three details that only matter once you leave Latin script, each of which was a real bug:

- **Thai, Lao, Khmer, Japanese and Chinese are written without spaces**, so a word index
  stores one enormous token and a query for a word inside it matches nothing. Reify keeps
  a trigram substring index for non-ASCII content and falls back to it.
- **Korean attaches particles to the stem** — `승인` becomes `승인을` — so whole-word
  matching finds neither. Non-ASCII terms match by substring.
- **Sentence length cannot be counted in spaces.** Where there is no spacing, characters
  stand in for it, or every claim in those languages is rejected as too short to be a rule.

## Four bridges from business vocabulary to code

In precision order. The last one is what makes Reify work on a repository that declares
nothing at all.

| Bridge | Source | Available when |
|---|---|---|
| **Declared** | `.reify/glossary.toml`, entity metadata, ORM mappings | a human or a framework wrote it down |
| **Translation** | i18n tables, message bundles | the product has been localised |
| **Co-occurrence** | document headings that also name code | there is documentation |
| **Code vocabulary** | phrases the identifiers keep repeating | **always** |

The last runs only on what the others left uncovered, so it fills gaps rather than
competing with better evidence.

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
