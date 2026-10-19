# Tree Spec — YAML format for directory-tree fixtures

Status: **public, embedded in policy-test v1.** Implemented by `alint_testkit::treespec`. The node format is append-only within `test_version: 1`; incompatible interpretation changes require a new policy-test version.

## Model

A *tree* is a recursive structure with four node forms:

- **Regular file:** a YAML string scalar containing the file's UTF-8 contents.
- **Directory:** a YAML mapping whose keys are child names.
- **Executable file:** `{ "$exec": "contents" }`; materialized with mode `0755` on Unix.
- **Symbolic link:** `{ "$symlink": "relative-target" }`; materialization is Unix-only.

Node names come from the mapping keys of the enclosing directory. The top-level value is always a mapping. `$exec` and `$symlink` are reserved mapping keys rather than filenames.

### Minimal example

```yaml
Cargo.toml: "[package]\nname = \"demo\"\n"
src:
  main.rs: "fn main() {}\n"
  lib.rs: ""
docs: {}
```

Materialized: three regular files (`Cargo.toml`, `src/main.rs`, `src/lib.rs`) and one empty directory (`docs/`). `lib.rs` is an empty file.

## Operations

Three operations use this model. Implementations live in `treespec/{materialize,verify,extract}.rs` and are independent of alint rule types.

### `materialize(spec, root)`

Creates the described tree on disk under `root`. Intermediate directories are created as needed. Pre-existing entries at `root` are left untouched unless a spec node writes that path. The public policy-test runner validates node names, `.git` ownership, and symlink confinement before calling this lower-level operation.

### `verify(spec, root, mode)`

Compares the on-disk tree under `root` to `spec`. Two modes:

- **`Strict`** (default): `root` must contain exactly the paths in `spec`, with byte-identical contents. Extra files, missing files, and content mismatches are all failures.
- **`Contains`**: every path in `spec` must be present and match; extra paths on disk are permitted. Useful when a policy test asserts only selected artifacts.

`verify` returns a `VerifyReport` with path and discrepancy class for each mismatch. It verifies executable mode and link text for their tagged node forms.

### `extract(root, opts) -> spec`

The inverse for the portable regular-file subset: walk `root` and emit a `TreeSpec` matching it.

- UTF-8 regular files no larger than `ExtractOpts::max_inline_bytes` are inlined.
- Oversized and non-UTF-8 files are skipped.
- Symlinks are skipped.
- File metadata, including executable mode, is not captured.
- Names in `ExtractOpts::skip_names` are skipped component-by-component; `.git` is skipped by default.

## Limitations

1. **Extraction is lossy.** It does not preserve symbolic links, executable mode, binary content, timestamps, ownership, or other metadata.
2. **Sidecar content is absent.** Large or binary fixture content must use a package-owned `fixture` directory instead of inline `tree`.
3. **Symbolic-link materialization is Unix-only.** Policy tests containing `$symlink` must carry the appropriate platform tag.
4. **Special filesystem nodes are unsupported.** Windows reparse points, FIFOs, sockets, and device files are outside the format.

Tree specs have no independent version field. They are interpreted under the enclosing policy-test version.

## Disambiguation rule

Nodes are distinguished by YAML value shape:

- a string scalar is a regular file;
- a mapping without a reserved node shape is a directory;
- a single-entry `$exec` mapping is an executable file;
- a single-entry `$symlink` mapping is a symbolic link.

The reserved node objects reject unknown fields. Policy-test validation separately rejects unsafe entry names and escaping or invalid initial-tree link chains.

## Relation to other tools

- **`build-fs-tree`** uses the same scalar-file / mapping-directory convention. Its portable output can be materialized here, modulo the reserved tagged nodes.
- **`snapbox` / `trycmd`** use `*.in/` and `*.out/` directories for CLI snapshots. Tree specs instead model repository setup and final state.
- **`assert_fs`** is a fluent builder. A tree spec can be materialized into an `assert_fs::TempDir` when predicate assertions are also useful.
