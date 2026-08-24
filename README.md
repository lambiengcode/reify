<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/logo-dark.png">
    <img src="assets/logo.png" width="300" alt="Reify">
  </picture>
</p>

<p align="center">
  <em>The business logic lives in one person's head.<br>Reify gets it out, without asking them to write documentation.</em>
</p>

<p align="center">
  <sub>Already installed? Run <code>reify upgrade</code></sub>
</p>

<p align="center">
  <a href="https://github.com/lambiengcode/reify/actions/workflows/ci.yml"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/lambiengcode/reify/ci.yml?style=flat-square&label=ci" /></a>
  <a href="https://github.com/lambiengcode/reify/releases/latest"><img alt="Release" src="https://img.shields.io/github/v/release/lambiengcode/reify?style=flat-square&color=blue" /></a>
  <a href="https://lambiengcode.github.io/reify/"><img alt="Documentation" src="https://img.shields.io/badge/docs-lambiengcode.github.io-2da44e?style=flat-square" /></a>
  <a href="LICENSE"><img alt="Apache-2.0" src="https://img.shields.io/github/license/lambiengcode/reify?style=flat-square&color=blue" /></a>
  <a href="#swe-bench-verified"><img alt="SWE-bench retrieval 87.0%" src="https://img.shields.io/badge/SWE--bench%20retrieval-87.0%25-blueviolet?style=flat-square" /></a>
  <a href="#privacy"><img alt="network calls: 0" src="https://img.shields.io/badge/network%20calls-0-success?style=flat-square" /></a>
</p>

<p align="center">
  <a href="#wire-it-into-your-agent"><img alt="Claude Code" src="https://img.shields.io/badge/Claude%20Code-supported-2da44e?style=flat-square" /></a>
  <a href="#wire-it-into-your-agent"><img alt="Cursor" src="https://img.shields.io/badge/Cursor-supported-2da44e?style=flat-square" /></a>
  <a href="#wire-it-into-your-agent"><img alt="Codex" src="https://img.shields.io/badge/Codex-supported-2da44e?style=flat-square" /></a>
  <a href="#wire-it-into-your-agent"><img alt="OpenCode" src="https://img.shields.io/badge/OpenCode-supported-2da44e?style=flat-square" /></a>
  <a href="#wire-it-into-your-agent"><img alt="MCP" src="https://img.shields.io/badge/MCP-6%20tools-2da44e?style=flat-square" /></a>
</p>

<p align="center">
  <strong>On SWE-bench Verified, Reify puts the file that had to change in front of the model 87.0% of the time — grep manages 6.6% &middot; 500 real issues, someone else's benchmark &middot; never opens a socket</strong><br>
  <sub>A real model on 142 tasks from real merged commits across ERPNext, OFBiz, OpenMRS and Medusa, each index built at a commit <em>before</em> those changes existed. That is <em>retrieval</em>: the right file, in front of the model. On end-to-end patch correctness Reify is <em>ahead of</em> a BM25 baseline but not significantly so, and <a href="#end-to-end-ahead-but-not-yet-proven">the section saying so</a> is as prominent as this one. <a href="benchmarks/REPORT.md">Full writeup</a> &middot; <a href="#reproducing-the-benchmark">reproduce it</a>.</sub>
</p>

<p align="center">
  <img src="assets/demo.gif" width="920" alt="A terminal feature tour against a real ERPNext index: reify index rebuilds the graph; reify context compiles a briefing for adding a discount tier under a 1,500-token budget; reify why on one line of customer.py returns its callers, the tables it writes, the files it co-changes with and the 2022-2025 commits that explain it; reify impact traces the blast radius of check_credit_limit through multiple hops; reify explain shows the credit-limit concept across every file it appears in; and reify context --toon emits the same facts in the agent format.">
</p>

