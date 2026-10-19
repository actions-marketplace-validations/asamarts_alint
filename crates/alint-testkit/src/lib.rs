//! Policy-test models, scenario runners, and declarative repository fixtures.
//!
//! [`policy_test`] is the versioned public document contract. [`scenario`] and
//! [`runner`] remain the repository-internal execution path until the P2/P3
//! runner cutover; they are intentionally not used to parse public v1 documents.
//! [`treespec`] is the shared declarative tree toolkit.

pub mod confinement;
mod error;
pub mod policy_test;
pub mod runner;
pub mod scenario;
pub mod strategies;
pub mod treespec;
pub use confinement::{ConfinementError, PackageEntryKind, PackageRoot, validate_package_path};
pub use error::{Error, Result};
pub use policy_test::{
    AnalysisFailureKindMatcher, AnalysisFailureMatcher, ConfigurationErrorMatcher, ExpectedExit,
    GivenV1, LevelMatcher, MAX_MATCHER_REGEX_BYTES, MessageMatcher, NoteMatcher,
    POLICY_TEST_VERSION, PolicyCommit, PolicyDetailedCommit, PolicyDocsCase, PolicyDocsExample,
    PolicyGit, PolicyTest, PolicyTestParseError, RelatedLocationMatcher, StepExpectation,
    TestCapabilities, TestStep, TreeExpectationMode, ValidationCode, ValidationDiagnostic,
    ViolationMatcher, policy_test_schema, policy_test_schema_json,
};
pub use runner::{ScenarioRun, StepOutcome, assert_scenario, run_scenario, setup_git};
pub use scenario::{
    AnalysisFailureKindName, CommitSpec, DetailedCommit, DocsCase, DocsExample,
    ExpectAnalysisFailure, ExpectStep, ExpectTreeMode, ExpectViolation, Given, GivenGit, LevelName,
    Scenario, Step,
};
pub use treespec::{
    Discrepancy, ExecNode, ExtractOpts, SymlinkNode, TreeNode, TreeSpec, VerifyMode, VerifyReport,
    extract, materialize, verify,
};
