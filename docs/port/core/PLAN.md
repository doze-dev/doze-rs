# doze core and CLI in Rust: the plan to finish

Status: **approved 2026-11-14** with the owner decisions in [section 8](#8-owner-decisions) (O1 reshaped and O2 reversed by the owner; the rest as recommended). Building with three builders. Owner's brief: "finish the real app and the real CLI in full… It's time to go
for the entire finish." When this plan is done, doze is Rust end to end: engine and daemon, doze.hcl, the full CLI, every
module, names and setup, the app. The Go product is then only an oracle.

Standing rules this plan obeys: the whole suite in Rust, no Go/Rust stitching (`all-rust-no-go-stitching`); ground-up Rust
idioms, Go read for *what*, never for *how* (`rust-port-write-it-in-rust`); honest sizing; prior art and the file
breakdown up front; a phase is done on evidence and an independent audit, not self-judged; Go is the floor, not the bar
(modules plan D16).

Grounding (2026-11-14): doze at `origin/main 23b73d6`, doze-names `5ca25dc`, doze-sdk `efb28ba`, doze-modules `origin/main
6aa83d8` and its local `pgext` branch, doze-rs `origin/main 2f1bb662` plus the open PR branches (`cli-build` #10,
`names-n4` #5, `names-n6` #6, `names-n5` #9, the app stack #8, #11 to #30), `cli-design` (the CLI proposal),
`app-plan` (the app plan), the HCL spike report (`_backup/spikes-2026-11-13/doze-hcl-spike.tgz`), and four read-only
surveys of the Go config, engine, control protocol and the doze-rs crates. Every Go line count below is `wc -l` of
non-test `.go` files, with test lines after the slash.

Contents: [1 Inventory](#1-scope-inventory) · [2 Architecture](#2-architecture) · [3 Phases](#3-order-of-work) ·
[4 CLI](#4-the-cli) · [5 Privileged work](#5-privileged-and-dns-work) · [6 Estimates and risks](#6-estimates-critical-path-and-risks) ·
[7 Prior art](#7-prior-art) · [8 Owner decisions](#8-owner-decisions) · [9 Files](#9-files)

---

## 1. Scope inventory

Legend for **In doze-rs**: **main** = merged on origin/main; **PR** = built on an open PR branch; **part** = some of it
exists; **new** = nothing yet; **drop** = not ported (with the reason).

### 1.1 doze (the Go core and CLI): 56 K Go lines, 20 K test lines, 562 test functions

| Go package | Lines | What it does | Rust destination | In doze-rs |
|---|---|---|---|---|
| `cmd/doze` | 6,147 / 1,495 | The CLI: 34 commands (3 hidden), global flags, three outputs, exit codes, `__daemon` / `__fetch` self-exec, detach | `doze-cli` (binary `doze`) | part: PR #10 has foundations plus `version`, `doctor` (no `--fix`), `modules docs/verify`, `update`, `completion`; 30 commands are stubs |
| `internal/config` | 4,890 / 2,993 | doze.hcl: load and merge files, kinds (`module`/`app`/`service`/`task`), variables and locals with precedence, `count`/`for_each`, `after` graph, references, functions, overrides, domains, diagnostics, comment-keeping editor | `doze-hcl` (the language) + `doze-config` (doze's schema over it) | new (spike only) |
| `internal/runspec` | 313 / 0 | Shared attributes of `app`/`service`/`task`: steps, `ready = tcp()/http()/log()/exec()` probe functions | `doze-config::runspec` | new |
| `internal/runtime` | 2,270 / 897 | The engine: lazy boot with coalesced cold starts, connection counting, idle reap, SpawnPlan execution in order with readiness and hooks, steps, tasks, watch, holders ("why awake"), port trap | `doze-engine` | part: the plan executor is in `doze-module-test::run` |
| `internal/registry` | 328 + 166 / 310 | In-memory state of every instance: `reaped → booting → active ⇄ idle → reaped`, `crashing`, `failed`, `done`; subscriptions | `doze-engine::lifecycle` (a sans-IO state machine) | new |
| `internal/supervisor` | 341 / 75 | One backend process in its own process group, logs to a sink, escalating stop | `doze-os::tree` + `doze-engine::supervise` | part: `doze-os::tree` on main (process group, PDEATHSIG, Job object, declared stop) |
| `internal/health` | 157 / 133 | Probes `http`, `tcp`, `socket`, `exec`, `log_line` | `doze-engine::ready` | part: `Ready` in `doze-module-test::run` |
| `engine/process` (+ `plugin`) | 960 / 397 | The built-in engine for `app`/`service`/`task`: run lines, restart `no`/`on-failure`/`always` with backoff, health, exempt from reaping | `doze-engine::process` | new |
| `internal/daemon` | 2,436 / 1,027 | Wires runtime, per-instance listeners, reaper, control socket, journal; the control-plane handler; ingress, domains, dynamic add/remove, meters, console, subscribe | `doze-daemon` | new |
| `internal/control` | 1,180 / 287 (+ `fakedaemon` 359, `fixture` ~1,450) | Control protocol 2: NDJSON over a unix socket, hello, multiplexed ids, cancel, 20 ops, 5 subscribe topics; client, server, the `Daemon` seam, a scripted fake daemon, the board fixtures | `doze-control` (types, codec, client, fixture daemon) + **server half** | PR #11 (client, types byte-compatible with Go goldens, fixture daemon); server half new |
| `internal/daemonctl` | 152 / 21 | Spawn the daemon (`doze __daemon`), stop, probe, pid, tail its log | `doze-daemon::ctl` | new (detach primitive in `doze-os::detach`) |
| `internal/proxy` | 463 / 143 | One listener per instance: accept → wake → count → splice; preamble/TLS/out-of-band filters | `doze-daemon::proxy` (+ module `Wire` capability for filters) | part: fd hand-off and `wire.accept` in protocol 2 |
| `internal/netplan` | 140 / 111 | What a module serves + config → listeners, backends, route names (`Grant`) | `doze-bind` | PR #6 |
| `internal/endpoints` | 235 / 95 | Client-facing address and connection string per instance; `.doze/endpoints.yaml` | `doze-config::endpoints` | new |
| `internal/state` | 328 / 94 | Applied objects per instance (`.doze/state.json`), what `sync` diffs and prunes | `doze-engine::applied` | new |
| `internal/logstore` | 614 / 311 | Per-instance runs as text segments on disk | `doze-engine::logstore` | new |
| `internal/journal` | 291 / 150 | What the daemon did (JSONL): wake, sleep, crash, restart, cause, duration | `doze-journal` (writer added) | PR #8 (reader) |
| `internal/projects` | 144 / 70 | `projects/<slug>/project.json`: where, last up/down, which services started (`up --last`) | `doze-projects` (writer added) | PR #8 (reader) |
| `internal/watch` | 99 / 68 | Polling watcher (size + mtime, quiet period) for `watch` and task inputs | `doze-engine::watch` | new |
| `internal/inputs` | 146 / 102 | Task `inputs` globs and their hash | `doze-engine::tasks::inputs` | new |
| `internal/hostboot` | 289 / 41 | The process-wide engine host: module manager, plugin manager, config hooks | `doze-engine::host` | part: `doze-module-host` pool |
| `internal/modules` | 1,051 / 539 | Signed registry index, module fetch, content-addressed cache | `doze-fetch` | part: PR #10 has the index, signature, select, meta, latest (in `doze-cli`); moves to `doze-fetch` |
| `internal/binaries` | 996 / 482 | Engine binaries from the doze-binaries mirror, checksums, cache, `doze.lock` | `doze-fetch` | part: PR #10 reads `doze.lock`; the writer is new |
| `internal/detect` | 850 / 160 | Reads a project (go.mod, imports, package.json, compose, Makefile) and proposes a doze.hcl | `doze-detect` | new |
| `internal/display` | 1,915 / 171 | The shared words: state, glyph, tone, "because", durations | `doze-display` | PR #12 (byte-identical to Go on 10,648 cases) |
| `internal/actions` | 170 / 52 | The one verb table the CLI and the dash project | `doze-control::verbs` + the CLI parity test | part: verbs in `doze-control` |
| `internal/fault` | 61 / 34 | Error kinds that survive the socket and become exit codes | `doze-control::fault` | PR #11 |
| `internal/portholder` | 193 / 132 | Who holds a port (lsof / ss / netstat) | `doze-os::ports` | part: the app's `doze-desk` has a ports helper; moves down |
| `internal/adminprompt` | 167 / 54 | A privileged script behind the system's admin prompt | `doze-names::setup` (privesc) | PR #9 |
| `internal/ui` | 446 / 0 | ANSI tables, colours, formatters for the CLI | `doze-cli::render` | part: PR #10 `output.rs` |
| `internal/brand` | 249 / 104 | Colours and type tokens for every surface | `doze-display::brand` | part (the app has its theme) |
| `internal/dash` | 7,294 / 3,413 | The terminal dashboard (bare `doze` over SSH), Bubble Tea | none | **drop** (O2: `status -w` is the screen over SSH) |
| `internal/desk/*` | 14,532 / 2,658 | The Wails desktop app (hub, views, serve, tray, notify, prefs, link, autostart, devserve, web) | `apps/doze-app` + `doze-desk` (app plan) | PR #15 to #30 (app plan, 171 days, not counted here) |
| `internal/dotdoze`, `dozeversion`, `netutil`, `inputs` | 37 + 68 + 29 / 49 + 17 | The `.doze` folder, own version, free ports | `doze-config`, `doze-cli`, `doze-os::net` | part |
| root package `doze` | 1,863 / 1,007 | The embeddable Go facade: `Serve`/`Attach`, `NewStack` builder, `Session.Topology` | `doze-engine::Engine::builder` + `doze-control::Client` | drop the Go API · **decision O4** |

### 1.2 Repositories the core depends on

| Repo / part | Lines | Rust destination | In doze-rs |
|---|---|---|---|
| doze-names (Go library v0.4) | 5,611 / 4,144 | `doze-names`, `doze-bind`, `doze-netd` | N0–N3 **main** (PR #4); N4 listen/netd **PR #5**; N5 setup/trust **PR #9**; N6 core side `doze-bind` **PR #6**. Root runs on macOS pending (owner) |
| doze-sdk (module SDK, protocol 1, go-plugin) | ~6 K + 5.5 K generated | `doze-module`, `-derive`, `-proto`, `-host`, `-test`, `-release` (protocol 2) | **main** |
| doze-modules: valkey 214, kvrocks 240, temporal 445 | | `doze-module-valkey`, `-kvrocks`, `-temporal` | **main** (better than Go, D16) |
| doze-modules: mariadb 607, ferret 1,013, postgres 1,920 (+2,577 on unpushed `pgext`, 11 commits) | | `doze-module-mariadb`, `-ferret`, `-postgres` | new (modules plan steps 9 to 13) |
| doze-kafka/module, doze-aws/module (Go module faces) | ~0.8 K, ~1.25 K | module mode in the Rust `doze-kafka` and `doze-aws` binaries | new (steps 14, 15); neither Rust binary references `doze_module` yet |
| doze-binaries | build tooling | stays (it builds engines, not doze) | n/a |

**"mailpit and so on"**: there is no Go mailpit module. Mailpit, minio, nats, meilisearch, memcached and mongo are
`internal/detect` recipes that write a plain `service` block run by the built-in process engine
(`internal/detect/render.go:20-28`). They come for free with `doze-engine::process` and `doze-detect`; a real module for
any of them is new work, out of this plan (listed under decision O9).

### 1.3 The oracle suites (Go's tests, used black-box)

| Suite | Size | How it runs against Rust | Hook needed in Go |
|---|---|---|---|
| `test/integration` (`-tags integration`) | 18 tests, 8 files, real engines | the harness builds `./cmd/doze` itself (`harness.go:83`) | `DOZE_BIN` and `DOZE_MODULE_BIN_DIR` (Rust module binaries) on the oracle branch |
| `cmd/doze/testdata/*.golden` | 11 goldens (status, daemon down, watch, wake, logs) | as transcripts in `doze-cli/tests/transcripts` | none (files copied, renderer differences listed) |
| `cmd/doze` exit codes, action parity | `exitcode_test.go`, `parity_test.go` | re-expressed as Rust transcripts and one parity test over the verb table | none |
| `internal/control` protocol + `fakedaemon` scripts | 14 tests, ndjson scripts | Go-captured frames replayed against the Rust server, normalised | a capture program (exists on `app-parity`: `testdata/go/capture`) |
| `internal/config` (107 tests, inline HCL) + `TestDocExamplesParse` (every HCL fence of the docs) + the HCL spike corpus (190 sources, 653 expressions) | | a Go **dump** program writes each loaded config (or its diagnostics with spans) as JSON; Rust writes the same; bun diffs | `tools/oracle-dump` on the oracle branch |
| `website/scripts/ground.ts` (`make ground`) | every command, flag and doze.hcl field the docs name | point it at the Rust binary | none: it already reads `DOZE_BIN` |
| `test/bench` (budget.json) | wake and `up`/`down` times on `examples/shop` | repoint the binary | `DOZE_BIN` |
| the app's real-daemon parity checklist | 286 rows (Go passes 285) | the same rows with the Rust daemon underneath the Rust app | none |
| `tools/module-oracle` (doze-rs) | 184 cases from the Go modules | exists | none |
| `tools/look` (dash scenes vs boards) | dash scenes | not used: the dash is dropped (O2) | none |

The Go edition stays the reference until cut-over: behaviour fixes land in Go first (as the kafka port did), then Go is frozen.

---

## 2. Architecture

### 2.1 Crates and their boundaries

```
   doze  (binary: CLI only, for CI and servers)        Doze.app's binary  (GUI + the full CLI)
        │  main.rs: doze_cli::main()                         │  main.rs: argv dispatch → doze_cli::main() or the GUI
        └──────────────────────┬─────────────────────────────┘
                          doze-cli  (library: every command, the self-exec dispatch, renderers)
                               │
   ┌──────────────┬────────────┴───┬─────────────┬────────────┐
   │ (client)  doze-daemon      doze-detect   doze-doctor   doze-names::netd
   │              │  (server)                                    (__netd)
   │      ┌───────┴────────┐
   │   doze-engine     doze-bind ── doze-names (registry, resolver, front door, CA, setup, trust)
   │      │   │   │
   │      │   │   └── doze-module-host ── doze-module-proto ── doze-os
   │      │   └────── doze-fetch (registry index, binaries, module fetch, doze.lock)
   │      └────────── doze-config ── doze-hcl
   │
   ├── doze-control (types, codec, client; server feature) ◄── also the GUI
   ├── doze-display (words) ◄── also the GUI
   └── doze-projects, doze-journal (formats) ◄── also the GUI

 GUI: doze-app (GPUI) ── doze-desk ── doze-control (client), doze-display, doze-projects, doze-journal
 Modules: separate binaries from the registry (doze-module-postgres, …), launched by doze-module-host
```

Arrows are Cargo dependencies, and Cargo enforces them. The rules each boundary keeps:

| Crate | Owns | Must not know |
|---|---|---|
| **`doze-hcl`** (new) | HCL as a language: hcl-edit syntax (vendored, pinned), values and types (cty-lite), 512-bit numbers (astro-float), conversion, the evaluator with spans, the 49 functions, `typeexpr`, merge and schema decode, diagnostics, the hclwrite-identical formatter, the comment-keeping editor | doze: no `module`, no ports. It could be published on its own |
| **`doze-config`** (new) | doze.hcl: the root schema as one data table (Go's `vocabulary.go` made the single source for decode, docs, lint and `ground`), kinds, variables/locals with precedence (`--var` > `DOZE_VAR_*` > `*.auto.doze.vars` > default), `count`/`for_each`, `after` and references, overrides, multi-file merge, the shape pass (no modules), module blocks validated against each module's declared schema (protocol 2, D1), endpoints, runspec, fingerprints, lint warnings. Output: a typed, validated `Stack` | processes, sockets, the network |
| **`doze-engine`** (new) | the stack at run time: the instance lifecycle (sans-IO), the dependency graph and start order, wake coalescing, holders, the reaper, the plan executor and supervision (promoted from `doze-module-test::run`), readiness, the process engine, steps, tasks and inputs, watch, the log store, the journal writer, applied state and `sync`/prune through module calls, the module pool. Input: a `Stack` (from `doze-config`, or built in code). It is the embeddable library | sockets for clients, the control protocol, names |
| **`doze-daemon`** (new) | the long-running process: the control server (every op and topic of protocol 2), the per-instance proxy listeners (wake on connect, splice, `wire.accept` hand-off, TLS termination), names through `doze-bind` (leases, front door routes, domains, dynamic add/remove), meters and stats, console and admin passthrough, `ctl` (spawn, stop, probe, detach) | terminals, rendering |
| **`doze-control`** (PR #11, extended) | protocol 2 types (byte-compatible with Go), the sans-IO NDJSON codec, the client, the fixture daemon; **new**: a `server` feature with a `Handler` trait and a tokio server loop, so client and server share one definition of every frame | engine internals |
| **`doze-fetch`** (new, from PR #10's `registry/`) | the signed index (ed25519 key compiled in), module and engine fetch, the content-addressed cache, `doze.lock` read **and write**, the host side of protocol 2's `fetch.*` | stacks |
| **`doze-cli`** (PR #10, extended; a **library** plus a thin `doze` bin, O1) | clap surface, the three renderers (terminal, plain, JSON envelope `doze/v1`), diagnostics renderer, notices, exit classes, transcripts; every command; the self-exec dispatch | engine internals (it is a client of the daemon, as the app is), except the read-only config shape for offline commands |
| **`doze-detect`** (new) | `init`: what a project needs, with the reason for each block, never invented (CLI proposal F6) | |
| **`doze-doctor`** (new) | checks (names, domains, registry, app, leftovers, versions) and the repair plan, as data; used by `doze doctor` and the app's Settings › Machine | the prompt itself (that is `doze-names::setup`) |
| `doze-names`, `doze-bind` | the zone, resolver, front door, CA, netd, setup and trust (names plan) | |
| `doze-os` (main) | every OS difference: process trees, channels, fd/socket hand-off, locks, owner-only dirs, cloning, detach, privilege, `which`, net, **new**: ports (who holds a port), named-pipe admin socket on Windows | |

### 2.2 What Rust makes natural (and what it replaces)

| Concern | Go design | Rust design |
|---|---|---|
| Instance lifecycle | `registry` with a mutex, timers and generation counters across `runtime`, `registry`, `daemon` | **sans-IO**: `Lifecycle::step(event, now) -> Vec<Effect>` per instance (events: connect, close, idle-deadline, exit(code), ready, probe-failed, verb). One actor per stack owns the machines and a `DelayQueue`; tests drive it with synthetic time and `proptest` event sequences (invariants: one boot at a time, a pinned instance is never reaped, `done` only from a run-to-completion task, the restart budget is never exceeded). Same pattern that held kafka's groups |
| Cold-start coalescing | singleflight | the waker holds a `watch::Sender<Phase>`; every connection awaits `Phase::Ready` or `Phase::Failed`; no lost wake-up by construction |
| Supervision | process group + signals, Unix only; plugins orphaned when the host dies (seen: `down`, `doctor`, `status` and the app leave plugins at PPID 1) | `doze-os::Tree` (group + PDEATHSIG on Linux, kill-on-close Job object on Windows, macOS: the control-socket EOF for modules and a startup sweep of recorded pids for engines); stop is what the module declares (D15); run lines are argument lists, never a shell (D14). **"Nothing left behind" is a gate**: after `down`, after SIGKILL of the daemon, after the app quits |
| Readiness | `health` probes | one `Ready` enum shared by the module harness and the engine (already written) |
| Config decoding | gohcl, 22 call sites, opaque gob specs to plugins | one schema table → typed values with spans; modules receive typed `Value` trees (protocol 2 D1); diagnostics come back as value paths and are mapped to spans |
| Control server | goroutine per connection, request map | a tokio task per connection, a `CancellationToken` per request id, one bounded channel per stream (a slow subscriber gets a fresh `snapshot`, as Go does, never unbounded memory) |
| Errors | `fault` kinds, wrapped errors | `thiserror` enums per crate mapped to `fault::Kind` once, at the boundary; `anyhow` only in the binary |
| Downloads | in-process, plus a `__fetch` child | the same split: `doze-fetch` in a child (`__fetch`) so a hung download never blocks the daemon |
| Embedding | Go `Serve` / `Attach` | `doze_engine::Engine::builder()` (in-process) and `doze_control::Client` (attach); the same API the daemon and tests use |
| Weight | 28-ish MB Go binary, Wails linked in | features: `tls-front-door`; the CLI-only `doze` carries no GUI; budgets in CI from the first phase |

### 2.3 Two binaries, one CLI (decision O1)

The owner's call (2026-11-14): **`doze` is a CLI-only binary** for CI and servers, and **the Doze.app binary contains both
the GUI and the full CLI**. So the CLI is a library, `doze-cli` (`lib.rs` with `pub fn main() -> ExitCode`), linked into
both:

- `doze` (crate `doze-cli`, `src/bin/doze.rs`) calls `doze_cli::main()` and nothing else. No GPUI, no GUI dependencies.
- The app binary (crate `doze-app`) looks at argv first: anything a CLI call would be (a subcommand, a flag, `__daemon`,
  `__netd`, `__fetch`, or a launch from a terminal) goes to `doze_cli::main()`; only a GUI launch starts GPUI. The `doze`
  the app puts on the PATH is that same binary, and the daemon, netd and fetch children the app starts are the app
  binary self-exec'd. One code path for every command, whichever binary runs it.

The dispatch inside `doze_cli::main()` is the first thing it does:

| argv | Runs | Notes |
|---|---|---|
| `doze __netd …` | `doze_names::netd::run` | **must be first**, before clap, logging or tokio (Go's `names.Helper()` rule). The netd that `dns-setup` installs is a copy of the `doze` binary that ran setup (Go's model) |
| `doze __daemon --config <abs>` | `doze_daemon::run` | one daemon per project (Go: `projects/<slug>/run/doze.admin.sock`, `doze.pid`, `doze.log`) |
| `doze __fetch …` | `doze_fetch::child` | downloads in a child |
| `doze` (bare) | terminal + GUI session → open `Doze.app`; SSH, no GUI, or piped → `status` | the dash is dropped (O2): `status -w` is the live screen over SSH |
| anything else | clap, the CLI | |

Weight: the 10 MB ceiling and the 9 MB CI line apply to `doze`. The app binary carries the CLI library on top of the GUI,
so its own line (app plan Q5, ≤ 16 MB) must absorb the CLI; the C0 weight probe reports both numbers.
Modules are separate binaries fetched from the registry, launched by `doze-module-host` over a socketpair (protocol 2).

**Where names and netd fit**: the daemon is the only writer of the zone for its stack (names plan); it takes leases
through `doze-bind` for each instance and its twins, routes HTTP names at the front door, answers from its own live
resolver before the registry (Go's `ServeResolveIn`), and records domains for `trust`. netd (macOS only) is the
root-started tunnel daemon that drops to the user; it is the same `doze` binary run as `__netd`. On Linux and Windows
there is no netd; services bind their own loopback addresses.

### 2.4 Compatibility at the seams (what must not change at cut-over)

1. **Control protocol 2, byte for byte.** The app's `doze-control` client is tested against Go's frames; the Rust daemon
   must answer the same frames (state words `reaped/booting/active/idle/crashing/failed/done`, Go duration strings,
   nanosecond `idle_timeout`, `snapshot` on fall-behind). Contract 2's renames (asleep/waking/awake/dozing) are a
   protocol 3 change made in one commit across daemon, app and CLI, after cut-over (decision O3).
2. **The CLI's machine contract**: `--json` envelopes (`doze/v1`), exit codes 0 1 2 4–9 130 (10 and 11 added), because the
   app runs `doze -y up|down` and `doze --json doctor|lint|status` as subprocesses.
3. **On-disk formats** under `DOZE_HOME` and the project: `project.json`, `run/` layout, `journal.jsonl`, log segments,
   `.doze/state.json`, `doze.lock`, `names.json` (format 2), data dirs `clusters/<instance>`. Kept so a person's data
   survives the switch and so Go and Rust can run side by side in the oracle (decision O3).
4. **Modules**: Rust doze speaks only protocol 2. A `doze.lock` that pins protocol-1 (Go) module releases is re-resolved on
   the first `up` to the protocol-2 release of the same module (compat.md rules apply; nothing published breaks).

---

## 3. Order of work

Every phase starts with a behaviour catalogue for its area (`docs/port/core/behaviours/<area>.md`, distilled from the Go
source, tests and docs; each item links to the Rust test or oracle case that holds it), ends with its gates green on the
five targets where it applies, the size and weight gates green, and **an independent audit** (a separate agent, not the
author) scoring it against the catalogue and the quality rules. Test diet (kafka plan rule 10) applies: no test per
mutant.

Three tracks run in parallel from day one (the critical path, section 6, assumes three builders):

- **L (language)**: H1 → H2 → H3 → H4. The longest chain.
- **E (engine)**: E1 → E2 → D1 → D2, built against a `Stack` made in code, so it never waits for HCL.
- **M (modules)**: the modules plan's steps 9 to 15, against the module harness, so it never waits for the core.
- The **CLI** (C1, C2) and **privileged** (P1) work joins where its inputs land.

### 3.1 Phases

| # | Phase | What | Gate (the Go oracle, and our own) | Days (range) |
|---|---|---|---|---|
| **C0** | **Rails and oracle hooks** | Merge the prerequisites in order (#8, #11, #12 app crates; #10 CLI foundations; #5, #6, #9 names) or rebase them onto one integration branch; crate skeletons (`doze-hcl`, `doze-config`, `doze-engine`, `doze-daemon`, `doze-fetch`, `doze-detect`, `doze-doctor`); `docs/port/core/behaviours/`. **Go oracle branch `rust-oracle` in doze**: `DOZE_BIN`/`DOZE_MODULE_BIN_DIR` in `test/integration`, `DOZE_BIN` in `test/bench`, `GROUND_BIN` in `ground.ts`, `tools/oracle-dump` (config JSON, diagnostics with spans, `fmt` output, control frames). **Go baselines measured**: binary, daemon idle RSS, wake latency, `status` latency, threads. A **weight probe**: a throwaway binary linking every dependency this plan picks, to see the 10 MB question on day 3, not day 80 | the oracle suites pass against the **Go** binary through the hooks (as kafka Phase 0); Rust CI green on five targets; `weight.md` has the Go baselines and the probe's size | 5 (4–7) |
| **C1** | **CLI on the control protocol** | The CLI commands that are pure clients of a running daemon: `status` (+ `-w`, `--graph` from the snapshot), `logs` (all flags, NDJSON follow), `wake`/`sleep`/`restart`/`reset`/`pin`/`on`/`off`/`task`, `why` (journal), the verb table and its parity test, `--json`, `-q`, notices, exit classes. Built and tested against `doze-control`'s fixture daemon and, as an oracle, against a running **Go** daemon (test-only; nothing ships that needs Go) | `cmd/doze/testdata` goldens as transcripts; the exit-code table; the verb parity test; same JSON envelopes as Go for the same frames | 9 (7–12) |
| **H1** | **doze-hcl: values and the evaluator** | hcl-edit vendored; cty-lite values and types with cty's key ordering; the 512-bit number type (spike code finished: `-0`, parse rules); conversion (string/number/bool, collections, objects, tuples); the evaluator on the hcl-edit AST (templates, directives, `for`, splat, conditional type unification, operators) with spans on every error; fuzz targets | the spike corpus: 653 expressions identical to go-cty (value and type), 190 sources accept/reject identically; no panic in `cargo fuzz` (the `5 % 0` class) | 21 (16–27) |
| **H2** | **doze-hcl: functions, types, diagnostics** | the 49 functions (41 cty stdlib, `toset/tolist/tomap/tostring/tonumber/tobool`, `env`, `env_of`) with cty's unification, RE2-compatible `regex`, `format` verbs, `csvdecode`; `typeexpr` for `variable` types; the diagnostics model (ranges, source snippet, did-you-mean); syntax-error message mapping | per-function differential tables against go-cty (every function × typical, edge and error args); diagnostics: same line and column as Go on the corpus | 16 (12–21) |
| **H3** | **doze-hcl: merge, decode, format, edit** | multi-file merge and schema decode (what gohcl and `PartialContent`/remain did); the formatter, byte-identical to `hclwrite.Format`; the comment- and order-keeping editor (`doze add`, `init`) | `fmt`: 177/177 sources byte-identical (the spike had 0/177); edit round trips keep every comment | 10 (7–13) |
| **H4** | **doze-config** | the doze.hcl schema as one table; kinds and old-syntax pointers; variables/locals precedence and cycles; `count`/`for_each`; `after` (with `started(...)`) and reference resolution with positioned cycle errors; module blocks against the module's declared schema (fetched manifests, cached); overrides and multi-file; endpoints; runspec probe functions; the shape pass; lint warnings; fingerprints | **`tools/oracle-dump` diff**: every inline HCL of the 107 Go config tests, `TestDocExamplesParse`'s docs fences, `examples/*`, the larder fixture → identical loaded config JSON or identical diagnostic positions; `ground.ts` fields; wording re-baselined (O6) | 15 (11–19) |
| **E1** | **doze-engine: lifecycle and processes** | the sans-IO `Lifecycle` and the stack actor; graph and start order; wake coalescing; holders; reaper; the executor promoted from `doze-module-test::run` (one copy, the harness then uses the engine's); readiness; the process engine (argument lists, `$PORT`/`$DATA`, restart policy and backoff, health); steps (`setup`, `on_ready`, `stop`) | proptests over event sequences with stated invariants; supervision tests on five targets incl. **zero processes left** after `down`, after SIGKILL of the host, after a crash loop; Go's `runtime`/`registry`/`process` tests as catalogue items | 16 (13–20) |
| **F1** | **doze-fetch** | move PR #10's registry code here; engine binaries from the mirror with checksums; module fetch and cache; `doze.lock` writer (pins on `up`, `modules upgrade`); `__fetch` child; the host side of `fetch.*` | Go `binaries`/`modules` tests as catalogue; fuzzed parsers (index, lock, meta exist); a local registry fixture (the live one has no DNS: F1 of the CLI proposal) | 7 (5–9) |
| **E2** | **doze-engine: tasks, logs, state** | tasks with `inputs` hashing and re-run rules; `watch` (polling, as Go chose); the log store (runs, segments, Go's layout); the journal writer; applied state and `sync`/prune through module `converge`/`objects`/`prune`; the module pool with the real fetch service; port trap and who-holds-the-port | Go `logstore`, `journal`, `state`, `inputs`, `watch` behaviours; the app's journal/log readers read what Rust writes (cross-language format tests, as names did) | 10 (8–13) |
| **D1** | **doze-daemon: control server** | `doze-control` `server` feature; every op (`status`, `up`, `apply`, `destroy`, `down`/`sleep`, `boot`/`wake`, `restart`, `reset`, `task`, `logs` incl. follow, `events`, `subscribe`, `add`, `remove`, `pin`/`keepawake`, `off`, `on`, `resources`, `admin`, `cancel`) and topic (`instances`, `stats`, `journal`, `logs`, `console`); `ctl` (spawn, stop, probe, detach on three OSes); stats and meters | Go-captured frame scripts replayed against the Rust server (normalised); the `doze-control` client suite against it; **milestone "app preview on the Rust daemon"**: the app's 286-row parity checklist with stacks built in code | 12 (10–15) |
| **D2** | **doze-daemon: proxy and names** | per-instance listeners, wake on connect, splice, connection counting; `wire.accept` hand-off (postgres filter) and TLS termination; names via `doze-bind` (leases, twins, front-door routes, domains, dynamic add/remove, the live resolver); the netd path on macOS | `test/integration` `names_test`, `module_names_test`, `service_test` (9) through `DOZE_BIN`; `test/bench` wake latency within Go's budget | 10 (8–13) |
| **C2** | **The CLI, complete** | everything else, on the Rust daemon and `doze-config`: `up` (`-f`, `--last`, `--all`), `down` (verifies, F7), `sync`, `env`, `run`, `url`, `connect`, `migrate`, `init` (+ `doze-detect`), `add`, `lint` (partial exits 11), `fmt`, `open` (+ `--show`, `doze://`, `--console`), `modules` / `upgrade` (lock writer), `bug`, `update` (daemon hand-over), `--dry-run` on the 18 mutating commands, dynamic completions, bare `doze`'s faces | every website command and flag (`ground.ts` with the Rust binary, and section 4's table); transcripts for F1–F8; `test/bench` budgets; `status`/`env`/`url` under 15 ms warm | 20 (16–25) |
| **P1** | **Privileged and DNS** | `dns-setup` (`--check`, `--dry-run`/`--print`, `--uninstall`), `trust` (`--check`, `--remove`, `--rotate`), `doctor --fix` and its checks (`doze-doctor`), the app's Repair via one admin prompt; owner-run root scripts (section 5) | names setup tests (Lima VM, Windows elevated CI); the owner's macOS root runs pass; `doctor` checks the registry (F5) | 7 (5–9) |
| **M1** | **mariadb, ferret, postgres** | modules plan steps 9 (mariadb 3), 10 (ferret 5), 11 (postgres core 9), 12 (postgres wire filter 3) | module oracle + real-engine acceptance; `test/integration` postgres, mariadb, ferret tests through `DOZE_BIN` | 20 (16–26) |
| **M3** | **kafka and aws module faces** | steps 14 (kafka 3) and 15 (aws 4) | differential vs the Go modules; Larder on Rust doze | 7 (6–10) |
| **X** | **Cut-over** | Section 3.2 | Section 3.2 | 8 (6–11) |
| **M2** | **postgres extensions** | step 13: catalog, bundles via `fetch.*`, ledgered install, PG18 paths (needs `pgext` merged in Go, O7) | `bundle_test` scenarios, the gated pgext e2e on 17 and 18 | 9 (7–12) |
| **W** | **Windows, whole** | the daemon's named-pipe admin socket, `ctl` detach, the process engine and supervision under Job objects, names (NRPT), the CLI end to end; integration suite on `windows-latest` and `windows-11-arm` | `test/integration` (minus engines with no Windows build) green on both Windows runners | 9 (7–12) |
| | **Total** (phase T, the terminal dash, removed by O2) | | | **211 (164–274)** |

The modules rows (M1, M2, M3 = 36 days) are the modules plan's remaining steps, counted here because the brief puts them
in; they are not extra on top of that plan.

### 3.2 The cut-over

**When:** after C2, D2, P1, M1 and M3 are done and audited. With three builders this is about day 95, or about day 83
with the two shortcuts in section 6.2.

**Gates (all against the Rust binary, on macOS and Linux):**
1. `test/integration`: all 18 tests (`DOZE_BIN`, Rust modules).
2. The app's 286-row real-daemon checklist with the Rust daemon and Rust CLI underneath: as many rows as Go (285).
3. `ground.ts`: every command, flag and doze.hcl field the docs name exists.
4. `test/bench` budgets; `weight.md` gates (section 6.3); zero leftover processes after every scenario.
5. The owner's day: `examples/shop` and Larder up, used, slept, woken, down, on his Mac, with names and trust.
6. An independent audit of the whole (not per phase): fidelity, quality rules, "nothing left behind".

**Mechanics:**
- `Doze.app` bundles the Rust `doze` and `doze-app`. The app's `doze-control` client needs **no change** (2.4.1); its
  subprocess calls (`doze -y up`, `--json doctor|lint|status`) now hit the Rust CLI (2.4.2).
- Homebrew formula and `install.sh` point at the Rust release; release tooling extends `doze-module-release` to `doze`.
- Upgrade with a Go daemon running: Rust `doze` speaks protocol 2 to it, so `doze update` (and the first Rust `up`) asks
  the Go daemon to `down`, waits for its socket to go, and starts its own; data dirs, `project.json`, logs, journal and
  state are the same formats, so nothing is lost.
- The machine switch on macOS: the Go netd launchd job (`dev.doze.netd`) is replaced by `doze dns-setup` once (one prompt).
- `doze.lock` entries pinned to protocol-1 modules are re-resolved to protocol 2 on the first `up`.
- The registry publishes protocol-2 module releases (they can go out before cut-over: Go selects only protocol 1).
- **Go is frozen**: tagged, kept buildable as the oracle, no new behaviour.

**Not gating cut-over** (still in "the entire finish", built in parallel where a builder is free, section 6.2): M2
(postgres extensions) and W (Windows); then protocol 3 (Contract 2's state words) if O3 says so.

### 3.3 What reaches the CLI and the app first

Builders: **L** runs C0 then H1–H4, C2 and X; **E** runs E1, F1, E2, D1, D2, then W; **M** runs C1, M1, M3, P1,
then M2. Days are from the start, without the shortcuts of 6.2.

| Day | What becomes real |
|---|---|
| ~5 (C0) | the oracle hooks; every PR stack merged; Go baselines; the weight probe |
| ~14 (C1, builder M) | the Rust CLI's daily read and verb commands, tested against fixture and Go daemons |
| ~50 (E1, F1, E2, D1) | **the app on the Rust daemon** (preview: stacks built in code, not yet from doze.hcl) |
| ~60 (D2) | proxy, wake on connect, names: the integration `service`/`names` tests on Rust |
| ~67 (H4) | `lint`, `fmt`, `init`, `add`; the app's stopped-stack shape and Map refs in-process (no `doze status --json` subprocess) |
| ~87 (C2) | the whole CLI |
| ~95 (X) | cut-over |

---

## 4. The CLI

The CLI proposal (`cli-design`: `docs/port/cli/proposal.md`) is adopted whole: its rules 1 to 8, its command surface,
`--dry-run` on every mutating command, exit codes 10 and 11 added after Go's published ones, `connect`, `why`,
`open --console`, `update`, `bug`, `modules`, `modules docs [--web]`, `modules verify`. Its 31 days are folded in as:
5 already built (PR #10: foundations plus `version`, `doctor` read side, `modules docs/verify`, `update`, `completion`),
**9 in C1**, **20 in C2** (incl. `doze-detect` 4, `--dry-run` 3, `connect`/`why`/`bug`/`open --console` 5), and
`doctor --fix`/`dns-setup`/`trust` in **P1**.

Every command and flag the website documents (the proposal's coverage table: 38 commands named by the website, 43
flags), and where it lands:

| Group | Commands and flags | Phase |
|---|---|---|
| Global | `-c/--config` (dir merges `*.hcl`), `--var`, `--json`/`DOZE_JSON` (NDJSON for `logs -f`, `status -w`), `-q`, `-y`/`DOZE_YES`, `--timeout`, `--no-color`/`NO_COLOR`, `-h`, exit codes | C1 (flags, envelopes, codes); `-c`/`--var` need H4 → C2 |
| The app | bare `doze` (app, or `status` over SSH and piped), `open [dir]`, `open doze://stack[/service]`, `open --show`, `open --console` (new) | C2 (over SSH, bare `doze` is `status`: O2) |
| The stack | `up [svc…] -f --last --all`, `down`, `sync [svc] --dry-run`, `status` (`tree`/`ls`/`ps`) `-w --graph`, `env [svc…] --dotenv`, `run [--env] -- cmd` | `status` C1; the rest C2 |
| One service | `wake` (`boot`), `sleep` (`reap`), `restart`, `reset`, `migrate`, `task`, `pin` (`keepawake`) `--off --toggle`, `off`, `on`, `logs -f --run --since -n`, `url` (`copy`), `connect` (new), `why` (new) | verbs, `logs`, `why` C1; `migrate`, `url`, `connect` C2 |
| Set up and edit | `init --wizard --force`, `add --version --port --as --dry-run`, `lint --offline`, `fmt --check`, `doctor --fix --print-fix`, `dns-setup --check --print --uninstall`, `trust --check --print --remove --rotate` | `init`/`add`/`lint`/`fmt` C2 (need H3/H4); `doctor`/`dns-setup`/`trust` P1 |
| Modules and lock | `modules` (bare, new), `mod`, `modules upgrade [type…] --check`, `modules docs <m> [--web]`, `modules verify` | `docs`/`verify` built (PR #10); bare and `upgrade` C2 (lock writer F1) |
| Other | `version` (first line kept for the site's parser), `completion bash\|zsh\|fish\|powershell` (dynamic), `help`, `update` (new), `bug` (new) | built or C2 |
| `--dry-run` | on `up`, `down`, `wake`, `sleep`, `restart`, `reset`, `migrate`, `task`, `pin`, `on`, `off`, `init`, `fmt`, `modules upgrade`, `trust`, `dns-setup`, `doctor --fix`, `update` | with each command |
| Internal | `__daemon`, `__fetch`, `__netd` | D1, F1, P1 |
| Dropped (website already says gone) | `dash`, `up --detach`, `sync --auto-approve`, `shell`, `psql`, `modules search`, `binaries` | — |

Gate for the CLI as a whole: `ground.ts` against the Rust binary reports nothing missing, and the proposal's three
website fixes are made in the doze repo docs and doze-website when the Rust CLI ships.

---

## 5. Privileged and DNS work

What needs root, once per machine, never per run (names plan): **dns-setup** (macOS: the netd launchd job and
`/etc/resolver/doze`; Linux: `doze0` with 192.0.2.53, the resolved or dnsmasq route, netd's systemd unit if any, the
marked `/etc/hosts` block, the privileged-port sysctl; Windows: the NRPT rule to 127.0.0.53:53), **trust** (the machine
CA into the keychain / ca-certificates / the Windows store; per-project domain routes; stale routes removed in the same
prompt), and **doctor --fix** (runs whichever of the two the checks say is broken).

How it is built (P1), on what N4/N5 already have:
- Each step is data (`Step { check, apply, undo, says }`, names plan); `--check`, `--dry-run` (aliases `--print`,
  `--print-fix`), the script and the app's "what will happen" sheet are all generated from the same list.
- **From the CLI**: `sudo` from the terminal, one prompt for the whole script; each step reported separately.
- **From the app**: Settings › Machine › Repair shows the plan (`doze --json doctor --fix --dry-run`), then runs
  `doze doctor --fix` behind **one** system admin prompt (`privesc`: osascript on macOS, pkexec on Linux, UAC on
  Windows). The app never runs anything privileged itself; it runs the CLI, so there is one code path.
- `doze-doctor` adds the registry check (F5), the leftovers sweep (processes, sockets, routes, CA, data), and partial
  results that exit 11.

**Owner-run steps on macOS** (the agent cannot sudo on his Mac; each script stops Go's `dev.doze.netd` for the run and
restores it, and uses a throwaway `DOZE_HOME`):

| # | When | Command | Proves |
|---|---|---|---|
| 1 | now (C0) | `./scripts/names-e2e-darwin.sh` on branch `names-n4` | the Rust netd: tunnel, translation, table cache, 2,000 services (still pending) |
| 2 | now (C0) | `./scripts/names-setup-e2e-darwin.sh` on branch `names-n5` | setup and trust: the launchd job, `/etc/resolver/doze`, the keychain (pending) |
| 3 | end of P1 | `./scripts/core-privileged-e2e-darwin.sh` (new, written in P1) | `doze dns-setup --check` → `dns-setup` (one prompt) → `doze up` on `examples/shop` → `curl http://api.shop.doze` → `doze trust` (one prompt) → `curl https://…` → `trust --remove` → `dns-setup --uninstall` → Go's setup restored |
| 4 | end of P1 | in the app: Settings › Machine › Repair, once | the GUI admin prompt path |
| 5 | cut-over | `doze dns-setup` with the released Rust doze | the machine switch: Rust netd replaces Go's for good |

Linux root runs happen in the Lima VM by the agent (`scripts/names-e2e-linux.sh` pattern); Windows on the elevated CI
runners.

---

## 6. Estimates, critical path and risks

### 6.1 Estimates

Working days for one builder agent per track, review-gated, independent audits included, honest rather than compressed.
They come from the Go line counts each phase replaces, the HCL spike's measured breakdown (47 days, 35–60), the modules
plan's per-step numbers, the CLI proposal's 31 days, and the pace measured on names (N0–N3: 10 days planned, met) and
the app (M0: 26 planned).

| Track | Phases | Days |
|---|---|---|
| Rails | C0 | 5 (4–7) |
| Language | H1 21, H2 16, H3 10, H4 15 | 62 (46–80) |
| Engine and daemon | E1 16, F1 7, E2 10, D1 12, D2 10 | 55 (44–70) |
| CLI | C1 9, C2 20 | 29 (23–37) |
| Privileged | P1 | 7 (5–9) |
| Modules | M1 20, M3 7, M2 9 | 36 (29–48) |
| Cut-over | X | 8 (6–11) |
| After cut-over | W 9 (T dropped, O2) | 9 (7–12) |
| **Total effort** | | **211 engineer-days (164–274)** |

Not in this total: the app plan's own remaining work (it has its own 171-day plan, much of it already built), and the
kafka and aws engines' own plans.

### 6.2 Critical path

```
C0 (5) → H1 (21) → H2 (16) → H3 (10) → H4 (15) → C2 (20) → X (8)        = 95 days
     └ engine: E1 16 → F1 7 → E2 10 → D1 12 → D2 10, done day 60; C2 needs it from day 67: slack ~7
     └ modules: C1 9 → M1 20 → M3 7 → P1 7, done day 48; X needs it at day 87: slack ~39
```

The engine track's slack is small: if E1 (supervision on three OSes) overruns by more than a week, the engine becomes
the critical path.

**About 95 working days to cut-over (75–120)** with three builders. HCL is the critical path. Two ways to shorten it:
- Split H2's function library (8 of its 16 days) to a second builder once H1's value types exist (about day 10). That
  saves about 6 days. Recommended.
- Start C2's daemon-only commands (`up`, `down`, `env`, `run`, `url`, `connect`) on stacks built in code while H4 runs.
  That saves about 6 more.

With both: **about 83 days to cut-over (66–106)**. Windows (W, 9 days) is "after cut-over" in what it
gates, but builder E is free from day 60 and can do it in parallel; M2 fits on builder M after day 48 once `pgext` is
merged. So the **whole finish lands within about a week of cut-over: about 90 to 100 days of calendar (75–125)** with
three builders, or 211 days (164–274) for one.

### 6.3 Weight budgets (the ceiling is 10 MB per binary)

| Binary | Budget (CI line under the ceiling) | Why it is at risk |
|---|---|---|
| `doze` (CLI + daemon + netd; the app binary carries the same on top of the GUI, O1) | **9 MB** line, 10 MB ceiling (macOS arm64, stripped) | it links tokio, hyper (front door), rustls + ureq (fetch), ed25519, hcl-edit/winnow, astro-float, regex, x509-cert/p256 and tun-rs. The C0 weight probe measures this first. Levers if it is over: `opt-level = "s"` + fat LTO (`release-app` showed −20%), one HTTP stack (hyper) for both fetch and front door instead of ureq + hyper, `regex-lite` where the full engine is not needed |
| `doze-app` | the app plan's ≤ 16 MB (app plan Q5; it has its own line) | not changed here |
| each module | the modules plan's lines | |
| daemon idle RSS | **≤ 8 MB** with nothing awake (to confirm against the Go baseline taken in C0) | tokio threads; keep it current-thread unless measured otherwise |
| `status`, `env`, `url` warm | **≤ 15 ms** (Go: 27–54 ms) | CLI proposal rule 5 |
| wake latency | **at or under Go's** `test/bench` budget | |

### 6.4 Risks

| Risk | Why | Mitigation |
|---|---|---|
| **HCL semantics** | no Rust crate has go-cty: conversions, 49 functions, unification, 512-bit numbers, formatter byte-identity (the spike: 0/177 formatted identically, 25 of 253 expressions differ in hcl-rs, `5 % 0` panics); hcl-edit has one maintainer | differential tests on every expression and function against go-cty from the first day (H1); vendor hcl-edit as kafka-protocol was; never evaluate with hcl-rs; the dump oracle makes every difference visible. If H1 runs long, the range is 35–60 for the language alone |
| **Process supervision on three OSes** | Go leaks module processes today (PPID 1 after `down`, `doctor`, `status`, app quit); macOS has no PDEATHSIG; Windows has no signals, needs Job objects and a non-elevated token | `doze-os::Tree` already on five targets; "nothing left behind" is a gate in E1 and at cut-over; macOS: modules die on control-socket EOF (protocol 2), engines are swept from recorded pids at daemon start |
| **Windows** | Go never ran there: no reference for the daemon, proxy, names, process engine; unix sockets for backends do not exist (D13: TCP backends); several engines have no Windows build | five-target CI from C0; W after cut-over with its own gate; compatibility sets per module (D12) say plainly what does not ship |
| **Weight** | `doze` is the fattest binary yet (section 6.3) | C0 weight probe; CI line from C0; levers listed; decision O1 keeps the app out |
| **Wire fidelity with the app** | the app was built against Go's frames | the frame scripts captured from Go become the server's tests; the 286-row checklist before cut-over |
| **Config edge cases** | 107 Go config tests are strings + `strings.Contains`, no goldens | the dump oracle turns them into goldens on both sides (C0) |
| **Owner-only steps** | root on macOS, the registry DNS (F1: `registry.doze-dev.com` still has no DNS answer, checked 2026-11-14), aws engine readiness (`pgext` was merged and pushed 2026-11-14) | listed in section 5 and O7/O8, with dates; nothing else waits on them |
| **Many open PRs** | 25 open PRs, stacked and conflicting (app, names, CLI) | C0 lands them in order before the core builds on them |
| **gpui-pre churn, aws port readiness** | other plans' risks that M3 and cut-over depend on | M3 aws face waits on doze-aws being bootable; cut-over gate lists Larder |

---

## 7. Prior art

| Piece | Prior art | What we take |
|---|---|---|
| **Process supervision** | **systemd** (`Restart=`, `KillMode=control-group`, socket activation, `sd_notify` readiness), **s6** / **runit** (readiness fd, supervision trees, finish scripts), **launchd** (on-demand jobs), **supervisord**, **process-compose** (the closest analogue: dependency conditions, readiness probes, restart policies, one YAML), **overmind**/**foreman** (Procfile, process groups), **Docker Compose** (`depends_on: condition: service_healthy`), Kubernetes probes, **watchexec** and its **process-wrap** crate (process groups on Unix, Job objects on Windows), **pueue** (a Rust daemon with a socket client) | the lifecycle as a state machine with a restart budget (systemd), readiness as a typed probe (k8s), process tree as the unit (s6, Job objects), declared stop per process (D15). process-wrap is evaluated against `doze-os::tree` in E1; doze-os stays if it already covers it |
| **Lazy wake / scale to zero** | **systemd socket activation** and **inetd**, **launchd** `Sockets`, **Fly.io** machines autostop/autostart proxy, **Knative** activator, **OrbStack**'s on-demand machines | accept first, then wake, then splice; the listener never closes while asleep (socket activation); the activator's "hold the request while cold" |
| **HCL** | **hashicorp/hcl v2** + **go-cty** (the reference), **hclwrite** (formatter), Terraform's `typeexpr`/`convert`, **OpenTofu**, **hcl-edit** / **hcl-rs** (Rust syntax; the evaluator is not usable), **tree-sitter-hcl**, **ensan** (stale), **magma-cty** (types only), the Terraform language server (diagnostics) | hcl-edit for syntax and spans; cty's semantics ported by hand on astro-float; Terraform's diagnostic shapes; nothing else is mature enough (spike) |
| **CLI frameworks** | **clap** (derive; already used in doze-rs), **bpaf**/**argh**/**lexopt** (lighter), **clap_complete** dynamic completion (`CompleteEnv`), **annotate-snippets** (rustc's renderer) / **miette** / **ariadne** (diagnostics), **indicatif** (progress), **anstream**/**owo-colors** (colour that respects NO_COLOR), **ratatui** (TUI), and for UX: **cargo**/**rustc** errors, **clig.dev**, **gh** (JSON per command), **fly launch** (detect and say why), **mise**/**bun** (instant), **Terraform** plan (dry runs object by object) | clap stays (already paid for); dynamic completions via clap_complete; diagnostics stay PR #10's own `snippet.rs` unless annotate-snippets is smaller and as good |
| **Daemon control** | Docker's API on a unix socket, **LSP** (JSON-RPC with ids and cancel), **Nushell**'s plugin protocol, tokio-util `LinesCodec`, Windows named pipes (`tokio::net::windows::named_pipe`) | Go's protocol 2 is already this shape; kept |
| **Fetch, signing, locks** | **cargo** index + `Cargo.lock`, Go's `go.sum`, **Homebrew** bottles, **minisign** (ed25519), **TUF** (not taken: too heavy for one signing key) | the signed index with a compiled-in key, checksums pinned in `doze.lock` |
| **File watching** | **notify** crate, **watchexec**, Go's polling `watch` | keep polling (Go's measured choice: it works on every filesystem and costs nothing at rest); notify only if a measurement asks |

---

## 8. Owner decisions

Decided by the owner on 2026-11-14: start building with three builders; O1 and O2 as written in the last column; O5 and O7
yes; O3, O4, O6 and O8 to O12 as recommended.

| # | Decision | Recommendation | Decided 2026-11-14 |
|---|---|---|---|
| **O1** | One binary or two: does the app join `doze`? (app plan Q2) | **Two**: `doze` (CLI + daemon + netd + dash) under 10 MB, `doze-app` beside it in `Doze.app`. Joining breaks the ceiling (the app alone is 12–16 MB) and slows every CLI call | **Two binaries, shaped differently**: `doze` is CLI-only, for CI and servers; the Doze.app binary contains both the GUI and the full CLI. The CLI is a library (`doze-cli`) linked into both (section 2.3) |
| **O2** | The terminal dash: port it (ratatui, ~18 days, after cut-over) or drop bare `doze` over SSH in favour of `status -w`? | **Port it, after cut-over.** The website and the CLI proposal promise it, and it is the only screen over SSH. If time is short, it is the first thing to defer | **Drop the terminal dash entirely**; phase T removed. Bare `doze` over SSH is `status` |
| **O3** | Compatibility at cut-over: protocol 2 byte for byte, and Go's on-disk formats | **Yes to both.** The app switches with no change, data survives, and Go/Rust run side by side for the oracle. Contract 2's state renames become protocol 3, done once in one commit across daemon, app and CLI after cut-over | as recommended |
| **O4** | The embeddable Go API (`doze.Serve`/`Attach`, `NewStack`) | **Drop it.** The Rust equivalents are `doze_engine::Engine::builder()` and `doze_control::Client`; no public library promise before 1.0 | as recommended |
| **O5** | A `rust-oracle` branch in the Go doze repo with test-only hooks (`DOZE_BIN`, `DOZE_MODULE_BIN_DIR`, `tools/oracle-dump`) | **Yes**, as kafka did (`rust-oracle` in doze-kafka). It never ships; it is how the Go suites judge the Rust binary | **Yes**, test-only hooks allowed. Pushed 2026-11-14 as doze `rust-oracle` |
| **O6** | Diagnostic wording: match Go's text, or re-baseline to the CLI proposal's rustc-style messages? | **Re-baseline the wording; keep positions exact.** Line, column and the offending span must match Go; the words follow the proposal's rules (whose problem, a fix) | as recommended |
| **O7** | Merge and push `pgext` (11 commits, +2,577 lines) in Go before the postgres port (modules plan D9) | **Yes, now.** The oracle must be the code that ships; M2 is blocked without it | **Yes, now.** Done 2026-11-14: rebased on main, vet and `go test -race` green, doze-modules main `a8500e0` |
| **O8** | The registry hostname: `registry.doze-dev.com` still has no DNS answer (CLI proposal F1, checked again 2026-11-14) | **Fix the DNS now** (a Cloudflare record), or every fresh Go install stays broken; the Rust fetch tests use a local registry until then | as recommended |
| **O9** | Module scope at cut-over | **postgres, mariadb, ferret, valkey, kvrocks, temporal, kafka, aws** (the registry's eight). Mailpit, minio, nats and friends stay `service` recipes in `doze-detect`; real modules for them are a later plan | as recommended |
| **O10** | Windows at cut-over or after? | **After** (phase W, right after). macOS and Linux are what Go supports today; Windows has no oracle and gets its own gate | as recommended |
| **O11** | `doze fmt` byte-identical to Go's `hclwrite.Format`? | **Yes.** People run `fmt --check` in CI; a different formatter would fail their builds on upgrade | as recommended |
| **O12** | Three builders in parallel (L, E, M tracks) | **Yes.** One builder is about 211 days; three is about 95 to cut-over (83 with the HCL split) | as recommended |

---

## 9. Files

### 9.1 New in doze-rs

```
docs/port/core/
  PLAN.md  plan.html                       this plan
  behaviours/<area>.md                     one catalogue per phase (lifecycle, process, daemon, proxy, config, …)
crates/doze-hcl/src/
  lib.rs  syntax.rs (hcl-edit adapter, spans)
  value.rs  value/{types,ordering,json}.rs  number.rs (astro-float, shortest printer)
  convert.rs  unify.rs
  eval.rs  eval/{template,for_expr,splat,ops,conditional,traversal}.rs
  funcs.rs  funcs/{string,collection,numeric,encoding,regex,format,convert,env}.rs
  typeexpr.rs  merge.rs  decode.rs  diag.rs  fmt.rs  edit.rs
  fuzz/  tests/{corpus,oracle}.rs
vendor/hcl-edit/                           pinned, PATCHES.md
crates/doze-config/src/
  lib.rs  schema.rs (the one table)  load.rs (files, merge, overrides)  kinds.rs  vars.rs  instances.rs
  foreach.rs  refs.rs (after, references, cycles)  modules.rs (declared schemas)  runspec.rs  endpoints.rs
  domains.rs  network.rs  paths.rs  shape.rs  lint.rs  fingerprint.rs  stack.rs (the typed output)
crates/doze-engine/src/
  lib.rs  engine.rs (builder, Engine handle)  stack_actor.rs
  lifecycle.rs  lifecycle/{machine,events,effects}.rs  graph.rs  holders.rs  reaper.rs
  exec.rs (plan executor, from doze-module-test::run)  ready.rs  supervise.rs
  process.rs  process/{runline,restart,health}.rs  steps.rs
  tasks.rs  tasks/inputs.rs  watch.rs  logstore.rs  applied.rs  sync.rs  host.rs (module pool + fetch service)
crates/doze-daemon/src/
  lib.rs  run.rs  server.rs (Handler impl)  ops/{status,verbs,logs,subscribe,add_remove,resources,admin}.rs
  topics/{instances,stats,journal,logs,console}.rs  proxy.rs  proxy/{splice,wire,tls}.rs
  names.rs (doze-bind)  meters.rs  ctl.rs
crates/doze-fetch/src/
  lib.rs  index.rs  signed.rs  select.rs  meta.rs  binaries.rs  modules.rs  cache.rs  lock.rs (read + write)  child.rs
crates/doze-detect/src/   lib.rs  go.rs  node.rs  python.rs  compose.rs  make.rs  render.rs  known.rs
crates/doze-doctor/src/   lib.rs  checks/{names,domains,registry,app,versions}.rs  leftovers.rs  repair.rs
crates/doze-module-mariadb/  doze-module-ferret/  doze-module-postgres/     (modules plan steps 9–13)
scripts/core-privileged-e2e-darwin.sh                                      (P1, owner-run)
```

### 9.2 Changed in doze-rs

- `crates/doze-control`: `server` feature (`server.rs`, `handler.rs`); `fault.rs` shared.
- `crates/doze-cli`: every `cmd/*.rs` stub becomes a command; `main.rs` gains the self-exec dispatch; `registry/` moves to
  `doze-fetch`; `render/` (tty, plain, json); `notice.rs`; `complete.rs`; `tests/transcripts/`.
- `crates/doze-journal`, `crates/doze-projects`: writers beside the readers.
- `crates/doze-os`: `ports.rs` (who holds a port), Windows named-pipe listener, the macOS pid sweep.
- `crates/doze-module-test`: uses `doze-engine`'s executor (one copy).
- `crates/doze-kafka`, `crates/doze-aws`: module mode (M3).
- `.github/workflows/ci.yml`: `core` job on five targets; `integration` job (Go oracle suites against the Rust binary,
  macOS and Linux nightly); weight lines for `doze`.
- `xtask`: `weight` gains `doze`; `core-oracle` (runs the dump diff and the Go suites).

### 9.3 In the Go repos (oracle only, never shipped)

- doze `rust-oracle` branch: `test/integration/harness.go` (`DOZE_BIN`, `DOZE_MODULE_BIN_DIR`), `test/bench`
  (`DOZE_BIN`), `website/scripts/ground.ts` (already reads `DOZE_BIN`), `tools/oracle-dump/` (parse, syntax, refs, fmt, edit, eval, config
  with diagnostics and positions), `internal/control/fixture` capture (frames). Pushed 2026-11-14.
- doze-modules: `pgext` merged and pushed (O7).

### 9.4 Read as the spec

doze: `cmd/doze/*`, `internal/{config,runspec,runtime,registry,supervisor,health,daemon,control,proxy,netplan,endpoints,state,logstore,journal,projects,watch,inputs,modules,binaries,detect,actions,fault,portholder}`,
`engine/process`, `docs/design/next/{daemon-api,lifecycle,networking,module-names,config}.md`,
`website/src/content/docs/{reference/cli.md,hcl/*}`; doze-names (whole); doze-modules `modules/{mariadb,ferret,postgres}`
(at `pgext`); doze-rs `docs/port/{modules/PLAN.md,modules/compat.md,names/PLAN.md,oracle.md,weight.md}`, `cli-design`
`docs/port/cli/proposal.md`, `app-plan` `docs/port/app/PLAN.md`.

---

## 10. Log: track L (builder A)

- **2026-11-14, C0 (rails), done.** doze `rust-oracle` pushed: `DOZE_BIN` (test/integration, test/bench),
  `DOZE_MODULE_BIN_DIR`, `tools/oracle-dump` (parse, syntax, refs, fmt, edit, eval, convert, config with
  diagnostics and positions) and the control-frame capture; the process-engine integration tests and
  `ground.ts` pass against Go through the hooks. pgext merged into doze-modules main (`a8500e0`, vet and
  `go test -race` green). Go baselines and the weight probe: `weight.md`. Landing the open PR stacks is the
  coordinator's.
- **2026-11-14, H1 (doze-hcl values and evaluator), done**, PR `core-h1`. Gate: spike corpus 637/653
  identical (the rest are H2: 7 function calls, 9 syntax-error positions), 190/190 sources accept/refuse
  as Go, fuzzed. An independent audit added 869 oracle cases and found real gaps (cty refinements,
  multiplication precision, number-literal grammar, Go's extra syntax rules, two quadratic paths); all
  fixed, three reviewed differences kept and documented (`crates/doze-hcl/testdata/eval/known-differences.json`).
  Open: hcl-edit's slow path on unclosed nesting (upstream; for H2). Counted as H1's planned scope
  (21 days); next is H2 (functions, typeexpr, diagnostics, syntax-error mapping).
- **2026-11-14, H2 (doze-hcl functions, typeexpr, diagnostics, syntax errors), done**, PR #43 (`core-h2`, on #40).
  The 49 functions agree with Go on 2,993 new cases plus 1,879 from the audit, with 5 reviewed differences.
  A hand port of Go's scanner, and of the parser's diagnostic control flow, gives every syntax diagnostic
  Go gives, in Go's order and range: 5,827/5,827 sources, about 8,300 diagnostics. That closes the 27 open
  syntax positions, and S6 (hcl-edit no longer sees broken nesting).
  `typeexpr` matches on 112 cases. The independent audit found no blocker; its fixes are in, including
  Go's Unicode 9.0 identifier table and full diagnostic ranges.
- **2026-11-14, H3 (fmt, editor, decode), done**, PR #44 (`core-h3`, on #43). `fmt` is byte-identical to
  hclwrite on 177/177 sources and on 6,239 random, broken and audit inputs, so O11 is met. The editor
  (`doze add`/`init`) is identical on 2,273 cases, 6 of them reviewed. Content, PartialContent,
  JustAttributes and MergeFiles match on 1,724 reads. Audited, and the fixes are in.
- **2026-11-14, H4 (doze-config), done**, PR #47 (`core-h4`, on #44); the engine wiring is draft PR #48
  (`core-h4-wire`, on #47, merges #32).
  - The loader is the whole of Go's `internal/config`, plus the command engine's decode and `runspec`.
    That covers files, overrides, variables, locals, defaults, count and for_each, references and
    `after`, cycles, outputs, lint, fingerprints and the shape pass.
  - Modules come only through a `Modules` trait, so the port table and the temporal exception are now
    test fixtures. The machine comes through `Host`.
  - Oracle: 257/257 in both modes, with diagnostics compared as sets, contexts included. A new multi-file
    corpus (30 stacks from the live Go oracle: overrides, siblings, auto vars, scopes, each audit
    finding) is 30/30 in both modes, with one listed difference.
  - Fuzzed: `fuzz_load` is in CI. It found an underflow in doze-hcl's diagnostic render, now fixed.
  - The independent audit had 15 findings, 13 confirmed against Go. All are fixed except two, which are
    kept and tested in `tests/differences.rs`: an overflowing `log_size` is refused, and the shape
    pass's default port comes from the module cache only.
  - Fixing the audit made deep chains linear: 20,000 locals load in 65 ms, where they used to overflow
    the stack. doze-hcl now steps into a variable by reference.
  - Wiring: `to_stack` builds `doze_engine::Stack`. `ca_env` supplies `extra_env`. The larder test runs
    larder's doze.hcl in an engine under a temporary home and wakes a probe app through Redis.
  - Open:
    - Tasks and `download` wait for E2.
    - Route names in `validateNetworks` need module listener declarations.
    - `psl` costs 0.84 MB (weight.md); that is an owner call.
  - Days: planned 15 (11–19); done in one session. Next on track L: C2.
- **2026-11-14, H4 follow-up: public suffix list owned.** `psl` (+0.84 MB) replaced by doze-config's own
  front-coded copy (78 KB, publicsuffix.org 2026-11-13, commit 71c235dd), decoded on the first `domains` check:
  +0.08 MB. Go's matching exactly: psl kept as a dev-dependency oracle (every rule and 300,000 random names; the one
  psl-vs-Go difference, a parent-only TLD, pinned to Go); built from Go's list revision, 131,275/131,275 names
  identical to Go. CI on #47/#48 was red for test-side reasons (serde_json `preserve_order` in workspace builds,
  Windows paths in the Go oracle); fixed in core-h3, core-h4 and core-h4-wire. The `weight*` failures are kafka's
  lines, inherited from main (#45).
- **2026-11-14, C2a, done (first slice of C2)**, PR #51 (`core-c2a`, on #41, merges #47). The CLI reads
  doze.hcl's shape offline: `status` with the daemon down (Go's golden byte for byte), name checks before
  "not running" for every verb, O6 diagnostics at Go's line and column, `lint` (module pass pending D1, exit 11),
  `fmt` (`--check`, `--dry-run` diff), `-c <dir>`, `--var`, dynamic completions in four shells, help examples.
  Independent audit: 14 findings, all fixed or documented. `doze` 6.2 MB (weight.md). About 4 of C2's 20 days.
  Next: `init`/`add` (module declarations from the registry's cached meta), then the daemon verbs (`up`, `down`,
  `env`, `url`, `run`, `connect`) as D1 lands, `modules upgrade` with F1's lock writer, `bug`, `open`.
- **2026-11-14, plan deviation (engine track; decided by the coordinator for the owner): the engine does not link
  doze-fetch.** §3 put "the module pool with the real fetch service" in E2. Instead `doze-engine` takes any
  `Downloads` and the `doze.lock` pins as values, and D1 (the daemon) wires F1's `ChildDownloads` and loads the
  lock. Reason: the engine stays pure (no network, no registry, no lock file I/O) and lighter, and its tests keep
  using in-memory downloads. No days move: the wiring is a small part of D1's 12.
