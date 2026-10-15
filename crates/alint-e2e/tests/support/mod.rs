#![allow(dead_code)] // Each integration-test crate uses one of the shared loader helpers.

use std::path::Path;
use std::sync::LazyLock;

static REGISTRY: LazyLock<alint_core::RuleRegistry> = LazyLock::new(alint_rules::builtin_registry);

pub fn load(path: &Path) -> alint_core::Result<alint_core::Config> {
    alint_dsl::load_with(
        path,
        &alint_dsl::LoadOptions::with_catalog(REGISTRY.catalog()),
    )
}

pub fn load_with(
    path: &Path,
    options: &alint_dsl::LoadOptions<'_>,
) -> alint_core::Result<alint_core::Config> {
    let options = alint_dsl::LoadOptions {
        kind_catalog: Some(REGISTRY.catalog()),
        cache: options.cache.clone(),
        fetcher: options.fetcher.clone(),
    };
    alint_dsl::load_with(path, &options)
}