<p align="center">
  <sub>Every command in the demo is real, against a real ERPNext index. The recording script is <a href="assets/demo.tape">committed</a> (recorded with <a href="https://github.com/charmbracelet/vhs">vhs</a>); if the GIF ever disagrees with the tool, re-record the GIF.</sub>
</p>

## Two minutes to first answer

```bash
curl -fsSL https://raw.githubusercontent.com/lambiengcode/reify/main/install.sh | sh
cd your-repository
reify init --write-agent-instructions   # wires your agent through AGENTS.md / CLAUDE.md
reify index                             # 4.2 s for 5,000 files; 0.5 s after one edit
reify context "the change you are about to make" --toon
```

<sub>One static binary — no daemon, no config, no API key, and every release ships a
SHA-256 checksum that both the installer above and `reify upgrade` verify before
anything is unpacked. Changed your mind?
`reify uninstall` removes the binary and `reify uninit` cleans one repository, both
showing their plan first. Per-agent wiring, hooks and MCP: <a href="#install">Install</a>.</sub>

<p align="center">
  <strong>English</strong> &middot; <a href="README.vi.md">Tiếng Việt</a> &middot; <a href="README.zh.md">简体中文</a>
</p>

---

**Contents**

- [Two minutes to first answer](#two-minutes-to-first-answer) · [the one-person problem](#the-one-person-problem) · [what it gives you](#what-it-actually-gives-you) · [before / after](#before--after)
- **Numbers:** [SWE-bench Verified](#swe-bench-verified) · [end to end](#end-to-end-ahead-but-not-yet-proven) · [four repositories](#four-repositories-chosen-to-hurt) · [where it doesn't work](#where-it-doesnt-work)
- **Using it:** [install](#install) · [wire it into your agent](#wire-it-into-your-agent) · [commands](#commands) · [privacy](#privacy)
- **Under it:** [how it works](#how-it-works) · [what it reads](#what-it-reads) · [multilingual](#multilingual) · [architecture](#architecture) · [reproducing the benchmark](#reproducing-the-benchmark)
- [FAQ](#faq) · [development](#development) · [license](#license)

## The one-person problem

Your system is eleven years old. The business logic is enormous and mostly
undocumented — there are BA documents in SharePoint from 2019, and some of them are
still true.

**One person understands it.** They can't take a holiday without a phone. You can't
hire around them, because a new developer needs the better part of a year before they
are useful, and the knowledge they would need to absorb isn't written down anywhere —
it's in that one person's head, and they are far too busy to write it down.

So you point an AI coding agent at it. The agent is brilliant on new code and useless
here. It reads the wrong forty files, misses the rule that mattered, and confidently
changes behaviour a customer depends on. Then the one person has to review it, which
was the bottleneck you were trying to remove.

**Reify gets that knowledge out of one head and into a form your agents and your new
hires can both use — without asking anyone to write documentation they will never
write.** It compiles what already exists: the code, the BA documents nobody reads, the
database schema, and eleven years of commit messages explaining why.

### Does this sound like your codebase?

- [x] Older than the newest person on the team
- [x] Business rules spread across code, stored procedures, config and someone's memory
- [x] No developer documentation. Some BA documents in Word or PDF, of uncertain age
- [x] "Ask Minh, he wrote that" is a normal answer to a technical question
- [x] Onboarding is measured in months
- [x] The documentation you *do* have disagrees with the code, and nobody knows where
- [x] AI agents work fine on your side projects and fall apart on this
- [x] Source code cannot leave the building

Reify was built for exactly this. If none of it sounds familiar, you probably don't
need it — see the [FAQ](#faq).

## What it actually gives you

Three questions, answered from evidence rather than from a model's recollection:

| Question | Command | Who asks it |
|---|---|---|
| *Why does this code exist?* | `reify why <file>:<line>` | the new hire, on day two |
| *What breaks if I change it?* | `reify impact "<symbol>"` | the person doing the change |
| *What must I know before I start?* | `reify context "<task>"` | **your AI agent, every time** |

The third is the one that matters. It hands an agent the smallest set of rules,
citations, code spans and known contradictions it needs — and nothing else.

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

## SWE-bench Verified

The four-repository benchmark below is ours, which is exactly the reason to also run
somebody else's. **[SWE-bench Verified](https://openai.com/index/introducing-swe-bench-verified/)**
is 500 real GitHub issues from twelve well-known Python projects, each pinned to the
`base_commit` the issue was filed against — the same index-before-the-change protocol
Reify's own benchmark uses, written by other people. The task is a plain issue report;
the right answer is the set of files the accepted fix actually touched.

| retrieval on SWE-bench Verified, n=500 | offered a file the fix touched | MRR | offered **every** such file | median tokens |
|---|--:|--:|--:|--:|
| grep, content | 6.6% <sub>[4.7–9.1]</sub> | 0.06 | 5.6% | 3,998 |
| grep, paths | 9.0% <sub>[6.8–11.8]</sub> | 0.06 | 7.8% | 3,996 |
| **reify**, one round | **72.6%** <sub>[68.5–76.3]</sub> | 0.42 | 65.4% | **3,670** |
| **reify**, three rounds | **87.0%** <sub>[83.8–89.7]</sub> | 0.43 | 81.4% | 9,549 |

**A single round of Reify beats grep on 342 instances and loses on 12 — while spending
fewer tokens** (3,670 against 3,998). Three rounds win 406 to 4 (exact McNemar
p ≈ 9 × 10⁻¹¹⁵). This is not a close measurement, and it is the cleanest number in this
README precisely because the tasks, the repositories and the ground truth all came from
somewhere else.

Per repository, three rounds against content-grep:

| | grep | reify ×3 | | | grep | reify ×3 |
|---|--:|--:|---|---|--:|--:|
| django (n=231) | 6% | **90%** | | astropy (n=22) | 0% | **77%** |
| sympy (n=75) | 7% | **79%** | | xarray (n=22) | 9% | **95%** |
| sphinx (n=44) | 7% | **80%** | | pytest (n=19) | 26% | **95%** |
| matplotlib (n=34) | 0% | **97%** | | pylint (n=10) | 10% | **60%** |
| scikit-learn (n=32) | 9% | **88%** | | requests (n=8) | 0% | **100%** |

**What this does and does not show.** It measures retrieval — whether the files that
had to change are put in front of the model — not whether the model then writes a
correct patch. Verified is Python-only, so it says nothing about the modern-TypeScript
weakness [below](#where-it-doesnt-work). And these repositories are famous enough that
models have partly memorised them; that affects a model's *answers*, not which files a
retriever offers, and every arm here ran against the same index at the same commit.
Reproduce it with the driver in [`benchmarks/swe/`](benchmarks/swe/).


### End to end: ahead, but not yet proven

Retrieval is not the final claim — resolving the issue is. The same benchmark, run
through the SWE-bench paper's own protocol (one model, one budget, the retriever as the
only difference), with every patch judged by the **official SWE-bench harness**:

| resolved the issue, 101 instances graded under both arms | | |
|---|--:|---|
| BM25 | 67.3% | 68 resolved, 1 empty patch |
| **Reify** | **73.3%** | 74 resolved, 2 empty patches |
| | | paired 12–6, exact McNemar **p = 0.24** |

**Read that p-value before the percentages.** Reify resolved six more issues and won
twice as many disagreements as it lost — but at this sample size that is not a
statistically significant result. The honest summary is *ahead and not yet proven*, not
a win. Two hundred more instances would settle it; today's evidence supports "Reify does
not cost you anything end to end, and probably helps", nothing stronger.

<sub>**These absolute rates are not comparable to earlier ones published here.** That run
used DeepSeek; this one uses Claude Sonnet, because the DeepSeek account ran out of
balance mid-project. A stronger model lifts both arms — the earlier run resolved about
24% on each side. What survives a model change is the *paired* comparison, because both
arms always answer the same instance with the same model, and that is what the table
above reports. Raw per-instance outcomes: [`benchmarks/swe/results/stage2-endtoend.json`](benchmarks/swe/results/stage2-endtoend.json).</sub>

It has not always been ahead. The first attempt was a **loss**, and what closed the gap
is the interesting part.

Reify retrieved the right file far more often and the model still did worse. Rebuilding
the exact prompts showed why: a context window filled with whole files in rank order
spends itself on whatever ranks first, and file-rank order is blind to file size where
BM25 has length normalisation built in. **The gold file was retrieved and then never
shown** — visible in only 27% of prompts against BM25's 40%.

`reify context --for-edit` fixes that at the source: regions padded to whole
definitions, the file's imports included once, overlapping regions merged, budget still
hard. Nothing retrieved is lost at the window any more:

| | gold file retrieved | **visible in the prompt** |
|---|--:|--:|
| BM25 | 60.0% | 40.0% |
| Reify, whole files | 76.7% | 26.7% |
| **Reify `--for-edit`** | **80.0%** | **56.7%** |

Capping how much of the window a single file may claim was tried early, **rejected on
evidence**, and only later adopted in the form that works. Truncating a file's *content*
made things worse, because a truncated file is not editable either. Capping how many
*symbols* one file contributes to the selection — upstream of the window, leaving each
region whole — is what improved retrieval, and it is what ships today. Cost-aware
ranking was tried and rejected outright: it cut retrieval by seven points while buying
nothing once regions made file size irrelevant.

So: Reify wins retrieval decisively and is ahead, inconclusively, on final patch
success. The remaining constraint is the patch-writing loop rather than the context —
both arms leave roughly a quarter of issues unresolved with the file that had to change
sitting in the prompt.

## Four repositories chosen to hurt

The honest measurement is a real model doing a real task: tickets taken from merged
commits, where the prompt is the developer's own description of the change and the
right answer is the files they actually touched. **Every index is built at a commit
before any of those changes existed**, so the code being asked for is genuinely absent.
Four repositories, chosen partly to hurt; several conditions exist to break the result
rather than support it.

<p align="center">
  <img src="assets/benchmark-agent.svg" width="860" alt="Hit rate by condition for four repositories, whiskers are 95% confidence intervals. ERPNext, 40 tasks: no context 22%, grep at tripled budget 50%, reify three rounds 75%, perfect context 100%. OFBiz, 40 tasks: 0%, 28%, 78%, 100%. OpenMRS, 22 tasks: 0%, 32%, 59%, 100%. Medusa, 40 tasks: 0%, 24%, 26%, 100% — reify and grep overlap on Medusa.">
</p>

The headline comparison is cost-matched: Reify iterates three rounds (an agent that
reads, doesn't find it, and asks again — with the already-read files excluded), so the
control is grep handed the same tripled budget outright.

| model-in-the-loop, hit rate | grep ×3 budget | **reify ×3 rounds** | margin | 95% CIs overlap? |
|---|--:|--:|--:|---|
| ERPNext (Python/JS), n=40 | 50% | **75%** | +25 | barely |
| OFBiz (Java + XML), n=40 | 28% | **78%** | +50 | no |
| OpenMRS (Java), n=22 | 32% | **59%** | +27 | barely |
| Medusa (modern TS), n=40 | 24% | **26%** | +2 | **fully — no win** |

> **A note on the fourth row:** Medusa's +2 is a tie, not a win, and it stays in the
> headline table at full prominence rather than in a footnote. What separates the
> repositories where Reify wins big from the one where it doesn't is measured, not
> guessed — see [Where it doesn't work](#where-it-doesnt-work).

**The controls, on every repository:** perfect context scores 100% everywhere, so
retrieval quality is the entire game. A decoy context of identical shape scores 0–12%,
so the content is doing the work, not the format. With no repository access the model
scores 0% on three repositories and **22% on ERPNext** — it has partially memorised the
most famous repo, which is exactly why the other three exist and why every headroom
figure subtracts that floor. Seven of ~1,000 provider calls failed, all on Medusa runs;
failed calls are excluded from rates, never scored as misses.

Single-shot, for the record: reify 55/68/41/15 against grep 30/12/41/21 at equal
single budget — on OFBiz a *single* reify round already beats grep by 56 points.

### Retrieval on its own, no model involved

<p align="center">
  <img src="assets/benchmark-retrieval.svg" width="860" alt="Share of tasks where a changed file was offered, per repository. ERPNext: grep 10%, path grep 18%, reify 57%, reify three rounds 75%. OFBiz: 12%, 15%, 70%, 78%. OpenMRS: 32%, 18%, 41%, 55%. Medusa: 18%, 18%, 18%, 28%.">
</p>

| a changed file was offered | grep | reify (MRR) | **reify ×3** |
|---|--:|--:|--:|
| ERPNext | 10% | 57% (0.45) | **75%** |
| OFBiz | 12% | 70% (0.45) | **78%** |
| OpenMRS | 32% | 41% (0.27) | **55%** |
| Medusa | 18% | 18% (0.09) | **28%** |

### Where it doesn't work

**Medusa** — a modern, well-factored TypeScript monorepo — is the open problem, and it
inverts the project's founding assumption. The legacy Java systems were supposed to be
the hard case; they are the *best* cases. Medusa's tasks describe UI behaviour
("remove the duplicate cloud auth button") whose vocabulary barely intersects the
code, its history is squashed PR merges, and nothing Reify currently reads closes that
gap. Iteration lifts it 18→28% on retrieval; with the model, 26% against grep's 24%; the
intervals overlap completely.

The earlier hypothesis — "Reify's advantage scales with declared vocabulary" — did not
survive the four-repo test either. OFBiz declares little the way ERPNext does, yet
shows the largest margin of all. What the four repositories actually separate on is
whether *commit history and file naming speak the vocabulary the tasks are written
in*. Where they do, Reify recovers 54–62% of the oracle gap. Where they don't
(Medusa), it is grep with better structure.

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

**Documents, however the analyst wrote them.** This is the part most code tools skip,
and it is the only documentation many of these systems have.

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
curl -fsSL https://raw.githubusercontent.com/lambiengcode/reify/main/install.sh | sh
```

Prebuilt binaries for macOS (Apple Silicon and Intel), Linux (x86_64 and aarch64)
and Windows (x86_64).
Or build from source:

```bash
cargo install --path crates/reify-cli
```

Then, in any repository:

```bash
reify init      # tells you what it will and won't index, and why
reify index     # 4.6s for 5,000 files; 0.7s after you edit one
```

**Stay current, leave cleanly.** `reify upgrade` replaces the binary with the latest
release — through `curl` and `tar` as visible subprocesses, never an embedded HTTP
client, with the checksum verified before anything is installed; `--check` only asks,
and `REIFY_OFFLINE=1` refuses the command outright. `reify uninstall --yes` removes the
binary and nothing else; `reify uninit --yes` removes one repository's `.reify/` store
and the instruction block `init` wrote. Both show their plan first when run without
`--yes`.

<details>
<summary><strong>Shell completions</strong></summary>

```bash
reify completions zsh  > ~/.zfunc/_reify
reify completions bash > /etc/bash_completion.d/reify
reify completions fish > ~/.config/fish/completions/reify.fish
```

</details>

### Wire it into your agent

```bash
reify init --write-agent-instructions
```

Appends a six-line block to `AGENTS.md` or `CLAUDE.md`. No protocol, no server, no
per-turn schema tax — this is the level the benchmark measured. For tools that read a
different file (`.cursorrules`, `CONVENTIONS.md`, `.windsurfrules`, `.clinerules/`),
paste the same four lines:

```markdown
Before changing code here, run `reify context "<what you are about to do>" --toon`.
Run `reify why <file>:<line>` before modifying unfamiliar logic.
Run `reify impact "<symbol>"` before changing anything shared.
Treat INFERRED claims as leads to verify, not facts.
```

**MCP**, if you prefer it: `reify serve --mcp` exposes six tools — `reify_context`,
`reify_why`, `reify_impact`, `reify_explain`, `reify_flow` and `reify_conflicts` — and
six is the whole surface. A server's schemas are re-sent every turn of every session,
so a tool built to save context should not charge rent to deliver it; a test asserts
they cost under 600 tokens, which six still fit inside.

**A model is optional and off** until you name a command in `.reify/llm.toml`
(`command = ["ollama", "run", "llama3"]`). Reify writes the prompt to its stdin. See
[Privacy](#privacy) for why that is a command and not an HTTP client.

<details>
<summary><strong>Shell completions, and a pre-edit risk hook</strong></summary>

```bash
reify completions zsh > ~/.zfunc/_reify     # also bash, fish
```

Inject a risk header before every edit, under 300 tokens, asserted by a test because it
runs on every edit. Non-blocking by default: a hook that blocks edits gets uninstalled,
and its warnings go with it.

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

Keep the index current with a git hook:

```bash
printf '#!/bin/sh\nreify index >/dev/null 2>&1 &\n' > .git/hooks/post-merge
chmod +x .git/hooks/post-merge && cp .git/hooks/post-merge .git/hooks/post-checkout
```

</details>

## Commands

| Command | What it does |
|---|---|
| `reify context "<task>"` | The minimum knowledge for a change, plus a reading plan. **The one that matters.** `--toon` emits the agent format |
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
| `reify upgrade [--check]` | Replace this binary with the latest release. The only networked command; refused under `REIFY_OFFLINE=1` |
| `reify uninstall --yes` \| `uninit --yes` | Remove the binary \| one repository's store and instruction block |
| `reify serve --mcp` | Model Context Protocol over stdio |
| `reify completions <shell>` | Completion script |

Everything takes `--json` against a versioned schema and `--budget <tokens>`. Full
output shapes: [docs/json-schema/](docs/json-schema/).

**Agents should ask for `--toon`.** JSON repeats every field name on every record;
TOON states each section's columns once, then one row per record — measured at **57%
fewer tokens for identical facts**, with `status` still the first column of every row.
The header carries the measured token cost of the very bytes being emitted, so the
budget claim and the payload cannot drift apart. MCP's `reify_context` answers in TOON
already.

## Privacy

**Your source code and your business documents never leave the machine.** Reify opens
no network connection — not "by default", at all. There is no HTTP client in the
dependency tree, and `cargo test` fails the build if one appears.

For a company that will not let proprietary code near a cloud service, that is the
difference between a tool they can evaluate and one they cannot.

| | |
|---|---|
| Networking crates in `Cargo.lock` | asserted zero, in CI |
| Sockets in the source | asserted zero, in CI |
| Subprocesses | `git`, reviewed document converters, and — for `reify upgrade` only — `curl` and `tar`; each named in a test |
| `git` reaching the network | forbidden: every invocation sets `GIT_NO_LAZY_FETCH=1`, so a partial clone cannot fetch |
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

**An incremental index is byte-identical to a full rebuild**, asserted by a property test that applies random edit sequences and compares canonical dumps. Each stage owns a disjoint set of edge kinds and its own invalidation trigger, which is what makes that true. Details: [docs/architecture.md](docs/architecture.md).

### Measured performance

ERPNext, 5,064 files, 8-core M-series laptop.

| | measured |
|---|--:|
| full index, no model | 4.2 s |
| reindex, nothing changed | 0.10 s |
| reindex, one file edited | 0.49 s |
| `reify context` | 57 ms |
| `reify impact` | 0.2 ms |
| `reify why` | 87 ms median, 168 ms worst — a `git log -L` subprocess; ~5 ms without |
| peak memory, full index | 224 MB |
| store size | 47 MB (33% of a 144 MB working tree) |

A full index took **78 seconds** until the full-text index was keyed by node id. `uid` is `UNINDEXED` in FTS5, so `DELETE ... WHERE uid = ?` scanned the whole table once per node — quadratic, and invisible until it was timed per stage. Editing one file took **5.9 seconds** until the repository-wide stages learned to skip when their inputs are provably unchanged.

Reindexing was **2× slower** until two things stopped being done repository-wide for
a one-line edit. Discovery read and hashed all 5,285 files on every run — 222 ms of
reading to find the handful that moved — and now `stat`s past anything whose size and
modification time are unchanged, hashing the rest across all cores. Reference
resolution reloaded and re-resolved all **144,309** references, 167 ms to resolve and
145 ms to commit, regardless of how little changed; it now re-resolves only references
whose *name* the edit added or removed, plus those inside the edited files, which is
provably the whole affected set. Measured against the previous build on the same
machine: full index 6.75 s → 4.25 s, no-op reindex 256 ms → 101 ms, one file edited
974 ms → 486 ms.

`reify why` was **1.5 seconds** on a blobless clone, and returned a *worse* answer than
it does now. `git log -L` needs the file's blob at every revision it walks, and on a
partial clone those blobs are not local — so git was silently fetching them from the
remote, one query costing 29.5 s of network and 0.07 s of work. The subprocess now runs
with `GIT_NO_LAZY_FETCH=1`: git answers from local objects or fails, and either way the
command returns in milliseconds. Eleven of twelve sampled symbols used to hit the
timeout; none do.

That fix is also why the privacy claim below is true of the whole process tree rather
than just this binary. Reify never opened a socket; the git it spawned did.

`REIFY_TIMING=1 reify index` prints the per-stage breakdown that found every one of
these.

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

**We have no developer documentation at all. Only BA documents, and they're old.**
That is the case Reify was built for. It reads DOCX, PDF, XLSX and the rest, splits them
into citable sections, and — critically — tells you where they *disagree* with the code,
so an old document becomes evidence rather than a trap. With no documents at all it falls
back to the code's own vocabulary, and still gives you `why`, `impact` and history.

**Our one expert has no time to help set this up.**
They don't need to. `reify init && reify index` needs nothing from them. If you can borrow
an afternoon, `reify concepts --suggest` turns what Reify mined into a draft glossary they
correct rather than author.

**Will this actually let us hire?**
It removes one specific bottleneck: a new developer, or an agent, being unable to find out
*why* code is the way it is without interrupting someone. That is a real part of the ramp,
not all of it. Anyone claiming a tool replaces eleven years of context is selling something.

**Is this another RAG thing?**
No vector database, no embedding model, no chunking. Retrieval is lexical and graph-based,
which is why every answer comes with a line number instead of a similarity score.

**My repo is 3,000 lines. Should I use it?**
No. Use ripgrep. Under roughly 20k LOC Reify buys you nothing a grep and a scroll wheel
don't.

**Does it send my proprietary code anywhere?**
It cannot. There is no HTTP client in the binary, and a test fails the build if one
appears. `reify upgrade` is the one command that reaches the network, through `curl` and
`tar` you can see, and `REIFY_OFFLINE=1` refuses it.

**Conflicts found nothing in my repo. Is it broken?**
Probably not. Detection requires five conditions at once and is biased hard toward silence,
because a conflict detector that cries wolf gets switched off in week two and takes its
true positives with it.

## License

[Apache-2.0](LICENSE). Patent grant included, so an agent vendor can actually ship it.
