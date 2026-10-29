//! Integration tests for `xtask`, built as one test binary.
//!
//! Each module is one file in this directory. `autotests = false` in
//! `Cargo.toml` turns off Cargo's one-binary-per-file discovery, so a new
//! test file runs only once it is declared here; xtask's `test_layout` test
//! fails when one is missing. `cargo test -p xtask --test it -- <module>::`
//! runs one file's tests; the filter matches substrings, so it also runs any
//! module whose name ends in `<module>` (nextest can anchor it:
//! `-E 'test(/^<module>::/)'`).

mod proptest_seeds;
mod publish_order;
mod test_layout;
mod testkit_package;
