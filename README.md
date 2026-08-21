<h1 align="center">Reify</h1>

<p align="center">
  <em>Your agent doesn't know why that line is there. Reify does.</em>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/license-Apache--2.0-111111?style=flat-square" alt="Apache-2.0">
  <img src="https://img.shields.io/badge/languages-11-111111?style=flat-square" alt="11 languages">
  <img src="https://img.shields.io/badge/doc%20formats-10-111111?style=flat-square" alt="10 document formats">
  <img src="https://img.shields.io/badge/network%20calls-0-111111?style=flat-square" alt="Zero network calls">
  <img src="https://img.shields.io/badge/tests-338-111111?style=flat-square" alt="338 tests">
</p>

<p align="center">
  <strong>Finds the right file 60% of the time vs grep's 32% &middot; 4.6s to index 5,000 files &middot; never opens a socket</strong><br>
  <sub>Measured with a real model on real merged commits from ERPNext, indexed at a commit <em>before</em> those changes were made, every condition on the same 4,000-token budget. On a second repository (OpenMRS, Java) the same measurement gives 45% vs 41% — <strong>not a distinguishable win</strong>. Why the two differ is the most useful thing this benchmark found, and it's in <a href="#numbers">Numbers</a>. <a href="benchmarks/REPORT.md">Full writeup</a> &middot; <a href="benchmarks/">reproduce it</a>.</sub>
</p>

---

Every mature team has one. Eleven years on the same system. You point at a line and ask why it's there; they don't read the code, they say "the 2019 invoice thing" and walk off. Nothing they know is written down anywhere you can grep.

Reify puts them inside your AI agent.

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

The honest measurement is a real model doing a real task: 40 tickets taken from merged ERPNext commits, where the prompt is the developer's own description and the answer is the files they actually changed. **The index is built at a commit before any of those changes existed**, so the code being asked for is genuinely absent.

Three of the five conditions exist to try to break the result, not support it.

| ERPNext, n=40 | | hit rate | 95% CI |
|---|---|--:|--:|
| no context at all | *memorisation control* | 22% | 12–38% |
| content grep, same budget | *baseline* | 32% | 20–48% |
| **reify** | | **60%** | **45–74%** |
| reify, another task's context | *decoy control* | 25% | 14–40% |
| perfect context | *ceiling* | 100% | 91–100% |

Perfect context scores 100% where none scores 22%. That 78-point gap is the whole space any retrieval tool can compete in, and it is wide — which was the one thing this benchmark had to establish before anything else mattered. **Reify recovers 49% of it. Grep recovers 13%.** A decoy of identical shape and size scores 25%, so the gain comes from what the context says, not from being handed a list.

### Where it doesn't work

Same method, second repository. OpenMRS, Java, 22 tasks:

| | hit rate | 95% CI |
|---|--:|--:|
| content grep | 41% | 23–61% |
| **reify** | **45%** | 27–65% |

Four points, intervals almost fully overlapping. **On this repository Reify is not measurably better than grep**, and saying otherwise would be a lie the confidence intervals would catch.

The cause is measurable. ERPNext *declares* 528 concepts in its entity metadata; OpenMRS declares 41. The rest Reify infers, and inferred vocabulary is weaker evidence than declared vocabulary — no amount of Rust changes that.

**The rule was never "index harder."** It is: the more a team has written its domain down — entity metadata, ORM mappings, a glossary, translation files — the more Reify has to work with. `reify concepts --suggest` exists to move a repository from the second case toward the first.

<details>
<summary><strong>Older numbers, and why they were wrong</strong></summary>

An earlier run indexed at `HEAD` instead of before each change, so the code being asked for was already present. Reify scored 55% and grep 40%.

That gap was too small, and in the flattering direction for the wrong arm: the new code contains the ticket's own words, so leakage helped the *lexical* baseline most. Fixing it moved grep 40% → 32% and Reify 55% → 60%.

The leaky numbers are gone from this README. They stay here because a benchmark that quietly deletes its mistakes is not a benchmark.

</details>

## How it works

**Deterministic first. Semantic second. LLM last.** In this build there is no LLM at all unless you configure one, and every command still works.

```
1. Is it in the AST?          → symbols, calls, imports, inheritance
2. Is it in the data layer?   → tables, columns, ORM mappings, embedded SQL
3. Is it in a document?       → sections, cited by heading
4. Is it in git?              → who introduced it, what fixed it, what moves with it
5. Is it declared anywhere?   → glossary, entity metadata, translation files
6. Only then: infer it        → and mark it INFERRED, with the evidence
```

Every claim carries where it came from and how much to trust it:

