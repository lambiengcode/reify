# ADR 0003 — SCIP/LSIF ingestion is deferred, not built

**Status:** accepted · **Date:** 2026-08-20

## Context

The original plan proposed ingesting a SCIP or LSIF index when one exists, to replace
heuristic call resolution with precise cross-references. It is listed as the
highest-leverage "do not reinvent" decision in the plan.

## Decision

Not built. Deferred until there is evidence it would help.

## Why

Three reasons, in order of weight:

1. **No evidence resolution precision is the bottleneck.** The benchmark's weakness is
   retrieval precision (0.03), which is a *ranking* problem. Nothing measured suggests
   that more precise call edges would move it. The plan's own rule is measure first.
2. **No test data.** Neither benchmark repository ships an index, so the code would
   ship untested against real input — which for a precision feature is worse than not
   shipping it.
3. **Real cost.** SCIP is protobuf, adding a `prost` dependency and a generated schema.
   LSIF is JSON-lines but requires walking result sets to recover the edges Reify wants.

The heuristic resolver already expresses its uncertainty honestly — confidence scales
with candidate count, and ambiguous names are dropped rather than guessed — so the
current behaviour is safe, merely imprecise.

## Consequences

- Cross-file resolution stays heuristic in dynamic languages.
- Revisit when a benchmark isolates resolution precision as a limiting factor, or when
  a benchmark repository ships an index to test against.
- Recorded here rather than silently omitted, because the plan named it as a MUST-adjacent
  item and a reader deserves to know it was a decision.
