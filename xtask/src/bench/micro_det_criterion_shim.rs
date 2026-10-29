//! Counting stand-in for the subset of the `criterion` API that alint's
//! micro benches use. NOT part of the xtask module tree: `xtask micro-det`
//! writes this file out as the `lib.rs` of a throwaway crate named
//! `criterion` and patches it over crates.io's `criterion` when it builds a
//! ref's benches, so each ref's own, unmodified bench sources compile
//! against it.
//!
//! Instead of timing, it runs exactly `INV_K` iterations of the one
//! benchmark whose full id (`group/id`) equals `INV_FILTER`. Every
//! iteration goes through [`__inv_measure`], which never inlines, so
//! Callgrind can collect only inside it (`--toggle-collect=*__inv_measure*`)
//! and setup stays out of the count. `INV_LIST=1` prints every benchmark id
//! instead of running anything.
#![allow(clippy::all, clippy::pedantic, missing_docs, dead_code)]

use std::fmt::Display;
use std::time::Duration;

pub use std::hint::black_box;

/// The measured region. `#[inline(never)]` keeps a distinct symbol per
/// monomorphization for Callgrind's `--toggle-collect`.
#[inline(never)]
pub fn __inv_measure<O, R: FnMut() -> O>(routine: &mut R) -> O {
    routine()
}

#[derive(Clone)]
pub struct Criterion {
    filter: Option<String>,
    k: usize,
    list: bool,
}

impl Default for Criterion {
    fn default() -> Self {
        let list = std::env::var_os("INV_LIST").is_some();
        Criterion {
            filter: std::env::var("INV_FILTER").ok(),
            k: std::env::var("INV_K")
                .ok()
                .and_then(|k| k.parse().ok())
                .unwrap_or(1),
            list,
        }
    }
}

impl Criterion {
    pub fn benchmark_group<S: Into<String>>(&mut self, name: S) -> BenchmarkGroup<'_> {
        BenchmarkGroup {
            c: self,
            name: name.into(),
        }
    }
    pub fn bench_function<ID: IntoBenchmarkId, F: FnMut(&mut Bencher)>(
        &mut self,
        id: ID,
        mut f: F,
    ) -> &mut Self {
        let full = id.into_id();
        self.run(&full, |b| f(b));
        self
    }
    pub fn bench_with_input<ID: IntoBenchmarkId, I: ?Sized, F: FnMut(&mut Bencher, &I)>(
        &mut self,
        id: ID,
        input: &I,
        mut f: F,
    ) -> &mut Self {
        let full = id.into_id();
        self.run(&full, |b| f(b, input));
        self
    }
    pub fn sample_size(self, _: usize) -> Self {
        self
    }
    pub fn measurement_time(self, _: Duration) -> Self {
        self
    }
    pub fn warm_up_time(self, _: Duration) -> Self {
        self
    }
    pub fn noise_threshold(self, _: f64) -> Self {
        self
    }
    pub fn significance_level(self, _: f64) -> Self {
        self
    }
    pub fn confidence_level(self, _: f64) -> Self {
        self
    }
    pub fn nresamples(self, _: usize) -> Self {
        self
    }
    pub fn without_plots(self) -> Self {
        self
    }
    pub fn configure_from_args(self) -> Self {
        self
    }
    pub fn final_summary(&mut self) {}

    fn run(&self, full: &str, mut f: impl FnMut(&mut Bencher)) {
        if self.list {
            eprintln!("INV-ID\t{full}");
            return;
        }
        if self.filter.as_deref() != Some(full) {
            return;
        }
        eprintln!("INV matched {full} k={}", self.k);
        let mut b = Bencher { k: self.k };
        f(&mut b);
    }
}

pub struct BenchmarkGroup<'a> {
    c: &'a mut Criterion,
    name: String,
}

