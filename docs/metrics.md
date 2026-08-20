# Metric definitions

Every number Reify prints is defined here. A metric that cannot be defined precisely
does not get printed — a scorecard full of impressive but undefined figures is a
liability, not marketing.

## `reify report`

| Metric | Definition |
|---|---|
| **Files** | Files indexed. Excludes anything `reify init` listed as skipped. |
| **Symbols** | Functions, methods, classes, interfaces, types and enums extracted by tree-sitter. Excludes variables and imports. |
| **Document sections** | Leaf sections of Markdown and text documents, split at every heading. A document with no headings counts as one section. |
| **Database objects** | Distinct tables named in SQL, plus entities declared by structured model metadata. Counted once regardless of how many places reference them. |
| **Concepts** | Distinct business concepts. Broken down by the bridge that produced each: `declared` (glossary or structured metadata), `translation` (mined from localisation files), `co-occurrence` (document headings that also name code). |
| **Business rules** | Rule candidates surviving the length and classification filters. Excludes conflicts. Every one carries at least one evidence citation. |
| **Contradictions** | Conflicts passing all five conditions in `docs/PLAN.md` §G.6. Deliberately biased toward silence: zero does not prove agreement. |
| **Commits linked** | Commits attached to at least one indexed file as `INTRODUCED_BY` or `CHANGED_BY`. Bounded by `--max-commits`. |
| **Relationships** | Total edges. |
| **Documented symbols** | Symbols with a docstring or leading comment, over all symbols. |
| **Knowledge coverage** | Symbols reachable from at least one concept or document section, over all symbols. Measures how much of the code the knowledge layer can say anything about. |

## Benchmark

| Metric | Definition |
|---|---|
| **Hit rate** | Tasks where the condition named at least one file the reference commit changed, over all tasks. |
| **Recall** | Changed files named, over all changed files, averaged across tasks. |
| **Precision** | Changed files named, over all files named, averaged across tasks. |
| **MRR** | Mean of `1 / rank` of the first changed file. Zero for a task where none was found. |
| **Tokens to first correct file** | Tokens to read the answer plus every file or span offered up to and including the first changed one. Reported as a median over tasks that found something — see the correction below. |
| **Expected tokens** | Mean tokens to reach a changed file, charging a miss the full budget. The comparable single number: a condition cannot improve it by failing more often. |
| **Head to head** | Median tokens over only the tasks *both* conditions solved. Removes the difficulty bias in the per-condition median. |

## Token estimation

`reify` estimates tokens with `heuristic-v1`: Latin script at four bytes per token, CJK
at 1.5 characters per token. The estimator's name appears in every JSON answer so a
number can be traced to how it was counted. Benchmarks comparing against a real model
must use the provider's own `usage` counts, never this estimate.
