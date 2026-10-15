use std::collections::HashMap;
use std::sync::Arc;

use crate::config::RuleSpec;
use crate::did_you_mean;
use crate::error::{Error, Result};
use crate::kind_meta::KindMeta;
use crate::rule::Rule;

pub type RuleBuilder = fn(&RuleSpec) -> Result<Box<dyn Rule>>;

/// Internal storage form: a boxed builder, so option-less registration can
/// wrap a plain [`RuleBuilder`] with option validation. `Send + Sync` because
/// the registry is shared across the engine's worker threads.
type BoxedBuilder = Box<dyn Fn(&RuleSpec) -> Result<Box<dyn Rule>> + Send + Sync>;

#[derive(Debug)]
enum StoredKindMeta {
    Static(&'static KindMeta),
    Owned(Arc<KindMeta>),
}

impl StoredKindMeta {
    fn as_ref(&self) -> &KindMeta {
        match self {
            Self::Static(meta) => meta,
            Self::Owned(meta) => meta,
        }
    }
}

struct RegisteredKind {
    builder: BoxedBuilder,
    metadata: Option<StoredKindMeta>,
}

/// Canonical rule-kind registrations plus alias-to-canonical mappings.
///
/// Builders and metadata share one canonical entry, avoiding duplicate keys
/// and making it impossible for them to drift. Aliases store no builder or
/// metadata: build and metadata lookups both resolve through the canonical
/// entry.
#[derive(Default)]
pub struct RuleRegistry {
    kinds: HashMap<String, RegisteredKind>,
    aliases: HashMap<String, String>,
}

impl std::fmt::Debug for RuleRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuleRegistry")
            .field("kinds", &self.kinds.keys().collect::<Vec<_>>())
            .field("aliases", &self.aliases)
            .field(
                "metadata",
                &self
                    .kinds
                    .iter()
                    .filter_map(|(kind, registration)| registration.metadata.as_ref().map(|_| kind))
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl RuleRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a built-in kind and its borrowed static capability record.
    pub fn register(&mut self, kind: &str, meta: &'static KindMeta, builder: RuleBuilder) {
        self.insert_kind(
            kind,
            RegisteredKind {
                builder: Box::new(builder),
                metadata: Some(StoredKindMeta::Static(meta)),
            },
        );
    }

    /// Register a runtime-discovered kind and its owned capability record.
    pub fn register_owned(&mut self, kind: &str, meta: Arc<KindMeta>, builder: RuleBuilder) {
        self.insert_kind(
            kind,
            RegisteredKind {
                builder: Box::new(builder),
                metadata: Some(StoredKindMeta::Owned(meta)),
            },
        );
    }

    /// Register a builder before its catalogue record has migrated to
    /// [`KindMeta`]. W1 removes this path after all built-in records land.
    pub fn register_without_meta(&mut self, kind: &str, builder: RuleBuilder) {
        self.insert_kind(
            kind,
            RegisteredKind {
                builder: Box::new(builder),
                metadata: None,
            },
        );
    }

    /// Register an option-less rule kind with its capability record.
    pub fn register_optionless(
        &mut self,
        kind: &str,
        meta: &'static KindMeta,
        builder: RuleBuilder,
    ) {
        self.insert_kind(
            kind,
            RegisteredKind {
                builder: Self::optionless_builder(builder),
                metadata: Some(StoredKindMeta::Static(meta)),
            },
        );
    }

    /// Option-less counterpart to [`register_without_meta`](Self::register_without_meta).
    pub fn register_optionless_without_meta(&mut self, kind: &str, builder: RuleBuilder) {
        self.insert_kind(
            kind,
            RegisteredKind {
                builder: Self::optionless_builder(builder),
                metadata: None,
            },
        );
    }

    fn optionless_builder(builder: RuleBuilder) -> BoxedBuilder {
        Box::new(move |spec: &RuleSpec| {
            spec.deny_unknown_options()?;
            builder(spec)
        })
    }

    fn insert_kind(&mut self, kind: &str, registration: RegisteredKind) {
        self.aliases.remove(kind);
        self.kinds.insert(kind.to_string(), registration);
    }

    /// Register `alias` as another spelling of an existing canonical kind.
    ///
    /// The alias stores no duplicate builder or metadata and therefore cannot
    /// diverge from its canonical kind.
    pub fn register_alias(&mut self, alias: &str, canonical: &str) {
        assert!(
            !self.aliases.contains_key(canonical),
            "alias `{alias}` targets alias `{canonical}`; alias chains are not supported"
        );
        assert!(
            self.kinds.contains_key(canonical),
            "alias `{alias}` targets unregistered canonical kind `{canonical}`"
        );
        assert!(
            !self.kinds.contains_key(alias),
            "alias `{alias}` conflicts with a canonical kind"
        );
        assert!(
            self.aliases
                .insert(alias.to_string(), canonical.to_string())
                .is_none(),
            "alias `{alias}` is already registered"
        );
    }

    /// Resolve a kind or alias to its canonical capability record.
    pub fn meta(&self, kind: &str) -> Option<&KindMeta> {
        let canonical = self.canonical_kind(kind);
        self.kinds
            .get(canonical)
            .and_then(|registration| registration.metadata.as_ref())
            .map(StoredKindMeta::as_ref)
    }

    /// Canonical kinds that have declared capability metadata.
    pub fn kinds_with_meta(&self) -> impl Iterator<Item = (&str, &KindMeta)> {
        self.kinds.iter().filter_map(|(kind, registration)| {
            registration
                .metadata
                .as_ref()
                .map(|meta| (kind.as_str(), meta.as_ref()))
        })
    }

    /// Registered alias-to-canonical mappings, in arbitrary order.
    pub fn aliases(&self) -> impl Iterator<Item = (&str, &str)> {
        self.aliases
            .iter()
            .map(|(alias, canonical)| (alias.as_str(), canonical.as_str()))
    }

    /// Resolve `name` to its canonical kind: the target of an alias registered
    /// via [`register_alias`](Self::register_alias), or `name` itself when it is
    /// already canonical (or not registered at all).
    pub fn canonical_kind<'a>(&'a self, name: &'a str) -> &'a str {
        self.aliases.get(name).map_or(name, String::as_str)
    }

    /// Every registered canonical kind, in arbitrary order.
    pub fn canonical_kinds(&self) -> impl Iterator<Item = &str> {
        self.kinds.keys().map(String::as_str)
    }

    pub fn build(&self, spec: &RuleSpec) -> Result<Box<dyn Rule>> {
        let canonical = self.canonical_kind(&spec.kind);
        let builder = &self
            .kinds
            .get(canonical)
            .ok_or_else(|| Error::UnknownRuleKind(spec.kind.clone()))?
            .builder;
        let rule = builder(spec).map_err(|e| enrich_error(e, &spec.kind))?;
        if spec.expect_matches && !rule.supports_expect_matches() {
            return Err(Error::rule_config(
                &spec.id,
                format!(
                    "`expect_matches` is not supported by `{}`; use it only on rules with an enumerable file `paths:` scope (existence and selector rules have their own empty-set semantics)",
                    spec.kind
                ),
            ));
        }
        Ok(rule)
    }

    pub fn known_kinds(&self) -> impl Iterator<Item = &str> {
        self.kinds
            .keys()
            .map(String::as_str)
            .chain(self.aliases.keys().map(String::as_str))
    }
}

/// Apply [`did_you_mean::enrich`] to the message of a `RuleConfig`
/// error. Other error variants pass through unchanged.
fn enrich_error(err: Error, kind: &str) -> Error {
    match err {
        Error::RuleConfig { rule_id, message } => {
            let enriched = did_you_mean::enrich(kind, &message);
            Error::RuleConfig {
                rule_id,
                message: enriched,
            }
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::Level;
    use std::borrow::Cow;

    use crate::{
        ContentAccess, DispatchClass, GitTrackedSupport, PathsSupport, ScopeFilterSupport,
        SinceSupport,
    };

    static TEST_META: KindMeta = KindMeta {
        dispatch: DispatchClass::SingleShot,
        paths: PathsSupport::Forbidden,
        scope_filter: ScopeFilterSupport::Rejected {
            reason: Cow::Borrowed("not file-scoped"),
        },
        git_tracked_only: GitTrackedSupport::None,
        respect_gitignore: false,
        root_only: false,
        selectors: Cow::Borrowed(&[]),
        since: SinceSupport::None,
        capabilities: Cow::Borrowed(&[]),
        content: ContentAccess::PathOnly,
        formats: Cow::Borrowed(&[]),
        dependencies: Cow::Borrowed(&[]),
        fix_ops: Cow::Borrowed(&[]),
        expect_matches: false,
        nested_rules: false,
    };

    fn fake_spec(kind: &str) -> RuleSpec {
        RuleSpec {
            id: "t".into(),
            kind: kind.into(),
            level: Level::Error,
            paths: None,
            message: None,
            policy_url: None,
            when: None,
            expect_matches: false,
            fix: None,
            scope_filter: None,
            extra: serde_yaml_ng::Mapping::new(),
        }
    }

    #[derive(Debug)]
    struct FakeRule {
        id: String,
    }

    impl Rule for FakeRule {
        fn id(&self) -> &str {
            &self.id
        }

        fn level(&self) -> Level {
            Level::Error
        }

        fn evaluate(&self, _ctx: &crate::Context<'_>) -> Result<Vec<crate::Violation>> {
            Ok(Vec::new())
        }
    }

    // The function must match the public `RuleBuilder` signature used by the registry.
    #[allow(clippy::unnecessary_wraps)]
    fn ok_builder(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
        Ok(Box::new(FakeRule {
            id: spec.kind.clone(),
        }))
    }

    #[test]
    fn new_registry_has_no_kinds() {
        let r = RuleRegistry::new();
        assert_eq!(r.known_kinds().count(), 0);
    }

    #[test]
    fn register_without_meta_inserts_and_overwrites_a_kind() {
        let mut r = RuleRegistry::new();
        r.register_without_meta("my_kind", ok_builder);
        r.register_without_meta("my_kind", ok_builder);
        let kinds: Vec<&str> = r.known_kinds().collect();
        assert_eq!(kinds, vec!["my_kind"]);
    }

    #[test]
    fn alias_reads_the_canonical_static_metadata_record() {
        let mut r = RuleRegistry::new();
        r.register("file_content_matches", &TEST_META, ok_builder);
        r.register_alias("content_matches", "file_content_matches");

        let canonical = r.meta("file_content_matches").unwrap();
        let alias = r.meta("content_matches").unwrap();
        assert_eq!(canonical, &TEST_META);
        assert!(std::ptr::eq(alias, canonical));
        assert_eq!(r.kinds_with_meta().count(), 1);
    }

    #[test]
    fn owned_metadata_survives_the_callers_arc_and_is_shared_by_aliases() {
        let meta = Arc::new(TEST_META.clone());
        let ptr = Arc::as_ptr(&meta);
        let mut r = RuleRegistry::new();
        r.register_owned("plugin/check", Arc::clone(&meta), ok_builder);
        r.register_alias("plugin/check_alias", "plugin/check");
        drop(meta);

        assert_eq!(std::ptr::from_ref(r.meta("plugin/check").unwrap()), ptr);
        assert!(std::ptr::eq(
            r.meta("plugin/check_alias").unwrap(),
            r.meta("plugin/check").unwrap()
        ));
    }

    #[test]
    fn metadata_is_removed_when_a_kind_is_replaced_without_a_record() {
        let mut r = RuleRegistry::new();
        r.register("my_kind", &TEST_META, ok_builder);
        r.register_without_meta("my_kind", ok_builder);
        assert!(r.meta("my_kind").is_none());
    }

    #[test]
    fn build_rejects_unknown_kind_with_clear_error() {
        let r = RuleRegistry::new();
        let err = r.build(&fake_spec("not_real")).unwrap_err();
        match err {
            Error::UnknownRuleKind(name) => assert_eq!(name, "not_real"),
            other => panic!("expected UnknownRuleKind, got {other:?}"),
        }
    }

    #[test]
    fn alias_builds_with_the_canonical_builder_and_original_spec() {
        let mut r = RuleRegistry::new();
        r.register_without_meta("file_content_matches", ok_builder);
        r.register_alias("content_matches", "file_content_matches");

        assert_eq!(r.known_kinds().count(), 2);
        assert_eq!(r.canonical_kind("content_matches"), "file_content_matches");
        assert_eq!(
            r.canonical_kind("file_content_matches"),
            "file_content_matches"
        );
        assert_eq!(r.canonical_kind("not_registered"), "not_registered");
        assert_eq!(
            r.build(&fake_spec("content_matches")).unwrap().id(),
            "content_matches",
            "the canonical builder must receive the original alias-bearing spec"
        );

        let canon: Vec<&str> = r.canonical_kinds().collect();
        assert_eq!(canon, vec!["file_content_matches"]);
    }

    #[test]
    #[should_panic(expected = "targets unregistered canonical kind")]
    fn alias_registration_rejects_an_unknown_canonical_kind() {
        let mut r = RuleRegistry::new();
        r.register_alias("alias", "missing");
    }

    #[test]
    #[should_panic(expected = "alias chains are not supported")]
    fn alias_registration_rejects_an_alias_target() {
        let mut r = RuleRegistry::new();
        r.register_without_meta("canonical", ok_builder);
        r.register_alias("first", "canonical");
        r.register_alias("second", "first");
    }

    #[test]
    fn known_kinds_iterator_lists_all_registered() {
        let mut r = RuleRegistry::new();
        r.register_without_meta("a", ok_builder);
        r.register_without_meta("b", ok_builder);
        r.register_without_meta("c", ok_builder);
        let mut kinds: Vec<&str> = r.known_kinds().collect();
        kinds.sort_unstable();
        assert_eq!(kinds, vec!["a", "b", "c"]);
    }
}
