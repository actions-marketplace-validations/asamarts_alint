# alint for JetBrains IDEs

In-editor diagnostics, hover-to-explain, and quick-fixes for
[alint](https://github.com/asamarts/alint) across the JetBrains suite
(IntelliJ IDEA, PyCharm, GoLand, WebStorm, RustRover, CLion, Rider,
Android Studio).

The plugin is glue: it registers `alint lsp` (the alint language
server) with [LSP4IJ](https://github.com/redhat-developer/lsp4ij) and
lets the IDE render the results. All linting logic lives in alint.

## How the binary is found

1. the path set in **Settings → … → alint** (`alint.path` equivalent),
2. `alint` on your `PATH`,
3. a copy the plugin previously downloaded,
4. otherwise a notification offers to download the matching release
   (opt-in, SHA-256 verified) — or install it yourself via Homebrew,
   `cargo install alint`, `npm i -g @asamarts/alint`, Docker, or the
   install script.

## Building locally

> The plugin was authored without a local Gradle/IntelliJ-SDK toolchain
> in CI. Validate it before publishing.

```sh
cd editors/jetbrains
gradle wrapper            # one-time: generates the wrapper jar
./gradlew buildPlugin     # produces build/distributions/alint-*.zip
./gradlew runIde          # launch a sandbox IDE with the plugin
```

`build.gradle.kts` carries a "verify-before-build" note listing the
version coordinates (IntelliJ platform, Kotlin, LSP4IJ, the Gradle
plugin) to confirm/bump on first build.

## Publishing

The `publish-jetbrains` job in the repo's `release.yml` runs
`gradle publishPlugin` on a version tag, gated on the
`JETBRAINS_MARKETPLACE_TOKEN` secret (plus signing secrets). The
`asamarts` Marketplace vendor must exist first.

## Versioning

The plugin version tracks the alint release it ships against.
