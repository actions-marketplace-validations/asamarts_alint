//! CLI snapshot tests. Each `*.toml` under `tests/cli/` describes
//! one invocation of the `alint` binary, paired with sandbox input
//! (`*.in/`), expected post-run tree (`*.out/`), and expected
//! stdout / stderr / exit status. Run via `cargo test -p alint
//! --test it -- --exact cli::cli_tests`; regenerate expected output with
//! `TRYCMD=overwrite cargo test -p alint --test it -- --exact cli::cli_tests`.

#[test]
fn cli_tests() {
    // Snapshot the normal Unicode presentation independently of the developer's
    // terminal. ASCII fallback itself is covered by GlyphSet unit tests.
    trycmd::TestCases::new()
        .env("TERM", "xterm-256color")
        .case("tests/cli/*.toml");
}
