# alint for Helix

Repository-structure linting from [alint](https://github.com/asamarts/alint)
in [Helix](https://helix-editor.com), via the `alint lsp` language
server. Helix speaks LSP natively, so this is a **config-only**
integration (an honorable mention, not a first-party package).

Requires the `alint` binary on `PATH`.

## Setup

Merge [`languages.toml`](./languages.toml) into
`~/.config/helix/languages.toml`: it declares the `alint`
language-server and adds it to a few languages' server lists.

## Known limitation

Helix attaches language servers **per-language** — there is no "all
files" wildcard. alint is repo-structural, so add `"alint"` to the
`language-servers` list of every language you want it to lint (see the
examples in `languages.toml`).
