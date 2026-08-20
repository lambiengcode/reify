# ADR 0001 — SQLite over a graph database

**Status:** accepted · **Date:** 2026-08-20

## Context

Reify stores a typed graph: roughly 10^5 nodes and 10^6 edges for a 5,000-file
repository. The obvious choice is a graph database.

## Decision

A single SQLite file: typed node and edge tables with covering indexes, plus FTS5.

## Why

At this scale a covering index on `edges(src, kind)` outperforms a graph engine's
traversal machinery, and traversal is not the bottleneck: `query/impact` runs in ~200µs.
SQLite additionally gives transactions, full-text search and a single copyable file, all
of which Reify needs. A graph database would add an operational dependency and a second
query language to buy nothing measurable.

## Consequences

- The store is `rsync`-able and inspectable with any SQLite tool.
- Deep traversal would degrade before a graph engine would. Revisit if a benchmark
  shows traversal dominating — and not before.
- No vector storage. If embeddings are ever needed, f32 blobs with a brute-force SIMD
  scan cover this scale: 50k concepts × 384 dims is 76MB and single-digit milliseconds.
