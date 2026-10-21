# Private native macOS validation

This runbook defines how maintainers may use a privately managed macOS host to
supplement the public `cross-platform.yml` matrix. It deliberately contains no
hostname, address, username, filesystem layout, device identifier, or credential
reference. Keep those values in the maintainer's local SSH configuration; never
put them in this repository, CI variables, issue comments, or shared logs.

The private run is additional evidence, not a replacement for GitHub-hosted CI.
The public workflow remains the required, reproducible cross-platform gate.

## When to run it

Run native macOS validation after the Linux preflight for changes involving:

- path, symlink, permission, case-sensitivity, or Unicode-normalization behavior;
- terminal, PTY, process-spawning, or Git integration;
- platform-gated Rust code or dependencies;
- macOS packaging, deployment-target floors, installers, or Homebrew; or
- a release candidate that needs native evidence before it is pushed or tagged.

Routine documentation-only changes do not require it unless a generated or test
contract reads those documents. The docs-export render-and-verify path remains
Ubuntu-only by design (ADR-0014); do not treat a macOS docs-export result as a
replacement for `ci/scripts/docs.sh` on Linux.

## Trust boundary

`cargo build` and `cargo test` execute build scripts, procedural macros, test
binaries, and subprocesses with the account's OS privileges. Environment
sanitization does not sandbox that code or prevent access to absolute paths, an
unlocked Keychain, or the network.

Therefore:

1. Run only an exact, maintainer-reviewed commit from trusted project history.
2. Never run an arbitrary fork, pull-request head, patch archive, or dirty working
   tree under a personal macOS account.
3. Untrusted code requires a disposable VM or dedicated unprivileged account with
   no personal files, unlocked Keychain items, SSH material, cloud credentials, or
   shared writable directories. Restrict network egress to an approved dependency
   mirror where practical and destroy the environment after the job.
4. Do not enable SSH agent, X11, socket, or port forwarding. The remote test does
   not need them.

A sanitized environment reduces accidental disclosure; it is not a security
boundary against malicious code.

## Source transfer

Test an immutable SHA, not the caller's working directory. The preferred transfer
is a streamed `git archive`:

```sh
sha="$(git rev-parse HEAD)"
test -z "$(git status --porcelain)"
git archive --format=tar "$sha" |
  ssh -T \
    -o BatchMode=yes \
    -o StrictHostKeyChecking=yes \
    -o ForwardAgent=no \
    -o ForwardX11=no \
    -o ClearAllForwardings=yes \
    "$MACOS_VALIDATION_HOST" \
    "$MACOS_VALIDATION_COMMAND"

`MACOS_VALIDATION_HOST` and the reviewed remote command are shell-local values or
local SSH aliases. They MUST NOT be committed. Do not use `ssh -A`, `scp -r` of a
working tree, or a clone authenticated with a personal GitHub credential.

`git archive` transfers only tracked bytes from the selected commit. It excludes
`.git`, ignored files, untracked files, local configuration, hooks, remotes, and
credentials. The remote side creates synthetic local Git metadata with no remote
when Git-aware tests or dogfood need a usable repository.

## Remote workspace invariants

The reviewed remote command MUST enforce all of these invariants:

- Create one randomly named temporary base with `umask 077`.
- Put `src`, `home`, `cargo-home`, `tmp`, and `target` in separate sibling
  directories beneath that base.
- Never put `HOME`, `CARGO_HOME`, `TMPDIR`, or `CARGO_TARGET_DIR` inside `src`.
  Test sandboxes search parent directories; nesting them under `src` can make an
  ostensibly config-free test discover the repository's `.alint.yml`. A nested
  Cargo home also makes dogfood inspect downloaded dependency sources.
- Extract the archive only into `src`.
- Run Cargo and test commands through `env -i` with an explicit minimal
  environment: isolated `HOME`, `CARGO_HOME`, `TMPDIR`, and
  `CARGO_TARGET_DIR`; the installed toolchain's `RUSTUP_HOME`; a minimal `PATH`;
  `CARGO_INCREMENTAL=0`; `CARGO_TERM_COLOR=never`; `NO_COLOR=1`; and
  `RUSTFLAGS=-D warnings`.
- Set `CARGO_NET_GIT_FETCH_WITH_CLI=false`, `GIT_CONFIG_NOSYSTEM=1`,
  `GIT_CONFIG_GLOBAL=/dev/null`, and `GIT_TERMINAL_PROMPT=0`. The isolated Cargo
  home must contain no `credentials.toml` or private registry configuration.
- Initialize `src` as a new local Git repository, configure a non-personal local
  identity, add the extracted files, and create one deterministic commit. Do not
  add a remote.
- Install an `EXIT`, `HUP`, `INT`, and `TERM` trap before extracting source. The
  trap makes the base writable, recursively removes it, and preserves the command
  status.
- After SSH exits, independently verify that the temporary base no longer exists.

An ephemeral Cargo home is the safest default. If download caching becomes
necessary, use a dedicated mode-`0700` cache containing only public registry
objects, with no credentials or user Cargo configuration. Keep build outputs and
source trees ephemeral even when dependency downloads are cached.

## Validation contract

First mirror the public Apple Silicon job exactly:

```sh
cargo test --workspace --locked
```

Run it with `CARGO_INCREMENTAL=0` and `RUSTFLAGS=-D warnings`. For changes that
need stronger native evidence, also run:

```sh
cargo clippy --workspace --all-targets -- -D warnings

# Apple Silicon release floor. Use 10.12 for an Intel release host.
MACOSX_DEPLOYMENT_TARGET=11.0 \
  cargo build --release --locked -p alint

vtool -show-build "$CARGO_TARGET_DIR/release/alint"
"$CARGO_TARGET_DIR/release/alint" check
"$CARGO_TARGET_DIR/release/alint" --only rust-file-max-lines check .
"$CARGO_TARGET_DIR/release/alint" --format json list |
  jq -e '.schema_version == 1 and (.rules | length > 0)'
```

The `vtool` result must report `platform MACOS` and the requested minimum OS
version. Dogfood must complete without errors or analysis failures. The focused
file-length check and JSON query prove the refactored source and a machine-output
path through the native release binary.

Run Linux `ci/scripts/preflight.sh` separately. It owns formatting, generated-doc
checks, architecture validation, version pins, and the Ubuntu-only docs-export
contract; duplicating those platform-neutral or Linux-defined gates remotely adds
cost without stronger macOS evidence.

## Evidence and privacy

Record only:

- the tested Git SHA;
- architecture and relevant toolchain versions;
- the commands or phases run;
- pass/fail status and repository-relevant diagnostics; and
- the verified deployment-target value when applicable.

Do not record or publish the private host alias, network address, username, home
path, serial number, SSH key path, local topology, full environment, or unrelated
process/filesystem information. Avoid `env`, `set -x`, `ssh -v`, and broad system
inventory commands. Sanitize incidental private paths before sharing a failure
log.

A successful run is complete only when the remote cleanup is independently
verified and the local working tree is still clean at the recorded SHA.
