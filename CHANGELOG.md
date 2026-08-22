# Changelog

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project adheres to semantic versioning once it reaches 1.0; before then, minor
versions may break the store schema, and `reify index --force` rebuilds it.

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
