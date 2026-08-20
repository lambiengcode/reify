//! Latency benchmarks for the targets in `docs/PLAN.md` §Q.
//!
//! These run against the committed `fixtures/minierp`, so they are reproducible on any
//! machine and in CI. They measure the *shape* of the cost — a regression shows up as a
//! ratio change — not the absolute numbers published in the README, which are measured
//! on a real repository and reported separately.
//!
//! Run with `cargo bench -p reify`.

use std::path::{Path, PathBuf};

use criterion::{criterion_group, criterion_main, Criterion};

use reify::context::{compile, ContextOptions};
use reify::index::{index, IndexOptions};
use reify::query;
use reify::store::Store;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate lives two levels below the workspace root")
        .join("fixtures")
        .join("minierp")
}

fn indexed() -> Store {
    let mut store = Store::in_memory().expect("store");
    index(&mut store, &IndexOptions::new(fixture())).expect("index");
    store
}

/// Q5: compiling a repository from nothing.
fn bench_full_index(c: &mut Criterion) {
    c.bench_function("index/full", |b| b.iter(indexed));
}

/// Q6: the cost of an unchanged reindex — the floor incremental indexing can reach.
///
/// The gap between this and `index/full` is the value the incremental machinery adds;
/// the gap between this and zero is the repository-wide work that still runs every
/// time, which is the open performance problem.
fn bench_reindex_unchanged(c: &mut Criterion) {
    let root = fixture();
    let mut store = indexed();
    c.bench_function("index/unchanged", |b| {
        b.iter(|| index(&mut store, &IndexOptions::new(&root)).expect("reindex"))
    });
}

/// Q1: `reify why`, without the git subprocess.
fn bench_why(c: &mut Criterion) {
    let store = indexed();
    let root = fixture();
    c.bench_function("query/why", |b| {
        b.iter(|| query::why(&store, &root, "SalesOrder.requires_approval").expect("why"))
    });
}

/// Q3: `reify impact`.
fn bench_impact(c: &mut Criterion) {
    let store = indexed();
    c.bench_function("query/impact", |b| {
        b.iter(|| query::impact(&store, "requires_approval").expect("impact"))
    });
}

/// Q2: the flagship command, across budgets.
///
/// Latency must stay flat as the budget grows: selection is a sort and a scan over an
/// already-ranked set, so a budget-dependent cost would mean the ranking is being
/// recomputed and something is wrong.
fn bench_context(c: &mut Criterion) {
    let store = indexed();
    let mut group = c.benchmark_group("query/context");
    for budget in [1_000u32, 4_000, 16_000] {
        group.bench_function(format!("budget-{budget}"), |b| {
            b.iter(|| {
                compile(
                    &store,
                    "approval for corporate orders on the strategic tier",
                    &ContextOptions {
                        budget,
                        ..Default::default()
                    },
                )
                .expect("compile")
            })
        });
    }
    group.finish();
}

/// The concept resolution path, which every multilingual query goes through.
fn bench_explain(c: &mut Criterion) {
    let store = indexed();
    c.bench_function("query/explain-vietnamese", |b| {
        b.iter(|| query::explain(&store, "khách hàng chiến lược"))
    });
}

criterion_group!(
    benches,
    bench_full_index,
    bench_reindex_unchanged,
    bench_why,
    bench_impact,
    bench_context,
    bench_explain
);
criterion_main!(benches);
