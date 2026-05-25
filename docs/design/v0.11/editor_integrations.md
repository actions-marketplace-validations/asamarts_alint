# Editor integrations — packaging the LSP server for six editors

Status: **Design draft**, written 2026-05-23. Expands the v0.11 LSP
scope (originally VS Code only — see `vscode_extension.md`) to a
six-editor matrix. Depends on `lsp_server.md` (the `alint lsp` stdio
server) and `single_file_reevaluation.md` (the per-edit hot path).

## Architecture: one server, thin wrappers

alint ships **one** language server — `alint lsp` (stdio, `tower-lsp`;
the dep landed in v0.9.7). LSP is the universal protocol, so every
target editor consumes the *same* binary; the per-editor work is a
thin wrapper that tells the editor "for these file types, launch
`alint lsp`." There is **no per-editor reimplementation** of
diagnostics, hover, or code actions — those live once in the server.

Two integration tiers fall out of this:

- **Tier 1 — packaged, published artifacts.** The editor needs a
  compiled/zipped plugin published to a marketplace before a user can
  install it: **VS Code**, **JetBrains** (whole family), **Zed**.
- **Tier 2 — config + docs.** The editor consumes any LSP binary via
  user config; the "integration" is a documented snippet (and, where
  one exists, a small community package): **Neovim**, **Sublime Text**,
  **Emacs** (plus honorable mentions Helix / Eclipse).

## Target editors + rationale

Picked for reach (Stack Overflow 2025 Developer Survey) intersected
with alint's audience (Rust / polyglot / OSS / monorepo) and
LSP-readiness:

