//! Property-based invariants for alint.
//!
//! Each invariant is a `proptest!` that feeds generated policy tests
//! into the testkit runner and asserts one global property:
//!
//! 1. `check_never_panics` — check tolerates arbitrary tree/config
//!    pairs from the broad strategy without panicking. Runner
//!    errors (invalid globs, rule-config errors, etc.) are
//!    permitted; panics are not.
//! 2. `fix_dry_run_is_pure` — under the fixable strategy with
//!    `Step::FixDryRun`, the resulting on-disk state equals the
//!    input tree byte-for-byte.
//! 3. `fix_is_idempotent` — running `fix` twice never performs
//!    applied operations on the second pass. Single-rule (Phase-0
//!    idempotence is a per-fixer guarantee) over the full fixer
//!    catalogue.
//! 4. `fix_converges_when_fully_resolved` — when a single fix pass
//!    reports zero skipped and zero unfixable, a subsequent `check`
//!    reports NO violation (of any level). Single-rule, all fixers.
//! 5. `fix_dry_run_is_pure_single_rule` — dry-run purity over the full
//!    fixer catalogue (the multi-rule `fix_dry_run_is_pure` covers
//!    only 4 fixers).
//! 6. `check_fixable_never_overlaps_a_suggestion` — `check` never tags a
//!    violation auto-fixable when a bare `fix` merely suggests it (the
//!    honesty net for the Unsafe-tier `file_remove` reporting).
//! 7. `fix_unsafe_converges_when_fully_resolved` — a fully-applied
//!    `fix --unsafe-fixes` converges, covering the Unsafe apply path
//!    (e.g. `file_remove`) that the Safe convergence law skips.
//!
//! The single-rule invariants use one fixable rule per policy test on
//! purpose: they isolate each fixer's OWN fixed-point behaviour (a fixer must
//! be its own fixed point in a single pass). As of Phase 1 `alint fix` is a
//! fixpoint (re-walk and re-fix to convergence, docs/design/v0.17/fixpoint.md),
//! so the CROSS-rule cascades these once excluded -- one rule creating a file
//! another must then fix, one rule renaming a file out from under another rule's
//! violation, a content edit and a whole-file op on one file -- now DO converge
//! in a single `fix`. Those are covered by the dedicated e2e policy tests
//! (`fix/interactions/create_then_content_fix_cascades`,
//! `content_edit_and_rename_same_file_no_corruption`,
//! `append_applies_once_when_not_self_satisfying`), which assert the exact
//! multi-pass outcome; the single-rule properties here stay single-rule so a
//! per-fixer regression is not masked by a cascade.
//!
//! The generated expectations intentionally permit execution while invariant
//! tests inspect [`alint_testkit::StepObservation`] directly. The public runner
//! still validates every generated document before executing it.
//!
//! Tuning knobs: `PROPTEST_CASES` env var scales case count.

use std::path::Path;

use alint_testkit::strategies::{
    any_policy_test, fixable_policy_test, single_fixable_policy_test, with_steps,
};
use alint_testkit::treespec::{VerifyMode, verify};
use alint_testkit::{PolicyTest, PolicyTestRun, Runner, RunnerOptions, StepObservation, TestStep};
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;

fn run_policy_test(test: &PolicyTest) -> alint_testkit::Result<PolicyTestRun> {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/invariants.rs");
    Runner::new(RunnerOptions::default()).run(test, &source)
}

fn inline_config(test: &PolicyTest) -> &str {
    test.given
        .config
        .as_deref()
        .expect("strategy uses inline config")
}

/// Keep the per-invariant case count small; each case spins up a tempdir and
/// runs alint end-to-end, which is ~5–10 ms.
///
/// Failure seeds persist beside this file, in
/// `invariants.proptest-regressions`. proptest's default location searches
/// upward for a `lib.rs` or `main.rs` and, now that `tests/main.rs` exists,
/// would use `crates/alint-e2e/proptest-regressions/` and skip the committed
/// seeds, so the location is pinned.
fn invariant_config() -> ProptestConfig {
    ProptestConfig {
        cases: 48,
        max_shrink_iters: 64,
        failure_persistence: Some(Box::new(FileFailurePersistence::WithSource(
            "proptest-regressions",
        ))),
        ..ProptestConfig::default()
    }
}

#[test]
fn committed_regression_seeds_are_replayed() {
    let persistence = invariant_config()
        .failure_persistence
        .expect("failure persistence is configured");
    assert!(
        !persistence
            .load_persisted_failures2(Some(file!()))
            .is_empty(),
        "no seeds load from tests/invariants.proptest-regressions"
    );
}

