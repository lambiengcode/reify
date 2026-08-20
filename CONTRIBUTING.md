# Contributing to Reify

## The one rule that matters

**No claim ships without the check behind it.** If the README says Reify makes no
network call, a test asserts it. If a benchmark number appears anywhere, the raw
result file that produced it is committed. A pull request that adds a claim without
its evidence will be asked for the evidence, not the claim.

## Before you open a pull request

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

All three are enforced in CI. `cargo test` includes `crates/reify/tests/offline.rs`,
which fails the build if a networking crate enters the dependency tree.

## Design rules

These are not style preferences; each one is load-bearing.

1. **Deterministic first, semantic second, LLM last.** If a question can be answered
   from the AST, the graph, the index or git, it must not reach a model. New code that
   calls a model needs a written reason why the deterministic path cannot work.
2. **Every derived fact carries a status and evidence.** `Status::Unknown` is the
   `Default` on purpose — anything that forgets to state its epistemic footing lands on
   the one an agent may not act on. A non-`CONFIRMED` node without evidence is a bug.
3. **Precision above recall.** A missing rule costs an agent one search; a wrong rule
   stated confidently costs an incident. When a heuristic is uncertain, emit `UNKNOWN`
   rather than a guess.
4. **Incremental must equal full.** `index::tests::incremental_indexing_equals_a_full_rebuild`
   is the property the whole storage design exists to preserve. If your change makes
   it fail, the change is wrong, not the test.
5. **The budget governs the whole answer.** Context output plus every span the reading
   plan recommends. Budgeting only the output is a lie by omission.

## Adding a language

1. Add the `tree-sitter-<lang>` crate.
2. Map it in `extract::code::extract`.
3. Add a case to `discover::classify`.
4. Add a golden test under `fixtures/parsers/` with committed expected output.
5. Check `SymbolIndex::callable_across` — a call never crosses a language boundary
   unless the two languages genuinely share a call graph.

## Adding an extractor

Extractors are pure: text in, staged knowledge out. No store access, no shared mutable
state, no I/O. That is what makes them parallel and what will make them cacheable by
content hash. Keep them that way.

## Benchmark changes

The task set is frozen before results are seen. If you change task selection, say so
in the pull request and regenerate — do not hand-edit `benchmarks/tasks/*.json`, and
never remove a task because Reify does badly on it. The "Where Reify lost" section of
the report is a required part of the document.

## Commit messages

Conventional commits (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`). The
benchmark's task generator reads these prefixes, so they are load-bearing here too.
