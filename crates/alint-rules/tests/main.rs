//! Integration tests for `alint-rules`, built as one test binary.
//!
//! Each module is one file in this directory. `autotests = false` in
//! `Cargo.toml` turns off Cargo's one-binary-per-file discovery, so a new
//! test file runs only once it is declared here; xtask's `test_layout` test
//! fails when one is missing. To run one file's tests:
//! `cargo test -p alint-rules --test it -- <module>::`.

mod deny_unknown_fields_uniformity;
mod did_you_mean_integration;
mod git_blame_age;
mod phase4_diagnostics_integration;
mod scope_filter_integration;
mod shell_out_rules;
