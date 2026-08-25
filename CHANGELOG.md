# Changelog

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project adheres to semantic versioning once it reaches 1.0; before then, minor
versions may break the store schema, and `reify index --force` rebuilds it.

## [0.2.3] - 2026-08-25

### Fixed
- **The index lock did nothing on Windows.** `process_is_alive` was implemented for
  unix and stubbed to `false` everywhere else. For the *current* process that means
  every lock looks stale, so the lock reclaims itself and stops excluding anything:
  two `reify index` runs could proceed against the same store. Windows users of 0.2.2
  have this; there is no configuration that avoids it, and upgrading is the fix.
  Implemented with `OpenProcess` + `WaitForSingleObject`, declared by hand for one
  question asked once, the same way `kill` already was. The remaining
  `not(any(unix, windows))` arm now returns `true`: without a liveness check the lock
  cannot be trusted, and refusing to reclaim a lock costs one user less than letting
  two indexers share a store costs everyone.
- **One file could take the whole answer.** Relevance spreads along edges, so every
  member of a file that matched the task loosely arrived holding a plausible score and
  nothing bounded how many of them were admitted. Measured on Medusa, a task about
  applying a discount twice filled 13 of 20 slots with one HTTP router and one
  arithmetic helper, and the promotion service that actually had to change ranked
  eighteenth. No file may now claim more than four symbol slots.
- **The reading plan collapsed onto a single file.** `next_reads` drained each ranked
  file in turn, so the top file consumed every slot: two of three sampled Medusa tasks
  produced six entries naming one file. Spans are now drawn a file at a time in rounds,
  so every ranked file earns one before any earns a second.
- Tests, fixtures and mocks outranked the code they exercise. A test names the domain
  vocabulary as densely as its implementation and a reader cannot edit it to change
  behaviour; on Medusa a promotion spec *and* its fixture both outranked the promotion
  service. They now give up half their score — a penalty, not an exclusion, because
  "fix the failing test for X" is a real task. Tunable as `test_path_penalty`.
- `install.sh` did not verify the checksum it advertised. The `curl | sh` path now
  checks the published SHA-256 before unpacking, which is what `reify upgrade` already
  did; a mismatch installs nothing.
- The README shipped its quickstart, language switcher and table of contents twice.
- The Roadmap and Status sections are gone from all three READMEs. Both were prose that
  needed hand-editing every release to stay true, and the Status section had already
  drifted — it still quoted `reify why` at 205 ms three versions after the measurement
  changed. What they said that is durable lives in the benchmark reports and this file.
- `Cargo.toml` pointed `documentation` at `docs.rs/reify`, which belongs to an
  unrelated crate published in 2021.

### Added
- **Windows and macOS are tested in CI.** Releases shipped binaries for both and no job
  had ever executed either, so a portability break would have reached users before it
  reached us. A `platform` job now runs the full suite and an end-to-end CLI smoke on
  `windows-latest` and `macos-latest`; `check` stays on Linux and keeps what does not
  vary by platform — formatting, clippy, and the network-egress gate that needs
  iptables. It found the lock bug above in its first run, by failing to recognise its
  own process as alive. Two details are handled rather than discovered later: Windows
  checkouts convert LF to CRLF, so `core.autocrlf false` is set *before* checkout; and
  the smoke step runs under bash, because PowerShell propagates only the last command's
  exit code and a failing `init` would otherwise leave the step green.
- **Windows x86_64 binaries.** `reify upgrade` works there too: Windows keeps a handle
  on the running image, so the old binary is renamed aside rather than overwritten, and
  restored if the replacement fails. `install.sh` recognises Git Bash, MSYS2 and Cygwin.
- Three more MCP tools — `reify_explain`, `reify_flow`, `reify_conflicts` — because
  those are the capabilities no other retriever offers and MCP is how most clients
  reach Reify at all. All six schemas still cost under the 600-token ceiling the
  original three were held to, which is still asserted by a test. `reify_preflight` was
  considered and left out: it answers the same question as `reify_why` for an agent.

