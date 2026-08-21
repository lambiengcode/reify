<h1 align="center">Reify</h1>

<p align="center">
  <em>Your agent doesn't know why that line is there. Reify does.</em>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/license-Apache--2.0-111111?style=flat-square" alt="Apache-2.0">
  <img src="https://img.shields.io/badge/languages-11-111111?style=flat-square" alt="11 languages">
  <img src="https://img.shields.io/badge/doc%20formats-10-111111?style=flat-square" alt="10 document formats">
  <img src="https://img.shields.io/badge/network%20calls-0-111111?style=flat-square" alt="Zero network calls">
  <img src="https://img.shields.io/badge/tests-345-111111?style=flat-square" alt="345 tests">
  <img src="https://img.shields.io/badge/built%20with-Rust-111111?style=flat-square" alt="Rust">
</p>

<p align="center">
  <strong>Finds the file that had to change 60% of the time, against grep's 32% &middot; 4.6s to index 5,000 files &middot; never opens a socket</strong><br>
  <sub>Measured with a real model on 40 real merged ERPNext commits, with the index built at a commit <em>before</em> those changes existed, every condition on the same 4,000-token budget. On a second repository (OpenMRS, Java) the same measurement gives 45% against 41% — <strong>not a distinguishable win</strong>. Why the two differ is the most useful thing this benchmark found, and it is in <a href="#numbers">Numbers</a>. <a href="benchmarks/REPORT.md">Full writeup</a> &middot; <a href="#reproducing-the-benchmark">reproduce it</a>.</sub>
</p>

---

Every mature team has one. Eleven years on the same system. You point at a line and ask why it's there; they don't read the code, they say "the 2019 invoice thing" and walk off. Nothing they know is written down anywhere you can grep.

Reify puts them inside your AI agent.

## Contents

