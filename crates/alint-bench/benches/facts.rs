//! Repository-fact evaluation over a large in-memory index.
//!
//! The 20-fact mix mirrors the versioning + toolchain-pins additions to S4:
//! eight exact-path existence facts and twelve exact-path content facts. Every
//! path is absent, so an accidental fallback to `index.files().any(...)` scales
//! as O(facts × files) instead of one lazy O(files) index build plus O(facts)
//! lookups.

use std::path::Path;
use std::time::Duration;

use alint_bench::fact_fixture::{absent_literal_facts, build_entries};
use alint_core::FileIndex;
use alint_core::facts::evaluate_facts;
use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

fn bench_absent_literal_facts(c: &mut Criterion) {
    let facts = absent_literal_facts();
    let root = Path::new("/bench/root");
    let mut group = c.benchmark_group("literal_facts");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(10));

    for files in [1_000usize, 10_000, 100_000] {
        let entries = build_entries(files);
        // The lazy exact-path map visits each file once; the fact count is a
        // fixed workload dimension, not `files × facts` processed elements.
        group.throughput(Throughput::Elements(
            u64::try_from(files).expect("benchmark size fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::new("20_absent", files),
            &entries,
            |bencher, entries| {
                bencher.iter_batched(
                    || FileIndex::from_entries(entries.clone()),
                    |index| {
                        std::hint::black_box(evaluate_facts(&facts, root, &index).unwrap());
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_absent_literal_facts);
criterion_main!(benches);
