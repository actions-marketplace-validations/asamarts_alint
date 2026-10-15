//! Static per-kind capability metadata.
//!
//! A [`KindMeta`] record describes what a rule kind can select and do without
//! constructing a rule instance. Built-in kinds use borrowed static data; future
//! plugin kinds can use the owned forms of the same [`Cow`] fields.

use std::borrow::Cow;

use serde::Serialize;

use crate::structured_format::Format;

/// How the engine dispatches a rule kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DispatchClass {
    /// The engine reads each matching file once and dispatches its bytes to the rule.
    PerFile {
        /// Whether the rule must see the complete repository index under `--changed`.
        full_index: bool,
    },
    /// The rule evaluates the repository index as a whole.
    FullIndex {
        /// Whether a path scope is available as the `--changed` skip predicate.
        path_scope: bool,
    },
    /// The rule is evaluated rule-major over an enumerable path scope.
    RuleMajorScoped,
    /// The rule runs once and has no path scope.
    SingleShot,
}

/// Whether the common `paths:` field is part of a kind's contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PathsSupport {
    Required,
    Optional,
    Forbidden,
}

/// Which `scope_filter:` predicates a kind accepts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScopeFilterSupport {
    Full,
    AncestorOnly,
    Rejected { reason: Cow<'static, str> },
}

/// How `git_tracked_only:` narrows a kind's index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GitTrackedSupport {
    None,
    FileOnly,
    DirAware,
}

/// Whether a Git range's `since:` input is accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SinceSupport {
    None,
    Optional,
    Required,
}

/// The syntax and semantics of a kind-specific selector field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectorType {
    Glob,
    GlobList,
    BasenameGlob,
    LiteralPath,
    PathTemplate,
    Regex,
    DiffGlob,
    Extract,
}

/// One kind-specific selector accepted by a rule kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SelectorMeta {
    pub name: Cow<'static, str>,
    #[serde(rename = "type")]
    pub selector_type: SelectorType,
    /// Whether a nested config can confine this selector by prefixing its root.
    pub rerootable: bool,
}

/// Git operations a kind may request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GitOperation {
    TrackedSet,
    Blame,
    Diff,
    Log,
    VerifyCommit,
    /// Mutates the repository index without deleting the working-tree file.
    Untrack,
}

/// A host capability a kind may exercise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Capability {
    SpawnUserCommand,
    Git {
        operations: Cow<'static, [GitOperation]>,
    },
    HostKeyring,
    EnvRead,
    EnvExport,
    WallClock,
    OutOfRootRead,
    WritesDuringCheck,
    UnixOnly,
}

/// Machine-checkable predicates for option-dependent capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OptionPredicate {
    ContainsLegacyEnvReference,
}

/// When a declared capability is active for a rule instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CapabilityCondition {
    Always,
    /// The option's effective value enables the capability: `true` for a
    /// boolean, or present/non-empty for an optional value. This does not mean
    /// merely that the YAML key was written (for example,
    /// `git_tracked_only: false` does not activate Git).
    OptionSet {
        option: Cow<'static, str>,
    },
    OptionMatches {
        option: Cow<'static, str>,
        predicate: OptionPredicate,
    },
    FixOpConfigured {
        op: Cow<'static, str>,
    },
    CommonFieldSet {
        field: Cow<'static, str>,
    },
}

/// A capability and the condition under which a kind may use it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CapabilityMeta {
    pub capability: Capability,
    pub condition: CapabilityCondition,
}

/// The maximum content access shape used by a kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContentAccess {
    PathOnly,
    Metadata,
    /// Reads a prefix. `max_bytes: None` means the length comes from a rule
    /// option and therefore has no kind-wide fixed maximum.
    Prefix {
        max_bytes: Option<usize>,
        /// Whether the same evaluation also reads file metadata.
        metadata: bool,
    },
    /// Reads a suffix. `max_bytes: None` means the length comes from a rule
    /// option and therefore has no kind-wide fixed maximum.
    Suffix {
        max_bytes: Option<usize>,
        /// Whether the same evaluation also reads file metadata.
        metadata: bool,
    },
    WholeFile,
    OtherFiles,
    Blame,
    Delegated,
}

/// A repository path option outside the kind's normal scope whose change can
/// alter the kind's verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DependencyMeta {
    pub field: Cow<'static, str>,
}

/// Static behavior and capability inventory for one canonical rule kind.
///
/// The boolean fields are independent schema capabilities, not states of one
/// state machine, so keeping them explicit makes record audits mechanical.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KindMeta {
    pub dispatch: DispatchClass,
    pub paths: PathsSupport,
    pub scope_filter: ScopeFilterSupport,
    pub git_tracked_only: GitTrackedSupport,
    pub respect_gitignore: bool,
    pub root_only: bool,
    pub selectors: Cow<'static, [SelectorMeta]>,
    pub since: SinceSupport,
    pub capabilities: Cow<'static, [CapabilityMeta]>,
    pub content: ContentAccess,
    pub formats: Cow<'static, [Format]>,
    pub dependencies: Cow<'static, [DependencyMeta]>,
    pub fix_ops: Cow<'static, [Cow<'static, str>]>,
    pub expect_matches: bool,
    pub nested_rules: bool,
}

/// Trust class applied to a fix operation before a rule-specific tier is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FixTrustClass {
    Spawning,
    ContentInjecting,
    FixedBehaviour,
}

/// Static trust metadata for one fix operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FixOpMeta {
    pub name: Cow<'static, str>,
    pub trust: FixTrustClass,
}