impl BenchmarkGroup<'_> {
    pub fn throughput(&mut self, _: Throughput) -> &mut Self {
        self
    }
    pub fn sample_size(&mut self, _: usize) -> &mut Self {
        self
    }
    pub fn measurement_time(&mut self, _: Duration) -> &mut Self {
        self
    }
    pub fn warm_up_time(&mut self, _: Duration) -> &mut Self {
        self
    }
    pub fn noise_threshold(&mut self, _: f64) -> &mut Self {
        self
    }
    pub fn sampling_mode(&mut self, _: SamplingMode) -> &mut Self {
        self
    }
    pub fn bench_with_input<ID: IntoBenchmarkId, I: ?Sized, F: FnMut(&mut Bencher, &I)>(
        &mut self,
        id: ID,
        input: &I,
        mut f: F,
    ) -> &mut Self {
        let full = format!("{}/{}", self.name, id.into_id());
        self.c.run(&full, |b| f(b, input));
        self
    }
    pub fn bench_function<ID: IntoBenchmarkId, F: FnMut(&mut Bencher)>(
        &mut self,
        id: ID,
        mut f: F,
    ) -> &mut Self {
        let full = format!("{}/{}", self.name, id.into_id());
        self.c.run(&full, |b| f(b));
        self
    }
    pub fn finish(self) {}
}

pub struct BenchmarkId(String);

impl BenchmarkId {
    pub fn new<S: Into<String>, P: Display>(name: S, p: P) -> Self {
        BenchmarkId(format!("{}/{}", name.into(), p))
    }
    pub fn from_parameter<P: Display>(p: P) -> Self {
        BenchmarkId(p.to_string())
    }
}

pub trait IntoBenchmarkId {
    fn into_id(self) -> String;
}
impl IntoBenchmarkId for BenchmarkId {
    fn into_id(self) -> String {
        self.0
    }
}
impl IntoBenchmarkId for &str {
    fn into_id(self) -> String {
        self.to_string()
    }
}
impl IntoBenchmarkId for String {
    fn into_id(self) -> String {
        self
    }
}

pub enum Throughput {
    Bytes(u64),
    BytesDecimal(u64),
    Elements(u64),
    Bits(u64),
}

pub enum BatchSize {
    SmallInput,
    LargeInput,
    PerIteration,
    NumBatches(u64),
    NumIterations(u64),
}

pub enum SamplingMode {
    Auto,
    Linear,
    Flat,
}

pub struct Bencher {
    k: usize,
}

impl Bencher {
    pub fn iter<O, R: FnMut() -> O>(&mut self, mut routine: R) {
        for _ in 0..self.k {
            black_box(__inv_measure(&mut routine));
        }
    }
    pub fn iter_with_setup<I, O, S: FnMut() -> I, R: FnMut(I) -> O>(
        &mut self,
        mut setup: S,
        mut routine: R,
    ) {
        for _ in 0..self.k {
            let input = setup();
            let mut once = Some(input);
            let mut call = || routine(once.take().expect("input consumed once"));
            black_box(__inv_measure(&mut call));
        }
    }
    pub fn iter_batched<I, O, S: FnMut() -> I, R: FnMut(I) -> O>(
        &mut self,
        setup: S,
        routine: R,
        _: BatchSize,
    ) {
        self.iter_with_setup(setup, routine)
    }
    pub fn iter_batched_ref<I, O, S: FnMut() -> I, R: FnMut(&mut I) -> O>(
        &mut self,
        mut setup: S,
        mut routine: R,
        _: BatchSize,
    ) {
        for _ in 0..self.k {
            let mut input = setup();
            let mut call = || routine(&mut input);
            black_box(__inv_measure(&mut call));
            drop(black_box(input));
        }
    }
}

#[macro_export]
macro_rules! criterion_group {
    (name = $name:ident; config = $config:expr; targets = $($target:path),+ $(,)?) => {
        pub fn $name() {
            let mut c: $crate::Criterion = $config;
            $( $target(&mut c); )+
        }
    };
    ($name:ident, $($target:path),+ $(,)?) => {
        pub fn $name() {
            let mut c = $crate::Criterion::default();
            $( $target(&mut c); )+
        }
    };
}

#[macro_export]
macro_rules! criterion_main {
    ($($group:path),+ $(,)?) => {
        fn main() {
            $( $group(); )+
        }
    };
}
