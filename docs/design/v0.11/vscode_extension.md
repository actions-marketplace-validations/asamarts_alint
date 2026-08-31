# VS Code extension

Status: **Implemented** (2026-05-24). The extension lives in
`editors/vscode/` (TypeScript, esbuild-bundled): it resolves the
`alint` binary (`alint.path` → PATH → opt-in download mirroring
`npm/install.js` → locate prompt), launches `alint lsp` over stdio via
`vscode-languageclient`, and registers the documented settings +
commands. A tag-gated `publish-vscode` job in `release.yml` publishes
to **both** the VS Code Marketplace (`vsce publish`, `VSCE_PAT`) and
Open VSX (`ovsx publish`, `OVSX_PAT` — what Cursor / Windsurf /
VSCodium read). **Caveat:** the TypeScript was authored but NOT built
or run in-repo (the dev container's Node is too old); a local
`npm ci && npm run build && npm run check-types` on Node 18+ is the
prerequisite before the first publish. Deviations from the draft are
noted inline. Originally one of six editor targets — see
[`editor_integrations.md`](./editor_integrations.md). Design draft
written 2026-05-02 after v0.9.6.

**Deviations from the draft below:**
- `alint.lintOnChange` / `alint.lintDelay` settings were dropped — the
  server owns the eval-on-change policy and exposes no flags for it
  yet, so those would have been dead config. Add them back if/when
  `alint lsp` grows the corresponding options.
- "Show effective rules" renders `alint list` output in the **alint
  output channel**, not a webview panel (smaller surface; revisit if a
  richer view is wanted).
- The single-file source layout is `src/{extension,binary,download,target}.ts`
  (the draft's `binary.ts` split into `binary.ts` + `download.ts`, plus
  a `target.ts` for the platform→triple map shared with the download
  path).

## Problem

Adopters install alint via Homebrew / install.sh / cargo / Docker, but
the in-editor surface is "run alint check in the terminal." VS Code is
the most common editor across alint's adoption tier (per the v0.7 field
test — JetBrains was the second-most-asked-for). A first-party
extension that ships and registers `alint-lsp` removes the manual
LSP-client wiring step.

## Surface area

A new directory `editors/vscode/` containing:

- `package.json` — extension manifest, registers the `alint` LSP
  client, declares activation events on `**/.alint.yml` /
  `workspaceContains:.alint.yml`.
- `src/extension.ts` — extension entrypoint. Locates the bundled
  `alint` binary (preferring `$PATH`, falling back to a copy shipped
  inside the extension), spawns `alint lsp`, registers the LSP client.
- `src/binary.ts` — platform-specific binary discovery and download
  logic, mirroring `npm/install.js`'s shape (same release archives,
  same SHA-256 checks).
- `tests/extension/` — `@vscode/test-electron` harness exercising the
  open → diagnostic → code-action loop end-to-end.

The extension does NOT ship its own diagnostics rendering — VS Code's
Problems panel and inline squiggles already render LSP diagnostics
natively. The extension is glue; the LSP server does the work.

## Semantics

Activation:

1. VS Code opens a workspace.
2. Extension activates on `workspaceContains:**/.alint.yml`.
3. Extension locates the `alint` binary (search `$PATH`, then
   extension-bundled, then prompt user to install).
4. Extension spawns `alint lsp` as the language server, registers
   document selector for files matching the open `.alint.yml`'s rules
   (effectively all text files in the workspace).
5. LSP server takes over — see [`lsp_server.md`](./lsp_server.md).

Settings (in VS Code's `settings.json`):

```jsonc
{
  "alint.binaryPath": "",          // default: auto-discover
  "alint.serverArgs": [],          // extra args appended to `alint lsp`
  "alint.trace.server": "off",     // "off" | "messages" | "verbose"
  "alint.lintOnChange": true,      // false → only on save
  "alint.lintDelay": 200           // debounce ms
}
```

Commands (palette):

- `alint: Restart LSP server` — kill + respawn.
- `alint: Show effective rules` — runs `alint list` and renders in
  a webview panel.
- `alint: Open .alint.yml` — opens the workspace's config.

## False-positive surface

- **No `alint` binary on `$PATH`** — extension prompts user with three
  options: Install (downloads to `~/.alint/bin/`), Browse (file picker),
  Cancel.
- **Multiple `.alint.yml` files in nested-config workspaces** — the LSP
  server handles config resolution; the extension just passes the
  workspace root.
- **VS Code on Windows with a musl Linux binary** — binary-discovery
  must check OS + arch; the same matrix `npm/install.js` uses applies
  here.

## Implementation notes

- TypeScript, bundled with esbuild (avoid the npm dep tree).
- Shipped to the VS Code Marketplace under publisher `asamarts`.
- Extension version tracks alint version (`0.10.0` extension ships
  `alint 0.10.x`); bumped on every alint release that changes LSP
  behaviour.

Complexity estimate: ~300 lines TS plus tests. ~3 days for a contributor
familiar with VS Code extension authoring.

## Tests

- `@vscode/test-electron` harness for: open workspace, expect
  diagnostics, hover, apply code action.
- CI: extension test runs on macOS + Linux + Windows (same matrix as
  alint's cross-platform CI).

## Open questions

1. **Should the extension auto-download the binary or always require
   user opt-in?** Opt-in. Auto-downloading executables behind the
   user's back is a foot-gun; extension marketplace reviews flag this.
2. **Should we publish to the Open VSX Registry too?** Yes, for VSCodium
   / Theia / Cursor / Eclipse Theia users. Same artifact.
3. **JetBrains plugin?** Out of scope for v0.10; tracked as a v0.11
   stretch goal. The LSP server itself supports JetBrains via the LSP4IJ
   plugin, so a first-party JetBrains plugin is a marketing question
   more than a technical one.
