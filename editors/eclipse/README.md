# alint for Eclipse

Repository-structure linting from [alint](https://github.com/asamarts/alint)
in Eclipse, via the `alint lsp` language server and
[LSP4E](https://projects.eclipse.org/projects/technology.lsp4e). This is
an **honorable mention** — config via LSP4E, not a first-party packaged
plugin.

Requires the `alint` binary on `PATH`.

## Setup (LSP4E)

1. Install **LSP4E** from the Eclipse Marketplace (Help → Eclipse
   Marketplace → search "LSP4E").
2. Open **Preferences → Language Servers** and add a new server:
   - **Command:** `alint lsp`
   - Associate it with the content types you want linted (LSP4E binds
     servers to Eclipse content types).
3. Reopen affected editors; alint diagnostics appear inline and in the
   Problems view.

> LSP4E's "Language Servers" preference UI varies by version; on older
> LSP4E you may need a tiny plugin that contributes a
> `org.eclipse.lsp4e.languageServer` extension pointing at `alint lsp`.
> A first-party Eclipse plugin is out of scope — open an issue if you
> need one.

## Known limitation

LSP4E binds servers to **content types**, not "all files". alint is
repo-structural, so associate it with the content types (languages) you
want it to lint.