[Before / after](#before--after) · [Numbers](#numbers) · [How it works](#how-it-works) · [What it reads](#what-it-reads) · [Multilingual](#multilingual) · [Install](#install) · [Commands](#commands) · [Privacy](#privacy) · [Architecture](#architecture) · [Reproducing the benchmark](#reproducing-the-benchmark) · [Development](#development) · [FAQ](#faq)

## Before / after

You ask your agent to change the order approval threshold. It greps for `50000000`, finds one hit, changes it, and ships. It never learns that the BRD says corporate customers always need approval while the code has been quietly bypassing it since 2019.

With reify:

```
$ reify why erpnext/selling/doctype/sales_order/sales_order.py:812

  [CONFLICT] documentation and implementation disagree about approval
    documented   Corporate customers must require approval    docs/BRD-42.md:6
    observed     Corporate customers bypass approval          sales_order.py:812

  Called by     3 services, 1 batch job
  Writes        tabSales Order, approval_log
  History       8a31c2f  2019-04-17  fix: enterprise approval flow
```

Three of those four sections are things grep structurally cannot produce.

## Numbers

The honest measurement is a real model doing a real task: tickets taken from merged commits, where the prompt is the developer's own description of the change and the right answer is the files they actually touched. **The index is built at a commit before any of those changes existed**, so the code being asked for is genuinely absent.

Three of the five conditions exist to try to break the result rather than support it.

<p align="center">
  <img src="assets/benchmark-agent.svg" width="860" alt="Hit rate by condition for two repositories. ERPNext, 40 tasks: no context 22%, budget-matched grep 32%, reify 60%, decoy context 25%, perfect context 100%. OpenMRS, 22 tasks: no context 0%, grep 41%, reify 45%, decoy 14%, perfect 100%. Whiskers show 95% confidence intervals; reify and grep overlap on OpenMRS.">
</p>

| ERPNext, n=40 | | hit rate | 95% CI | recall |
|---|---|--:|--:|--:|
| no context at all | *memorisation control* | 22% | 12–38% | 0.16 |
| content grep, same budget | *baseline* | 32% | 20–48% | 0.27 |
| **reify** | | **60%** | **45–74%** | **0.54** |
| reify, another task's context | *decoy control* | 25% | 14–40% | 0.17 |
| perfect context | *ceiling* | 100% | 91–100% | 1.00 |

**Perfect context scores 100% where none scores 22%.** That 78-point gap is the whole space any retrieval tool can compete in, and it is wide — the one thing this benchmark had to establish before anything else mattered. **Reify recovers 49% of it. Grep recovers 13%.** A decoy of identical shape and size scores 25%, so the gain comes from what the context says, not from being handed a list of files.

### Retrieval on its own, no model involved

Before asking whether a model uses the context, ask whether the right file is in it.

<p align="center">
  <img src="assets/benchmark-retrieval.svg" width="860" alt="Share of tasks where a changed file was offered at all. ERPNext: content grep 10%, path grep 18%, reify 57%. OpenMRS: content grep 32%, path grep 18%, reify 41%.">
</p>

| ERPNext, n=40 | content grep | path grep | **reify** |
|---|--:|--:|--:|
| a changed file was offered | 10% | 18% | **57%** |
| mean recall | 0.08 | 0.16 | **0.50** |
| rank of the first correct file (MRR) | 0.07 | 0.12 | **0.23** |
| files put in front of the agent | 3 | 88 | 13 |
| latency | 45 ms | 0 ms | 59 ms |

Path grep offers 88 files to reach 18%. Offering everything is not retrieval.

### Where it doesn't work

Same method, second repository. OpenMRS, Java, 22 tasks:

| | hit rate | 95% CI |
|---|--:|--:|
| content grep | 41% | 23–61% |
| **reify** | **45%** | 27–65% |

Four points, intervals almost entirely overlapping. **On this repository Reify is not measurably better than grep**, and saying otherwise would be a lie the confidence intervals would catch.

The cause is measurable rather than mysterious. ERPNext *declares* 528 concepts in its entity metadata; OpenMRS declares 41. The rest Reify infers, and inferred vocabulary is weaker evidence than declared vocabulary — no amount of Rust changes that.

**The rule was never "index harder."** It is: the more a team has written its domain down — entity metadata, ORM mappings, a glossary, translation files — the more Reify has to work with. `reify concepts --suggest` exists to move a repository from the second case toward the first.

<details>
<summary><strong>Older numbers, and why they were wrong</strong></summary>

An earlier run indexed at `HEAD` rather than before each change, so the code being asked for was already present. Reify scored 55% and grep 40%.

That gap was too small, and wrong in the flattering direction for the wrong arm: new code contains the ticket's own words, so leakage helped the *lexical* baseline most. Fixing it moved grep 40% → 32% and Reify 55% → 60%.

An earlier retrieval-only run had a second flaw. It compared medians computed over each condition's *own* successes, so a tool that only ever solved easy tasks posted a flattering median precisely because it failed everywhere else. The report now also reports expected cost with a miss charged the full budget, and a paired comparison over only the tasks both conditions solved.

The leaky numbers are gone from the tables above. They stay here because a benchmark that quietly deletes its mistakes is not a benchmark.

</details>

## How it works

**Deterministic first. Semantic second. LLM last.** In this build there is no LLM at all unless you configure one, and every command still works without it.

```
1. Is it in the AST?          → symbols, calls, imports, inheritance
2. Is it in the data layer?   → tables, columns, ORM mappings, embedded SQL
3. Is it in a document?       → sections, cited by heading
4. Is it in git?              → who introduced it, what fixed it, what moves with it
5. Is it declared anywhere?   → glossary, entity metadata, translation files
6. Only then: infer it        → and mark it INFERRED, with the evidence attached
```

Every claim carries where it came from and how far to trust it:

| | |
|---|---|
| `CONFIRMED` | read straight out of a source file |
| `OBSERVED` | derived deterministically from confirmed facts |
| `INFERRED` | a heuristic. **Check the citation before acting on it** |
| `CONFLICTED` | two sources disagree. Resolve it before changing behaviour |
| `UNKNOWN` | explicitly unresolved, so absence is never read as evidence |

`Status::Unknown` is the `Default` on purpose. Anything that forgets to state its footing lands on the one an agent may not act on.

### Four bridges from business vocabulary to code

In precision order. The last is what makes Reify work on a repository that declares nothing at all.

| Bridge | Source | Available when |
|---|---|---|
| **Declared** | `.reify/glossary.toml`, entity metadata, ORM mappings | a human or a framework wrote it down |
| **Translation** | i18n tables, message bundles | the product has been localised |
| **Co-occurrence** | document headings that also name code | there is documentation |
| **Code vocabulary** | phrases the identifiers keep repeating | **always** |

The last runs only on what the others left uncovered, so it fills gaps instead of competing with better evidence. Boilerplate is filtered by measuring which words are ubiquitous *in this repository*, not from a curated list of `get`/`set`/`manager` that would fit one stack and no other.

## What it reads

**Code, 11 languages.** Python, TypeScript, JavaScript, Java, Go, C#, Rust, Ruby, PHP, C/C++, Kotlin, plus SQL. Each has a test asserting it yields containers, callables *and* calls — because a missing grammar node gives you an index that looks healthy and holds one symbol per file. That is not hypothetical; it shipped for Java, and the test now catches it.

**Documents, however the analyst wrote them.**

| | |
|---|---|
| Native | Markdown, plain text, HTML (Confluence exports included) |
| Zip + XML | DOCX, ODT, XLSX, PPTX |
| Delegated | PDF, legacy binary DOC, RTF |

Formats with no usable pure-Rust reader go to an external converter (`pdftotext`, `mutool`, `antiword`, `textutil`, `soffice`), tried in order. When none is installed, Reify **names every tool it tried and how to install it** rather than indexing nothing quietly.

**Whatever the team declared.** Frappe DocType JSON, Hibernate ORM mappings, Java and Spring `.properties` message bundles, i18n CSV tables. The highest-precision vocabulary a repository can offer, because the application itself reads it and so it stays true.

## Multilingual

No language is canonical, English included. Concept ids are opaque and every label carries a language tag, so a Vietnamese, Thai, Korean or German requirement reaches English code through the concept layer rather than an embedding model — which is why the answer arrives with a line number instead of a similarity score.

~60 locales recognised on translation files and message bundles. Obligation and exemption language detected in 11 languages, so a rule written in any of them is mined as a rule.

Three things that only break once you leave Latin script, each of which broke here first:

- **Thai, Lao, Khmer, Japanese and Chinese have no word spaces**, so a word index stores one enormous token and searching for a word *inside* it matches nothing. There is a trigram substring index for non-ASCII content; ASCII-only repositories never pay for it.
- **Korean glues particles to stems.** `승인` becomes `승인을`, and whole-word matching finds neither.
- **Sentence length cannot be counted in spaces**, or every Thai requirement is rejected as too short to be a rule.

## Install

```bash
cargo install --path crates/reify-cli
```

Then, in any repository:

```bash
reify init      # tells you what it will and won't index, and why
reify index     # 4.6s for 5,000 files; 0.7s after you edit one
```

<details>
<summary><strong>Shell completions</strong></summary>

```bash
reify completions zsh  > ~/.zfunc/_reify
reify completions bash > /etc/bash_completion.d/reify
reify completions fish > ~/.config/fish/completions/reify.fish
```

</details>

### Claude Code

Level 0 — the one the benchmark measured, and the one to start with:

```bash
reify init --write-agent-instructions
```

That appends a six-line block to your `AGENTS.md` or `CLAUDE.md`. No protocol, no server, no per-turn schema tax.

<details>
<summary><strong>A preflight hook, and keeping the index fresh</strong></summary>

Inject a risk header before every edit:

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

```
PREFLIGHT  erpnext/selling/doctype/sales_order/sales_order.py
  rules 7 · concepts 4 · tables 3 · dependants 22 · conflicts 1
  RISK: HIGH — documentation and implementation disagree about this file
```

Under 300 tokens, asserted by a test, because it runs on every edit. Non-blocking by default: a hook that blocks edits gets uninstalled, and then its warnings are lost too.

Keep the index current:

```bash
printf '#!/bin/sh\nreify index >/dev/null 2>&1 &\n' > .git/hooks/post-merge
chmod +x .git/hooks/post-merge
cp .git/hooks/post-merge .git/hooks/post-checkout
```

</details>

### Codex, Cursor, OpenCode, Aider, Pi, Windsurf, Cline

No adapter needed — Reify is a CLI. Put this in whatever instruction file the tool reads (`AGENTS.md`, `.cursorrules`, `CONVENTIONS.md`, `.windsurfrules`, `.clinerules/`):

```markdown
Before changing code here, run `reify context "<what you are about to do>"`.
Run `reify why <file>:<line>` before modifying unfamiliar logic.
Run `reify impact "<symbol>"` before changing anything shared.
Treat INFERRED claims as leads to verify, not facts.
```

### MCP

```bash
reify serve --mcp
```

Three tools — `reify_context`, `reify_why`, `reify_impact` — and three is the whole surface. An MCP server's schemas are re-sent every turn of every session, so a tool built to save context should not charge rent to deliver it. A test asserts the schemas cost under 600 tokens.

### Optional: a model

There is no default provider and nothing is enabled until you say so.

```toml
# .reify/llm.toml
command = ["ollama", "run", "llama3"]
```

Reify writes the prompt to that command's stdin, or substitutes a `{prompt}` argument. See [Privacy](#privacy) for why it is a command and not an HTTP client.

## Commands

| Command | What it does |
|---|---|
| `reify context "<task>"` | The minimum knowledge for a change, plus a reading plan. **The one that matters.** |
| `reify why <file>:<line>` | What this is, what calls it, what data it touches, what changed it |
| `reify impact "<symbol>"` | What depends on it — including through the database, where no call edge exists |
| `reify explain "<term>"` | A business concept across every language, table and file it appears in |
| `reify flow "<process>"` | The call sequence that carries out a business process |
| `reify conflicts` | Documentation that disagrees with the code |
| `reify rules` | Mined business rules, with evidence |
| `reify concepts --suggest` | Turn what was mined into glossary entries you edit down |
| `reify preflight <file>` | A risk header for an editor hook |
| `reify report` | System scorecard |
| `reify status` | Freshness, coverage, and what was skipped |
| `reify llm status \| preview` | Is a model configured, and exactly what would be sent |
| `reify serve --mcp` | Model Context Protocol over stdio |
| `reify completions <shell>` | Completion script |

Everything takes `--json` against a versioned schema and `--budget <tokens>`. Full output shapes: [docs/json-schema/](docs/json-schema/).

## Privacy

**Reify opens no network connection.** Not "by default" — at all. There is no HTTP client in the dependency tree, and `cargo test` fails if one appears.

| | |
|---|---|
| Networking crates in `Cargo.lock` | asserted zero, in CI |
| Sockets in the source | asserted zero, in CI |
| Subprocesses | `git`, and reviewed document converters — each named in a test |
| Code from your repo, executed | never. tree-sitter parses; it does not run |
| The store | `.reify/`, gitignored by `reify init` |

Model assistance is a command **you** configure, not an embedded client. Local models work with no extra code, no credential passes through Reify, `reify llm preview` prints the exact bytes before any are sent, and `REIFY_OFFLINE=1` makes it unreachable no matter what a config file says.

Full threat model, including what is **not** covered: [docs/privacy.md](docs/privacy.md).

## Architecture

One SQLite file per repository. No graph database, no vector store, no daemon.

```
  LAYER 4  Synthesis    optional model, cached, always INFERRED        llm.rs
  LAYER 3  Selection    seed → spread → budget knapsack → render       context.rs
  LAYER 2  Semantics    concepts, rules, conflicts       concepts.rs · rules.rs
  LAYER 1  Structure    symbols, calls, tables, sections, commits  extract/ · gitlog.rs
  LAYER 0  Substrate    walk, classify, hash, store      discover.rs · store.rs
```

**An incremental index is byte-identical to a full rebuild**, asserted by a property test that applies random edit sequences and compares canonical dumps. Each stage owns a disjoint set of edge kinds and its own invalidation trigger, which is what makes that true. Details: [docs/architecture.md](docs/architecture.md), design rationale and kill criteria: [docs/PLAN.md](docs/PLAN.md).

### Measured performance

ERPNext, 5,064 files, 8-core M-series laptop.

| | measured | target | |
|---|--:|--:|---|
| full index, no model | 4.6 s | < 10 min | ✅ |
| reindex, nothing changed | 0.6 s | — | ✅ |
| reindex, one file edited | 0.7 s | < 500 ms | ~ |
| `reify context` | 57 ms | < 100 ms | ✅ |
| `reify impact` | 0.2 ms | < 50 ms | ✅ |
| `reify why` | 205 ms | < 20 ms | ❌ a `git log -L` subprocess; ~5 ms without |
| peak memory, full index | 224 MB | < 2 GB | ✅ |
| store size | 47 MB (33% of a 144 MB working tree) | < 5% | ❌ |

A full index took **78 seconds** until the full-text index was keyed by node id. `uid` is `UNINDEXED` in FTS5, so `DELETE ... WHERE uid = ?` scanned the whole table once per node — quadratic, and invisible until it was timed per stage. Editing one file took **5.9 seconds** until the repository-wide stages learned to skip when their inputs are provably unchanged.

`REIFY_TIMING=1 reify index` prints the per-stage breakdown that found both.

## Reproducing the benchmark

Nothing in the tables above was typed in by hand. The task sets, raw per-task outcomes and the charts are all committed.

```bash
# 1. Freeze a task set from real merged commits, ending before a chosen base
reify-bench tasks --repo <repo> --after <base-sha> --out benchmarks/tasks/mine.json

# 2. Index at that base, so the change being asked for is genuinely absent
git worktree add /tmp/base <base-sha>
reify -C /tmp/base init && reify -C /tmp/base index

# 3. Retrieval, then with a model
reify-bench run   --repo /tmp/base --tasks benchmarks/tasks/mine.json --out results/
REIFY_LLM_COMMAND='<your model cli> {prompt}' \
reify-bench agent --repo /tmp/base --tasks benchmarks/tasks/mine.json --out results/

# 4. Report and charts, generated from the raw results
reify-bench report --in results/ --out benchmarks/REPORT.md
reify-bench chart  --results "Mine=results/" --out assets/
```

The task set is frozen before any condition runs. The report includes a **"Where Reify lost"** section listing every task the baseline won, and it is a required part of the document rather than an optional one.

## Development

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
cargo bench -p reify
```

All enforced in CI, along with a full test run under blocked network egress. `cargo test` includes `crates/reify/tests/offline.rs`, which fails the build if a networking crate enters the dependency tree.

Fixtures live in [`fixtures/minierp`](fixtures/) — a small business system with *planted* knowledge: a documented rule, code that contradicts it, a magic number, a bilingual concept, cross-module coupling that exists only through a shared table. Every claim Reify makes about it has a known right answer, so a failure there is unambiguous.

Adding a language is a grammar, a node-kind mapping, a `classify` case and a golden test. See [CONTRIBUTING.md](CONTRIBUTING.md) for the design rules that are load-bearing rather than stylistic.

## FAQ

**Do I have to write a glossary?**
No, and Reify works without one. It also gets visibly better with one, which is the entire finding in [Numbers](#numbers). `reify concepts --suggest` writes you a first draft to edit down.

**Is this another RAG thing?**
There is no vector database, no embedding model and no chunking. Retrieval is lexical and graph-based, which is why every answer comes with a line number instead of a similarity score.

**My repo is 3,000 lines. Should I use it?**
No. Use ripgrep. Under roughly 20k LOC Reify buys you nothing a grep and a scroll wheel don't.

**Does it send my proprietary code anywhere?**
It cannot. There is no HTTP client in the binary, and a test fails the build if one appears. If you configure a model provider, that is a command you chose, and `reify llm preview` shows the exact bytes first.

**Why is `reify why` slower than everything else?**
It shells out to `git log -L` for precise line history. 205 ms with it, ~5 ms without. Still on the list.

**Conflicts found nothing in my repo. Is it broken?**
Probably not. Detection requires five conditions to hold at once and is biased hard toward silence, because a conflict detector that cries wolf gets switched off in week two and takes its true positives with it. It finds zero on ERPNext, which ships almost no specification prose, and exactly one on the fixture, where one is planted.

**What does "reify" mean?**
To make an abstract thing concrete. The knowledge was always there; it just wasn't a file.

## Status

Early, and measured. Known misses, all documented rather than buried: the store is 33% of the working tree against a 5% target, `reify why` is 205 ms against 20 ms, and Windows is untested.

[docs/PLAN.md](docs/PLAN.md) contains the kill criteria — the conditions under which this project should be considered a failure and the thesis changed. They are written down because a project that cannot say when to stop is not being engineered, it is being believed in.

## License

[Apache-2.0](LICENSE). Patent grant included, so an agent vendor can actually ship it.