| | |
|---|---|
| `CONFIRMED` | read straight out of a source file |
| `OBSERVED` | derived deterministically from confirmed facts |
| `INFERRED` | a heuristic. **Check the citation before you act on it** |
| `CONFLICTED` | two sources disagree. Resolve it before changing behaviour |
| `UNKNOWN` | explicitly unresolved, so absence is never read as evidence |

`Status::Unknown` is the `Default` on purpose. Anything that forgets to state its footing lands on the one an agent may not act on.

## What it reads

**Code, 11 languages.** Python, TypeScript, JavaScript, Java, Go, C#, Rust, Ruby, PHP, C/C++, Kotlin, plus SQL. Each has a test asserting it yields containers, callables *and* calls — because a missing grammar node gives you an index that looks healthy and holds one symbol per file. That is not hypothetical; it shipped, and the test now catches it.

**Documents, however the analyst wrote them.** Markdown, text, HTML, DOCX, legacy binary DOC, ODT, RTF, XLSX, PPTX, PDF. Formats with no usable pure-Rust reader get delegated to an external converter, and when none is installed Reify says so loudly rather than indexing nothing quietly.

**Whatever the team declared.** Frappe DocType JSON, Hibernate mappings, Spring `.properties` bundles, i18n CSV tables. The highest-precision vocabulary a repo can offer, because the application itself reads it and so it stays true.

## Multilingual

No language is canonical, English included. A Vietnamese, Thai, Korean or German requirement reaches English code through the concept layer — not an embedding model — which is why it is deterministic and citable.

~60 locales on translation files. Obligation and exemption language in 11 languages, so a rule written in any of them is mined as a rule.

Three things that only break once you leave Latin script, each of which broke here first:

- **Thai, Lao, Khmer, Japanese and Chinese have no word spaces**, so a word index stores one giant token and searching for a word *inside* it finds nothing. There's a trigram substring index for non-ASCII content; ASCII repos never pay for it.
- **Korean glues particles to stems.** `승인` becomes `승인을`. Whole-word matching finds neither.
- **Sentence length can't be counted in spaces**, or every Thai requirement is rejected as too short to be a rule.

## Install

```bash
cargo install --path crates/reify-cli

cd your-repo
reify init      # tells you what it will and won't index, and why
reify index     # 4.6s for 5,000 files; 0.7s after you edit one
```

Then tell your agent, or let `reify init --write-agent-instructions` do it:

```markdown
Before changing code here, run `reify context "<what you are about to do>"`.
Run `reify why <file>:<line>` before modifying unfamiliar logic.
Treat INFERRED claims as leads to verify, not facts.
```

That's the integration. No protocol, no server, no per-turn schema tax. `reify serve --mcp` exists for clients that can't run a shell command — three tools, and three is the whole surface, because an MCP server's schemas are re-sent every turn and a tool built to save context shouldn't charge rent to deliver it.

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
| `reify preflight <file>` | A risk header for an editor hook. Under 300 tokens, asserted |
| `reify report` | System scorecard |

Everything takes `--json` against a versioned schema and `--budget <tokens>`.

## Privacy

**Reify opens no network connection.** Not "by default" — at all. There is no HTTP client in the dependency tree, and `cargo test` fails if one appears.

Model assistance is a command *you* configure, not an embedded client:

```toml
# .reify/llm.toml
command = ["ollama", "run", "llama3"]
```

Local models work with no extra code, no credential ever passes through Reify, `reify llm preview` prints the exact bytes before any are sent, and `REIFY_OFFLINE=1` makes it unreachable no matter what a config file says. Full threat model, including what is **not** covered: [docs/privacy.md](docs/privacy.md).

## FAQ

**Do I have to write a glossary?**
No, and Reify works without one. It also gets visibly better with one, which is the whole finding in [Numbers](#numbers). `reify concepts --suggest` writes you a first draft to edit down.

**Is it another RAG thing?**
There is no vector database, no embedding model, and no chunking. Retrieval is lexical and graph-based, which is why every answer comes with a line number instead of a similarity score.

**My repo is 3,000 lines. Should I use it?**
No. Use ripgrep. Under roughly 20k LOC Reify buys you nothing that a grep and a scroll wheel don't.

**Why is `reify why` slower than everything else?**
It shells out to `git log -L` for precise line history. 205ms with it, ~5ms without. That one is still on the list.

**What does "reify" mean?**
To make an abstract thing concrete. The knowledge was always there; it just wasn't a file.

## Status

Early, and measured. The full plan, including the conditions under which this project should be considered a failure, is in [docs/PLAN.md](docs/PLAN.md) — the kill criteria are written down because a project that can't say when to stop isn't being engineered, it's being believed in.

Known misses, all in [docs/](docs/): store is 33% of the working tree against a 5% target, `reify why` is 205ms against 20ms, Windows is untested.

## License

[Apache-2.0](LICENSE). Patent grant included, so an agent vendor can actually ship it.