### Changed
- Retrieval measured before and after on all three brownfield repositories, same
  machine, same task sets. **Hit rate improved in every repository and both
  conditions**; grep baselines are byte-identical, which is the control.

  | three rounds | before | after |
  |---|--:|--:|
  | medusa (TypeScript, n=40) | 27.5% | **42.5%** |
  | openmrs (Java, n=22) | 54.5% | **59.1%** |
  | ofbiz (Java, n=40) | 77.5% | **85.0%** |

  Stated plainly because it is a trade: mean reciprocal rank rose on medusa
  (0.097 → 0.136) and fell on openmrs (0.278 → 0.208) and ofbiz (0.461 → 0.392).
  Spreading the top slots across more files means that when the right file was already
  ranked first it now shares the window. That is the intended direction — a file never
  offered cannot be used, while a file at rank six is still in the context — and it is
  the failure the end-to-end SWE-bench run diagnosed: retrieval was already ahead and
  the model still did no better, because of what the window contained and in what
  order.
- **SWE-bench Verified re-run in full, all 500 instances**, and every published
  retrieval figure moved up. Both grep baselines came back byte-identical to the
  previous run, which is the control.

  | retrieval, n=500 | before | after |
  |---|--:|--:|
  | reify, one round | 66.0% | **72.6%** |
  | reify, three rounds | 84.6% | **87.0%** |
  | one round, offered *every* touched file | 59.0% | **65.4%** |
  | three rounds, offered *every* touched file | 77.0% | **81.4%** |

  Paired against content-grep, one round now wins 342 and loses 12 (was 310 / 13);
  three rounds win 406 and lose 4 (was 395 / 5), exact McNemar p ≈ 9 × 10⁻¹¹⁵. Six of
  twelve repositories improved and none regressed — the largest are pytest 84% → 95%,
  matplotlib 91% → 97% and sphinx 75% → 80%. MRR moved the other way, 0.45 → 0.43 at
  three rounds, for the reason given above. Median tokens rose from 3,466 to 3,670 at
  one round, still below content-grep's 3,998.

- **Stage 2, the end-to-end result, re-run and now ahead** — 101 instances graded under
  both arms by the official SWE-bench harness, up from 63.

  | resolved | | |
  |---|--:|---|
  | BM25 | 67.3% | 68 resolved, 1 empty patch |
  | **Reify** | **73.3%** | 74 resolved, 2 empty patches |
  | | | paired 12–6, exact McNemar **p = 0.24** |

  Reify resolves six more issues and wins twice as many disagreements as it loses, but
  at this sample size that is **not statistically significant**: the honest reading is
  *ahead and not yet proven*, and the README says so before it says the percentages.
  The previous run was a tie, and the one before that a loss.

  These absolute rates are **not** comparable to the 23.8% published earlier. That run
  used DeepSeek, whose account ran out of balance mid-project; this one uses Claude
  Sonnet, and a stronger model lifts both arms. What survives a model change is the
  paired comparison, because both arms always answer the same instance with the same
  model. Per-instance outcomes are committed in
  `benchmarks/swe/results/stage2-endtoend.json`.

### Fixed (reproduction)
- The committed stage-2 driver was **not the script that produced the published
  numbers**. `benchmarks/swe/stage2c.py` fed the model whole files; the run behind the
  figures used `reify context --for-edit` regions, which is the entire reason the
  earlier loss became a tie. Anyone following the repository's own instructions would
  have failed to reproduce its own results. Replaced by `benchmarks/swe/stage2.py`,
  which is the driver that produced the numbers above, with the model as a parameter.
- An epistemic status shared by every row of a section is stated once on the section
  heading instead of repeated down the page. Symbols are `CONFIRMED` by construction,
  so `[confirmed]` appeared on all twenty code rows of every answer, where it carried
  no information and invited the wrong reading — it attests that a symbol was parsed
  from source, never that it is the right place to change. A section with mixed
  statuses still badges every row, and machine-readable output is unchanged.

## [0.2.2] - 2026-08-22