proptest! {
    #![proptest_config(invariant_config())]

    #[test]
    fn check_never_panics(base in any_policy_test()) {
        let scenario = with_steps(base, vec![TestStep::Check]);
        // The assertion is panic-freedom; valid config failures are observations.
        let _ = run_policy_test(&scenario);
    }

    #[test]
    fn fix_dry_run_is_pure(base in fixable_policy_test()) {
        let scenario = with_steps(base, vec![TestStep::FixDryRun]);
        let Ok(run) = run_policy_test(&scenario) else { return Ok(()); };
        let Some(expected_tree) = scenario.given.tree.as_ref() else { return Ok(()); };
        let Ok(report) = verify(expected_tree, &run.root, VerifyMode::Strict) else {
            return Ok(());
        };
        prop_assert!(
            report.is_match(),
            "dry-run mutated disk state:\n{report}",
        );
    }

    #[test]
    fn fix_is_idempotent(base in single_fixable_policy_test()) {
        // Per-fixer idempotence: with one rule, a second `fix` applies nothing
        // (each fixer is a genuine fixed point). This is the property-level guard
        // for the round-3 non-convergence bugs (F1 doubled-CR, F2 interior-CR, F3
        // stacked-BOM), each a fixer whose second pass still applied.
        //
        // Deliberately SINGLE-rule. Since Phase 1 `alint fix` is a byte-level
        // fixpoint, a single `fix` already runs a MULTI-rule cascade to convergence
        // internally (rule A's `file_create` makes a file that rule B then fixes on
        // the next internal pass), so those cascades are asserted by the dedicated
        // `fix/interactions` policy tests instead. Kept single-rule here so a
        // per-fixer non-convergence regression cannot hide behind a cascade. This
        // `[fix, fix]` shape holds because every shipped fixer is normalizing or
        // self-guarding (a second invocation finds nothing to do): the fixpoint
        // keeps no cross-invocation state, so idempotence rests entirely on the
        // fixers' own idempotence, which this asserts.
        let scenario = with_steps(base, vec![TestStep::Fix, TestStep::Fix]);
        let Ok(run) = run_policy_test(&scenario) else { return Ok(()); };
        let Some(second) = run.observations.get(1) else {
            return Ok(());
        };
        prop_assert!(
            second.applied.is_empty(),
            "second fix pass applied {:?} for a single fixer; expected idempotence; config:\n{}",
            second.applied,
            inline_config(&scenario),
        );
    }

    #[test]
    fn fix_converges_when_fully_resolved(base in single_fixable_policy_test()) {
        // Convergence law: after a single, fully-applied `fix` (nothing skipped,
        // nothing unfixable), a subsequent `check` finds NOTHING. Driven by the
        // SINGLE-rule strategy so every fixer is exercised with no
        // cross-rule single-pass ordering interference, and asserted against
        // residuals of ANY level. (The old form used the multi-rule strategy --
        // whose fixers were all `level: warning` -- and counted only ERROR-level
        // residuals, so it could never fail: vacuous. Round-3 audit fix.)
        let scenario = with_steps(base, vec![TestStep::Fix, TestStep::Check]);
        let Ok(run) = run_policy_test(&scenario) else { return Ok(()); };
        let Some((fix_report, check_report)) = extract_fix_then_check(&run) else {
            return Ok(());
        };
        // A single fixable rule must reach a fixed point. The byte-level fixpoint
        // fails to converge only on a genuinely oscillating config (the tree keeps
        // changing every pass) -- which no single normalizing / self-guarding
        // fixer is. This directly gates "no fixable rule is non-convergent": a
        // future fixer that changes bytes every pass without settling would cap
        // out and trip this. (With apply-once removed, this is the direct
        // invariant, not the round-3 fingerprint-stability coupling it replaced.)
        prop_assert!(
            !fix_report.non_convergent,
            "a single-rule fix hit the non-convergence cap; config:\n{}",
            inline_config(&scenario),
        );
        // Only assert convergence when the fix resolved every violation it
        // encountered. A fixer that skipped (binary file, size limit, ...) leaves
        // a real violation on disk; that is not a convergence failure. Likewise a
        // fix surfaced as `suggested` was deliberately withheld at this threshold
        // (an Unsafe fixer such as `file_remove` under a bare `fix`), so its
        // violation correctly survives -- also not a convergence failure.
        if !fix_report.skipped.is_empty()
            || !fix_report.unfixable.is_empty()
            || !fix_report.suggested.is_empty()
        {
            return Ok(());
        }
        let residual = check_report.violations.len();
        prop_assert_eq!(
            residual,
            0,
            "check still reported {} violation(s) after a fully-applied single-rule fix \
             (non-convergent fixer); config:\n{}",
            residual,
            inline_config(&scenario),
        );
    }

    #[test]
    fn fix_dry_run_is_pure_single_rule(base in single_fixable_policy_test()) {
        // Dry-run purity over the full fixer catalogue: even the content
        // fixers (compose buffer) and the direct-write trio (create/remove/
        // rename, via the stage sink) must leave the tree byte-identical under
        // `fix --dry-run`.
        let scenario = with_steps(base, vec![TestStep::FixDryRun]);
        let Ok(run) = run_policy_test(&scenario) else { return Ok(()); };
        let Some(expected_tree) = scenario.given.tree.as_ref() else { return Ok(()); };
        let Ok(report) = verify(expected_tree, &run.root, VerifyMode::Strict) else {
            return Ok(());
        };
        prop_assert!(
            report.is_match(),
            "dry-run mutated disk state:\n{report}",
        );
    }

    #[test]
    fn check_fixable_never_overlaps_a_suggestion(base in single_fixable_policy_test()) {
        // Honesty law (round-7): `check` tags a violation `is_fixable` only when a
        // bare `alint fix` would RESOLVE it. A single-rule tree shares one fixer
        // tier, so if that bare fix produced ANY `Suggested` (a below-Safe-threshold
        // fixer such as the now-Unsafe `file_remove`), then NONE of its violations
        // may be tagged fixable by `check` -- otherwise `check` promises a
        // resolution the bare `fix` withholds (it needs `--unsafe-fixes`). This is
        // the net that would have caught the pre-fix regression where an Unsafe
        // `file_remove` was still counted "auto-fixable" in `check`.
        let scenario = with_steps(base, vec![TestStep::Check, TestStep::Fix]);
        let Ok(run) = run_policy_test(&scenario) else { return Ok(()); };
        let (Some(check), Some(fix)) =
            (run.observations.first(), run.observations.get(1))
        else {
            return Ok(());
        };
        if fix.suggested.is_empty() {
            return Ok(()); // no below-threshold fixer in play; nothing to assert
        }
        let check_fixable = check
            .violations
            .iter()
            .filter(|violation| violation.is_fixable)
            .count();
        prop_assert_eq!(
            check_fixable,
            0,
            "check tagged {} violation(s) auto-fixable, but a bare fix only SUGGESTED \
             (did not resolve) them; config:\n{}",
            check_fixable,
            inline_config(&scenario),
        );
    }

    #[test]
    fn fix_unsafe_converges_when_fully_resolved(base in single_fixable_policy_test()) {
        // Convergence under `--unsafe-fixes` (round-7): the Safe-threshold
        // convergence law early-returns on a `Suggested` below-threshold fixer, so
        // since `file_remove` became Unsafe that op got ZERO apply/convergence
        // coverage from the property net. At the Unsafe threshold every shipped
        // fixer applies (`Safe <= Unsafe`, `Unsafe == Unsafe`), so a single
        // fully-applied `fix --unsafe-fixes` must leave `check` finding nothing --
        // restoring file_remove's apply + convergence coverage.
        let scenario = with_steps(base, vec![TestStep::FixUnsafe, TestStep::Check]);
        let Ok(run) = run_policy_test(&scenario) else { return Ok(()); };
        let Some((fix_report, check_report)) = extract_fix_then_check(&run) else {
            return Ok(());
        };
        // Same guard as the Safe law: a genuine skip/unfixable leaves a real
        // violation. No Suggestion-tier fixer is generated, so `suggested` is 0 at
        // the Unsafe threshold; guard it anyway for parity.
        if !fix_report.skipped.is_empty()
            || !fix_report.unfixable.is_empty()
            || !fix_report.suggested.is_empty()
        {
            return Ok(());
        }
        let residual = check_report.violations.len();
        prop_assert_eq!(
            residual,
            0,
            "check still reported {} violation(s) after a fully-applied single-rule \
             `fix --unsafe-fixes` (non-convergent fixer); config:\n{}",
            residual,
            inline_config(&scenario),
        );
    }
}

fn extract_fix_then_check(run: &PolicyTestRun) -> Option<(&StepObservation, &StepObservation)> {
    Some((run.observations.first()?, run.observations.get(1)?))
}
