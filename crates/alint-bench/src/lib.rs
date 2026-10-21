//! Benchmark helpers for alint.
//!
//! - [`tree`] — deterministic synthetic repository-tree generator keyed on
//!   a seed. Given the same `(files, depth, seed)` triple, produces
//!   byte-identical output across platforms and runs.
//!
//! Criterion bench targets live in `benches/`. The `xtask bench-release`
//! driver reuses [`tree::generate_tree`] to materialise scaling-curve trees
//! before shelling out to `hyperfine`.

#[doc(hidden)]
pub mod fact_fixture;
pub mod tree;