| Editor | Why | Tier | Artifact | Channel |
|---|---|---|---|---|
| **VS Code** (#1, ~74%) | Dominant; also covers Cursor / Windsurf / VSCodium (VS Code forks) | 1 | `.vsix` (TS) | VS Code Marketplace **+ Open VSX** (forks read Open VSX) |
| **JetBrains** (IntelliJ + PyCharm + GoLand + WebStorm + RustRover + CLion + Android Studio) | One plugin → the entire JetBrains suite | 1 | plugin `.zip` (Kotlin/Java) | JetBrains Marketplace |
| **Neovim** (Vim #5; Neovim strong & dominant in alint's audience) | Built-in LSP client; config-only | 2 | `lsp/alint.lua` snippet + upstream `nvim-lspconfig` server entry | docs + nvim-lspconfig PR |
| **Zed** (Rust-built, fast-growing) | Natural fit for a Rust tool; *requires* an extension for a custom server (no config-only path) | 1 | Zed extension (`extension.toml` + Rust `language_server_command`) | Zed extension registry |
| **Sublime Text** (broad, mature) | `LSP` package is the de-facto standard | 2 | server config snippet + optional `LSP-alint` helper package | docs + Package Control |
| **Emacs** (large, mature) | `eglot` is built in (29+); `lsp-mode` huge | 2 | `eglot-server-programs` snippet + optional `alint.el` package | docs + MELPA |

**Honorable mentions (config-only, community-contributed, documented
but not first-party packaged):** Helix (`languages.toml`, Rust-built),
Eclipse (LSP4E). Each is a ~10-line config snippet pointing at `alint
lsp`; we document them but don't own a published package. Now shipped as
docs/snippets under `editors/helix/` and `editors/eclipse/`.

## Per-editor specifics

### VS Code (Tier 1) — see `vscode_extension.md`
- TS extension using `vscode-languageclient`; spawns `alint lsp`.
- Resolves the `alint` binary from PATH, a bundled download, or a
  `alint.path` setting.
- Publish to **both** the VS Code Marketplace (`vsce publish`) and
  **Open VSX** (`ovsx publish`) — Cursor, Windsurf, and VSCodium pull
  from Open VSX, so this one artifact reaches the whole VS Code-fork
  ecosystem.

### JetBrains (Tier 1)
- A single plugin targeting the IntelliJ Platform, registering
  `alint lsp` as an LSP server for the relevant file types.
- **Two viable client paths** (research, Sept 2025): the native
  **LSP API** (now free for plugin developers, no paid subscription
  needed to target it) or Red Hat's open-source **LSP4IJ** (works on
  *every* JetBrains flavor incl. Community). **Recommend LSP4IJ** for
  v0.11: open-source, widest compatibility, no dependence on the
  closed native API's licensing evolution.
- Built with Gradle (`gradle-intellij-plugin`); published as a `.zip`
  to the JetBrains Marketplace.
- One plugin covers IntelliJ IDEA, PyCharm, GoLand, WebStorm,
  RustRover, CLion, Rider, Android Studio.

### Zed (Tier 1)
- Zed has **no config-only path** for a custom external LSP binary — a
  custom server must be provided by an extension. So this is a small
  Rust extension: `extension.toml` declaring the server + an
  `Extension` impl whose `language_server_command` returns the `alint
  lsp` invocation (resolving the binary from PATH or a download).
- Published to the Zed extension registry (PR to
  `zed-industries/extensions`).
- Lowest-surface of the Tier-1 three; natural since both Zed and alint
  are Rust.

### Neovim (Tier 2)
- Ship an `lsp/alint.lua` config (`cmd = { "alint", "lsp" }`,
  `filetypes`, `root_markers = { ".alint.yml", ".git" }`) that users
  drop into their runtime path, plus a one-liner enabling it
  (`vim.lsp.enable("alint")`).
- Upstream the server definition to **`nvim-lspconfig`** so
  `:LspInstall`-style flows discover it; that PR is the real
  "distribution."

### Sublime Text (Tier 2)
- Document the `LSP` package server-config snippet (command +
  selector).
- Optionally ship an **`LSP-alint`** convenience package (mirrors
  `LSP-pylsp` / `LSP-ruff`) to Package Control that bundles the config
  + binary resolution — a thin JSON/Python package.

### Emacs (Tier 2)
- **`eglot`** is built into Emacs 29+ — register `alint lsp` via
  `eglot-server-programs`:
  `(add-to-list 'eglot-server-programs '((<modes>) . ("alint" "lsp")))`.
  `lsp-mode` users register an analogous client.
- Optionally ship a thin **`alint.el`** package to **MELPA** that does
  the registration + binary resolution (so users `(use-package alint)`
  rather than copy a snippet) — the Emacs analogue of `LSP-alint`.
- alint is repo-structural, not language-specific, so the snippet
  hooks broad modes (or `prog-mode` + a `.alint.yml`-presence check)
  rather than one language mode.

## Binary resolution (shared concern)

Every wrapper must locate the `alint` binary. Consistent strategy
across all five:
1. An explicit editor setting (`alint.path` / equivalent).
2. `alint` on `PATH`.
3. A managed download of the matching release binary (the same
   per-platform tarballs `install.sh` / the npm shim / Homebrew use),
   verified against the published SHA-256.

The VS Code and Zed extensions can do (3) automatically; JetBrains can
too (download into the plugin's sandbox). Neovim / Sublime docs lead
with (2) and point at the existing install channels.

## Packaging & CI

New release-time artifacts (extend `release.yml`, gated on the version
tag like the existing crates.io / npm / Docker / Homebrew jobs):

| Artifact | Build | Publish step | Secret |
|---|---|---|---|
| `alint-vscode-*.vsix` | `npm run package` (vsce) | `vsce publish` + `ovsx publish` | `VSCE_PAT`, `OVSX_PAT` |
| JetBrains plugin `.zip` | `gradle buildPlugin` | `gradle publishPlugin` | `JETBRAINS_MARKETPLACE_TOKEN` |
| Zed extension | (registry builds from source) | PR to `zed-industries/extensions` | — (manual/PR) |
| nvim-lspconfig entry | — | PR to `neovim/nvim-lspconfig` | — (manual/PR) |
| `LSP-alint` (Sublime) | — | Package Control PR | — (manual/PR) |
| `alint.el` (Emacs) | — | MELPA recipe PR | — (manual/PR) |

The two **token-published** marketplaces (VS Code + Open VSX,
JetBrains) join the existing release matrix and carry the same
mid-release-token-expiry risk as npm — see the npm-token recovery
note. Zed / nvim-lspconfig / Sublime are **PR-based** registries, so
they're a manual post-release follow-up, not a tagged CI job.

## Phasing (within the v0.11 LSP epic)

1. ✅ **`alint lsp` server** (`lsp_server.md`) + `single_file_reevaluation`
   — done. Diagnostics, hover, and apply-fix code actions all ship.
2. ✅ **VS Code extension** (`vscode_extension.md`) — done (`editors/vscode/`),
   with a tag-gated `publish-vscode` job (Marketplace + Open VSX). One
   prerequisite remains before the first publish: a local Node 18+
   `npm ci && npm run build` validation (the TS wasn't built in-repo).
3. ✅ **JetBrains plugin** (LSP4IJ) — done (`editors/jetbrains/`, Kotlin
   + `intellij-platform-gradle-plugin` 2.x), with a tag-gated
   `publish-jetbrains` job. **Build-validated:** `./gradlew buildPlugin`
   compiles the Kotlin against the real LSP4IJ 0.7.0 + IntelliJ 2024.2
   and packages the `.zip`; a committed Gradle wrapper + the `editors`
   CI job guard it. Still needs a manual `runIde` smoke (does the server
   actually attach in a live IDE?) and the `asamarts` Marketplace vendor
   + `JETBRAINS_MARKETPLACE_TOKEN` / signing secrets before publishing.
4. ✅ **Zed extension** — done (`editors/zed/`, Rust→wasm via
   `zed_extension_api`); compiles to wasm locally. Publishing is a
   manual PR to `zed-industries/extensions` (no release job). Known
   limitation: Zed attaches LSPs per-language, so `extension.toml` lists
   a broad common-language set rather than all files.
5. ✅ **Neovim + Sublime + Emacs docs/packages** + honorable-mention
   config snippets — done. `editors/{nvim,sublime,emacs,helix,eclipse}/`
   ship config snippets / a small `alint.el` / docs. Remaining work is
   the upstream/registry submissions (nvim-lspconfig PR, optional
   `LSP-alint` Package Control + `alint.el` MELPA recipes) — tracked as
   community follow-ups, not code in this repo.

Realistically 2-5 may slip to v0.11.x / v0.12 — the server + VS Code is
the v0.11 must-have; the rest is incremental once the server is stable.

## Out of scope
- Per-editor *native* (non-LSP) reimplementations — the whole point is
  one server.
- Visual Studio (proper) and Xcode — their plugin models don't consume
  an external LSP binary cleanly; revisit only on demand.
- A bespoke install manager — reuse the existing release tarballs +
  SHA-256 companions.

## Open questions
1. **Bundle the binary in each extension, or download on first run?**
   Bundling bloats the artifact ×5 platforms; downloading adds a
   first-run network hop. Lean: download-on-first-run with a pinned
   SHA (matches the npm shim), `alint.path` override for air-gapped.
2. **JetBrains: LSP4IJ vs the now-free native API?** Recommend LSP4IJ
   for v0.11 (open-source, Community-compatible). Re-evaluate if the
   native API's reach surpasses it.
3. **Open VSX as a hard release gate?** It's what reaches Cursor /
   Windsurf / VSCodium — arguably the most important channel given the
   AI-IDE growth. Treat it as co-equal with the VS Code Marketplace,
   not a nice-to-have.
