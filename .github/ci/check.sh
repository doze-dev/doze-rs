#!/usr/bin/env bash
# The workspace check (ci.yml `check` on Linux, and the first part of the one `macos` job): format, lint, test, the
# kafka builds the workspace runs leave out, the generated and catalogued files, and the docs. Needs bun on PATH.
set -euo pipefail
step() { echo "::endgroup::"; echo "::group::$*"; "$@"; }
echo "::group::start"

step cargo fmt --check
# The desktop app is left out here: the `macos` job lints and tests it.
step cargo clippy --workspace --exclude doze-app --all-targets -- -D warnings
step cargo test --workspace --exclude doze-app
# The broker without the console (`--no-default-features`) and the console crate on its own: the two runs above build
# the default features only. The lean build is the Phase 8 broker with no HTTP in it (docs/port/weight.md, "lean").
step cargo clippy --no-default-features -p doze-kafka -p kafka-console --all-targets -- -D warnings
step cargo test -p kafka-console
step cargo test -p doze-kafka --no-default-features
# TLS is in the default build (one build since 2026-11-14), so the workspace runs above cover it; kafka-server's own TLS
# tests need its feature named, as the crate keeps it optional.
step cargo test -p kafka-server --features tls
step cargo clippy -p kafka-server --all-targets --features tls -- -D warnings
# The storage simulation (random workloads, a disk that fails and loses power at every file operation).
SIM_SEEDS=200 step cargo test -p kafka-storage -p kafka-meta --features kafka-storage/sim --release
# The generated aws-model crates are what the models and overlays produce (docs/port/aws/PLAN.md, rule 11).
step cargo xtask codegen --check
# The desktop app's themes are what the Go app's tokens.css gives (apps/doze-app/themes).
step cargo xtask themes --check
# No file grows past the hard size limit (docs/port/PLAN.md, quality rule 1).
step cargo xtask lint-size
# Every parser has a fuzz target (a `// fuzz:parser` marker with no target fails).
step cargo xtask fuzz-check
# The Phase 8 catalogue: each SEC number defined once, none referenced without a definition, every deviation in the
# index, every test name it quotes a real `fn` (tools/check-security-catalogue.ts).
step bun tools/check-security-catalogue.ts
# The same checks of the schema registry's catalogue (docs/port/behaviours/registry.md, REG-n).
step bun tools/check-registry-catalogue.ts
# The Phase 9 catalogue (docs/port/behaviours/console.md): each CON number defined once, none referenced without a
# definition, every test name it quotes a real `fn`; the lean tree has no HTTP and the default tree has the console
# (decision D1: `default = []` would take the console out and the size limits, being upper bounds, would not notice).
step bun tools/check-console-catalogue.ts --lean-tree --default-tree
# Public docs build without broken links.
RUSTDOCFLAGS="-D warnings" step cargo doc --workspace --exclude doze-app --no-deps
echo "::endgroup::"
