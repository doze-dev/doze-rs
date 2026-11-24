# doze-rs tasks. `just` lists them. Each recipe is a name for a command this repo already runs (cargo xtask, the CI
# scripts in .github/ci, the bun workspaces); the commands stay the source of truth, so CI and `just` cannot drift.

set shell := ["bash", "-euo", "pipefail", "-c"]

# List the recipes.
default:
    @just --list --unsorted

# ── Everyday ────────────────────────────────────────────────────────────────────────────────────────────────────────

# The pre-push checks (fmt, clippy, codegen, themes, lint-size, fuzz-check, tests). Push only when this exits 0.
gates *args:
    cargo xtask gates {{ args }}

# The gates without the slow steps.
quick:
    cargo xtask gates --quick

# Format everything.
fmt:
    cargo fmt

# Clippy as CI runs it (the desktop app has its own recipe: `just app-lint`).
lint:
    cargo clippy --workspace --exclude doze-app --all-targets -- -D warnings

# Tests: the whole workspace (less the app), or one crate (`just test doze-fetch`), extra args passed to cargo.
test crate="" *args:
    if [ -z "{{ crate }}" ]; then cargo test --workspace --exclude doze-app {{ args }}; else cargo test -p {{ crate }} {{ args }}; fi

# Type-check a crate (or the workspace) without building it.
check crate="":
    if [ -z "{{ crate }}" ]; then cargo check --workspace --exclude doze-app --all-targets; else cargo check -p {{ crate }} --all-targets; fi

# ── CI, locally ─────────────────────────────────────────────────────────────────────────────────────────────────────

# CI's `check` job: fmt, clippy, tests, the kafka feature builds, the storage simulation, codegen, themes, sizes.
ci-check:
    .github/ci/check.sh

# CI's `os` job: doze-os and the module protocol.
ci-os:
    .github/ci/os.sh

# CI's `modules` job: each module boots its published engine on this machine's target (needs GH_TOKEN).
ci-modules target=`rustc -vV | sed -n 's/^host: //p'`:
    .github/ci/modules.sh {{ target }}

# Release size, package count and idle memory against xtask/weight-limits.txt (one package, or all).
weight *packages:
    cargo xtask weight {{ packages }}

# Regenerate what is generated: the aws-model crates and the app's themes.
generate:
    cargo xtask codegen
    cargo xtask themes

# Fail if anything generated is stale (CI's check).
generate-check:
    cargo xtask codegen --check
    cargo xtask themes --check

# ── Fuzzing (nightly, cargo-fuzz) ───────────────────────────────────────────────────────────────────────────────────

# Every fuzz target, by crate.
fuzz-list:
    for d in crates/*/fuzz; do c=${d#crates/}; echo "${c%/fuzz}: $(ls $d/fuzz_targets | sed 's/\.rs$//' | tr '\n' ' ')"; done

# Fuzz one target for a while, from its seeds (`just fuzz kafka-wire fuzz_frame 60`).
fuzz crate target seconds="30":
    cd crates/{{ crate }}/fuzz && mkdir -p corpus/{{ target }} && \
      seeds=$([ -d seeds/{{ target }} ] && echo seeds/{{ target }} || true) && \
      cargo +nightly fuzz run {{ target }} corpus/{{ target }} $seeds -- -max_total_time={{ seconds }}

# Every marked parser has a fuzz target.
fuzz-check:
    cargo xtask fuzz-check

# ── Release builds ──────────────────────────────────────────────────────────────────────────────────────────────────

# doze, the CLI alone (the size-built `release-doze` profile CI weighs).
build-doze:
    cargo build --locked --profile release-doze -p doze-cli

# doze-kafka as the registry ships it.
build-kafka:
    cargo build --locked --release -p doze-kafka

# doze-aws as the registry ships it (its own size profile; the version it prints comes from DOZE_AWS_VERSION).
build-aws version="":
    DOZE_AWS_VERSION="{{ version }}" cargo build --locked --profile release-aws -p doze-aws

# ── Desktop app (macOS) ─────────────────────────────────────────────────────────────────────────────────────────────

# Run the app from source.
app *args:
    cargo run -p doze-app -- {{ args }}

# Lint and test the app as CI's macos job does.
app-lint:
    cargo clippy -p doze-app --all-targets --features capture -- -D warnings
    cargo clippy -p doze-app --all-targets -- -D warnings
    cargo clippy -p doze-os --features ui --all-targets -- -D warnings
    cargo test -p doze-app --features capture

# Doze.app, signed ad hoc (or with DOZE_SIGN_ID), into target/bundle unless --out says otherwise.
app-bundle *args:
    cargo xtask app-bundle {{ args }}

# "Doze Preview.app" (its own identity, never the installed app's place), into target/bundle unless out says otherwise.
app-preview out="target/bundle":
    cargo xtask app-bundle --profile release-app --out {{ out }} --id dev.doze.preview --name "Doze Preview" --no-scheme

# First-frame time and idle memory (`just app-bench --real 6 --churn`); see apps/doze-app/src/cli.rs for the flags.
app-bench *args:
    cargo run --profile release-app -p doze-app -- bench {{ args }}

# ── Consoles (bun) ──────────────────────────────────────────────────────────────────────────────────────────────────

# Install both consoles' packages as CI does.
web-install:
    cd web && bun install --frozen-lockfile
    cd console && bun install --frozen-lockfile

# The kafka console (web/): build the embedded bundle, then typecheck, lint, test.
kafka-web:
    cd web && bun run build && bun run typecheck && bun run lint && bun test

# The aws console and the shared UI (console/): build the embedded bundle, then lint and test.
aws-web:
    cd console && bun run build && bun run lint && bun run test

# The aws console's browser tests (Firefox, WebKit, Chromium) against a debug doze-aws.
aws-e2e:
    cargo build -p doze-aws
    cd console && bun run e2e

# A console's dev server with hot reload: `just web-dev kafka` or `just web-dev aws`.
web-dev which:
    case "{{ which }}" in kafka) cd web && bun run dev ;; aws) cd console/apps/aws && bun run dev ;; *) echo "kafka or aws"; exit 2 ;; esac

# ── GitHub ──────────────────────────────────────────────────────────────────────────────────────────────────────────

# Start CI on a branch (CI is manual: main is protected and every job must be green).
ci-run branch=`git branch --show-current`:
    gh workflow run ci --ref {{ branch }}

# The newest CI runs of a branch.
ci-status branch=`git branch --show-current`:
    gh run list --workflow ci --branch {{ branch }} --limit 5

# What is in the merge queue.
queue:
    gh api graphql -f query='{repository(owner:"doze-dev",name:"doze-rs"){mergeQueue(branch:"main"){entries(first:20){nodes{position state pullRequest{number title}}}}}}' \
      --jq '.data.repository.mergeQueue.entries.nodes[] | "\(.position)  #\(.pullRequest.number)  \(.state)  \(.pullRequest.title)"'

# Each module's newest published tag.
module-tags:
    git fetch -q --tags origin
    for m in $(git tag -l 'module/*' | cut -d/ -f2 | sort -u); do echo "$m $(git tag -l "module/$m/v*" --sort=-v:refname | head -1)"; done
