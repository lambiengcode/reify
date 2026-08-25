# Project agent memory

This file is the project's committed home for project-intrinsic agent knowledge: build, test, release, architecture, and sharp-edge notes that should travel with the code.

## Checks

`cargo fmt --all`, `cargo clippy --workspace --all-targets` (CI treats warnings as
errors) and `cargo test --workspace` must all be clean. CI additionally runs the test
suite with network egress blocked — `crates/reify/tests/offline.rs` fails the build if a
networking crate enters the dependency tree.

## The standard this repository is held to

`crates/reify-bench` is the most load-bearing thing here, and its value is its
intellectual honesty rather than its numbers: steel-manned baselines, falsification
conditions written down *before* the run, Wilson intervals reported next to the
admission that they overlap, provider failures excluded rather than scored as misses,
and one rule from `metrics.rs` — *a metric that cannot be defined precisely does not get
reported*. Metric definitions live in `docs/metrics.md`. Reports are **generated**, never
hand-edited; a number that was not re-measured is corrected with a dated note rather than
silently regenerated (see the top of `benchmarks/REPORT-medusa.md`).

When a fitted parameter fails held-out validation, the fit is published and the default
reverts — `HISTORY_PRIOR_WEIGHT` in `crates/reify/src/context.rs` is the worked example.

## Decisions already measured

`reify verify` — a post-flight check reporting what an agent's patch missed — was
measured before being written and **failed** its pre-registered condition on Rust, Python
and Go. `benchmarks/REPORT-verify.md` has the numbers; `reify-bench verify-eval`
reproduces them in about two minutes with no model. Do not build it on the `CALLS` graph
alone without re-running that benchmark and beating those numbers.

## Maintaining this file

Keep this file for knowledge useful to almost every future agent session in this project.
Do not repeat what the codebase already shows; point to the authoritative file or command instead.
Prefer rewriting or pruning existing entries over appending new ones.
When updating this file, preserve this bar for all agents and keep entries concise.