### Fixed
- `reify why` no longer lets git reach the network. `git log -L` needs the file's blob
  at every revision it walks; on a blobless clone those blobs are not local, so git was
  silently fetching them from the promisor remote — one query measured at 29.5 s of
  network against 0.07 s of actual work. Query-time git now runs with
  `GIT_NO_LAZY_FETCH=1`, so it answers from local objects or fails fast. Reify itself
  never opened a socket, but a subprocess that lazily fetches does, and "your code never
  leaves the machine" has to hold for the whole process tree.

### Changed
- **The store schema is v7.** Files now carry a modification time, so `reify index
  --force` is needed once to rebuild an older store; the store says so when opened.
- Indexing is roughly twice as fast, measured against the previous build on the same
  machine: full index **6.75 s → 4.25 s**, reindex with nothing changed **256 ms →
  101 ms**, reindex after one edit **974 ms → 486 ms**.
- Discovery no longer reads the whole tree to find what changed. It `stat`s past any
  file whose size and modification time match what the last index recorded — the same
  bet git makes in its own index, and safe here because it only decides whether to
  *read* a file, never whether it changed; the hash still decides that, and `--force`
  ignores stamps entirely. The files that must be read are hashed across all cores.
- Reference resolution is incremental. It reloaded and re-resolved all 144,309
  references on ERPNext for a one-line edit — 167 ms to resolve, 145 ms to commit — and
  now re-resolves only references whose name the edit added or removed, plus those
  inside the edited files. That is the whole affected set: a reference can only resolve
  differently if the symbols matching its name changed, and symbols only change in
  re-parsed files. `commit` is an upsert and never deletes edges it was not given, so
  the rest keep the edges they have. Stage cost on that edit: **356 ms → 23 ms**.
- `reify why` is **1517 ms → 87 ms median** on a 60,000-commit blobless clone, with the
  worst of twelve sampled symbols at 168 ms. Eleven of those twelve previously hit the
  1.5 s timeout and fell back to file-level history; none do now, so the answer is both
  faster and more precise.
- `is_repository` reads the filesystem instead of spawning `git rev-parse`, which cost a
  whole process — 18 ms of a 205 ms command — to answer a question `.git` already
  answers. Worktree and submodule `.git` files, and subdirectories, are all handled.
- `line_history` results are memoised for the life of the process, keyed by the commit
  `HEAD` points at. The first walk is inherently expensive — git must go back through
  history until it finds enough commits touching those exact lines, and a line stable
  for years means walking years — so the thing worth avoiding is walking it twice, which
  `reify serve` did every time an agent revisited a symbol.

## [0.2.1] - 2026-08-22

### Changed
- Brand: a hand-drawn mascot replaces the geometric mark. `assets/mascot.png` is the
  only hand-made file; the light and dark lockups, the dark-background variant and the
  16–512px icon ladder are all derived from it by `assets/make-logo.py`, so the set
  cannot drift the way four separately-edited SVGs did. Adds a favicon, which the
  repository did not have. The link card leads with the mascot.
- Link card: rebuilt around SWE-bench Verified — a file the fix touched is offered
  84.6% of the time against grep's 6.6%, over 500 real issues, winning on all 12
  repositories — and now animated, the bars growing and the numbers counting up. Frame
  one is the finished card, because most link unfurlers render only that frame. It reads
  its numbers from the committed results rather than having them typed in.
- Demo: re-recorded **in colour**, on Dracula, in zsh, with no window chrome. Recorded
  with vhs, which runs each command in a real terminal — reify colours only when one is
  attached, so a recorder that captures through a pipe gets flat text whatever the tape
  says. All three status tags are now coloured as a user sees them, the prompt is green,
  and the command word highlights green as it is typed, by a dozen lines of `zle` rather
  than a plugin the recorder would have to install. 3.7 MB → 0.63 MB.

### Fixed
- The demo's window dots quantised to grey: the documented `max_colors=32` palette spent
  itself on antialiasing greys, the frame being almost entirely monochrome text. Now 64.
- The dark logo variant lost the mascot's pupils — repainting the black outline for dark
  backgrounds repainted them too, into the cream of the eyes they sit in. The repaint now
  spares the eyes.

## [0.2.0] - 2026-08-22

