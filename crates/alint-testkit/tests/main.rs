//! Integration tests for `alint-testkit`, built as one test binary.
//!
//! Each module is one file in this directory. `autotests = false` in
//! `Cargo.toml` turns off Cargo's one-binary-per-file discovery, so a new
//! test file runs only once it is declared here; xtask's `test_layout` test
//! fails when one is missing. To run one file's tests:
//! `cargo test -p alint-testkit --test it -- <module>::`.

mod policy_runner;