### Added
- **SWE-bench Verified evaluation** ([`benchmarks/swe/`](benchmarks/swe/)). Retrieval over
  all 500 instances: reify offers a file the fix touched 84.6% of the time against grep's
  6.6%, and a single round wins on 310 instances while spending fewer tokens. End-to-end
  patch resolution over 101 instances went the other way — a BM25 baseline resolved 18.1%
  against reify's 11.1% — and both results are in the README with equal prominence.
- Retrieval: a bounded content scan as a seed source, commit *bodies* in the history
  prior behind a temporal leakage wall, and a second file fan-out pass after graph
  spread, so files reached through history and co-change lift their symbols.
- `reify-bench`: `coverage` (cross-file dependent coverage per language), `--weights`
  ablations, `--arms` selection, task windows (`--until`, `--exclude`), a miss taxonomy
  in `audit`, and a coordinate-descent `fit`.
- `reify upgrade [--check] [version]` — replace the binary with a release, fetched by
  `curl` and unpacked by `tar` as visible subprocesses (never an embedded HTTP
  client), checksum verified in-process before install; refused under
  `REIFY_OFFLINE=1`.
- `reify uninstall --yes` — remove the binary and nothing else.
- `reify uninit --yes` — remove one repository's `.reify/` store and the instruction
  block `init --write-agent-instructions` appended. Both new removal commands print
  their plan and change nothing without `--yes`.

### Changed
- The README demo is a feature tour recorded with
  [termgif](https://github.com/aayushadhikari7/termgif) from a committed `assets/demo.tg`,
  replacing the narrated vhs/terminalizer recording.
- Store schema is unchanged; `reify index` needs no rebuild coming from 0.1.0.

## [Unreleased]

### Added
- `reify context --for-edit` — regions sized to be *edited* rather than spans sized to be
  *read*: each hit padded to its whole enclosing definition, the file's imports included
  once, overlapping regions merged, budget still hard. Measured on SWE-bench Verified,
  this raises the share of prompts that actually contain a file the fix touched from 26.7%
  to 56.7%, against BM25's 40.0%, and turns an end-to-end loss (11.1% vs 18.1%) into a tie
  (23.8% vs 23.8%).

### Changed
- Benchmark charts redrawn: gradient bars, dashed grid, staggered condition labels, and
  colours that adapt to light and dark themes with a mid-tone fallback for renderers that
  ignore `prefers-color-scheme`.
- READMEs refactored — one badge style, fewer badges, a grouped contents list, install and
  agent wiring consolidated into one section, and the vestigial scorecard removed.

## [0.1.0] - 2026-08-21

First vertical slice: a repository can be compiled, queried and benchmarked end to end.

### Added
- Knowledge model with epistemic status, confidence and evidence on every derived fact.
- SQLite store with FTS5 retrieval, stage-scoped invalidation and a canonical dump for
  equivalence testing.
- tree-sitter extraction for Python, TypeScript, JavaScript and Java.
- SQL table access from `.sql` files and from queries embedded in source, attributed to
  the enclosing symbol.
- Structured model metadata ingestion — entity definitions with machine names and human
  labels, the declared business vocabulary of metadata-driven systems.
- Markdown, text, HTML, DOCX and PDF document ingestion, split into citable sections.
- Git archaeology: introduce and change lineage, co-change, commit classification, and
  precise line-range history computed lazily at query time under a wall-clock bound.
- Multilingual concept layer with three bridges: declared glossary, translation files,
  and co-occurrence between document headings and code identifiers.
- Deterministic business-rule mining and conservative conflict detection.
- Context compilation with a budget governing the whole answer, including the reading
  plan it recommends.
- CLI: `init`, `index`, `status`, `context`, `why`, `impact`, `explain`, `flow`,
  `conflicts`, `rules`, `concepts`, `report`, `preflight`, `serve --mcp`.
- Optional model assistance through a user-configured external command, so no HTTP
  client enters the dependency tree and every byte sent is auditable.
- The Reify Brownfield Benchmark: retrieval and single-shot model conditions, with
  frozen tasks, committed raw outcomes and a generated report.

### Known limitations
- Incremental indexing rebuilds three repository-wide stages on every run.
- Conflict detection finds nothing on repositories that ship no specification prose.
- The translation bridge is covered by tests but not by the ERPNext benchmark.
