# doze modules in Rust: the SDK and every module, before the core

Status: proposed 2026-11-12, not approved. Owner's brief: "Doze-modules and doze-sdk is what needs to move to
rust before the core", no Go/Rust stitching, no relic of the past. doze-binaries (build tooling), doze-names
(reference only, to be deleted), doze-examples and doze-website stay out of scope.

Grounding: three read-only audits on 2026-11-12 (doze-sdk support packages; the six modules on branch `pgext`;
what the Go core demands of a module, at doze `23b73d6`, SDK v0.12.0), my own read of `doze-sdk/engine` and
`plugin/server.go`, and the go-plugin host spike (`~/Code/doze-dev/doze-plugin-spike/REPORT.md`).

## What exists today (facts the design rests on)

- **The contract is wide and partly dead.** 23 RPCs (`plugin/proto/engine.proto`). The core never calls
  `Hooked`/`Hook`, `PortBinder`/`AdvertisedAddr`, `LegacySpawner`, `ErrorWriter`, the in-process
  `ProxyFilter`; `Instance.Port` is a hard-coded 5432 for modules; Inventory/Prune ride on the `converger`
  capability; Console is only reachable when Admin is also advertised (adapter accident).
- **Modules decode their own HCL.** `DecodeConfig` ships the whole source file, re-parses it in the module,
  strips core's arguments, rebuilds a cty eval context (variables only, no functions) and runs gohcl
  (`plugin/server.go:183-226`). The result is an opaque gob blob the core stores and sends back on every call.
- **The core already owns everything that runs.** It executes the module's `SpawnPlan` (order, readiness
  probes `http|tcp|socket|exec|log_line`, hooks, process groups, logs, pidfiles), owns ports, names, grants,
  sleep and wake, the module/engine fetch, `doze.lock`. A module describes; it never spawns.
- **kafka and aws already have the shape we want:** their module binary *is* the engine (Plan re-execs
  itself with `__serve`). So "the engine speaks the protocol itself" is not new; it is what they do.
- **One module process per type, warm for the daemon's life**, relaunched if it dies. Postgres's wire filter
  means every postgres client byte flows through the module process.
- **Modules never download.** `Resolve` gets a host `Fetcher` (ResolveMajor, Ensure) for the length of the
  call; mirror, cache, checksum and lock are core's.
- **Registry contract is language-neutral:** schema-1 `index.yaml` (with `protocol`, `engines`, `doze`),
  `meta.yaml` from `Describe()`, ed25519 signatures done in JS by doze-registry. Go doze selects only
  releases whose `protocol` equals its own (1).
- **Size:** SDK ~6 K Go lines plus 5.5 K generated proto; modules: postgres 3.3 K (+2.6 K on `pgext`, not
  merged, not pushed), ferret 1.0 K, mariadb 0.6 K, temporal 0.45 K, kvrocks 0.24 K, valkey 0.21 K;
  kafka module ~0.8 K, aws module ~1.25 K (in their own repos).
- **Platforms (Go today):** Apple Silicon macOS, x86-64 and arm64 Linux (`engine/platform.go`). No Windows,
  no Intel Mac. The Rust plan adds Windows: see Platforms.

## Prior art

- **Terraform's plugin protocol (tfplugin6):** the provider *declares a schema*; core parses and evaluates HCL
  and sends the provider typed values. Providers never see HCL. This is the model for D1.
- **Nushell's plugin protocol** (Rust): a versioned hello, then framed messages over an inherited pipe,
  bidirectional calls (plugin calls back into the engine), streams. Model for D3.
- **Cargo's credential-provider protocol:** versioned hello with a capability list, tiny surface. Model for
  keeping the hello the place where capabilities are fixed.
- **systemd socket activation:** pass ready file descriptors to a child instead of paths. Model for the
  inherited control socket and the client-fd handoff.

## Decisions (proposed; ⚑ = needs the owner)

**D1. Modules declare a schema; the core evaluates.** A module's config type derives its schema
(`#[derive(Spec)]`): attributes, nested and labelled blocks, types, defaults, docs, version bounds
(`since`/`until`, what `RequireVersion` does today), uniqueness of labels. The core parses and evaluates the
block (variables, functions, for_each, everything) and validates it against the schema (unknown argument,
wrong type, missing required, version gate, duplicate label), then sends the module a typed `Value` tree in
which every node carries its source span. The module's `decode` does only semantic checks a schema cannot say
(grant needs exactly one of database|schema; kvrocks namespaces need a password), and reports them against a
value path, which the core maps back to a span and renders like any other diagnostic.
Why: modules never link HCL or cty (the 45-day port happens once, in core); one schema drives validation,
diagnostics, `meta.yaml` and the registry docs, so they cannot drift; no opaque gob. Before the Rust core
exists, the SDK's harness evaluates literal-only HCL with `hcl-edit` (enough for tests and the published
Examples).

**D2. Stateless calls.** Every call carries the instance and its validated `Value` spec; the module decodes it
(microseconds) and holds no per-config state between calls. This removes the postgres `declareWanted`
process-global (bug 2 below) by construction: Resolve computes the wanted bundles from the spec it is given.

**D3. A Rust-native protocol, v2.** No go-plugin, gRPC, protobuf, gob or TLS.
- Transport: the host creates a `socketpair`, the child inherits one end as fd 3. No path, no handshake line,
  no cookie. When the host dies the socket closes and the module exits: **no orphaned modules**, so no pidfile
  sweep for module processes.
- Framing: length-prefixed frames, multiplexed by request id, in both directions (host→module calls;
  module→host `fetch.*` and `log`), plus stream frames (`Tail`) and cancellation.
- Encoding: CBOR with numbered fields (`minicbor` 2.x derive; unknown fields skipped, new fields optional), so
  the wire evolves additively like protobuf without codegen or a `.proto`. **Chosen in S0:** postcard fails every
  version-skew case; prost evolves as well but forces protobuf-shaped types.
- Wire handoff: the client fd travels on the *same* socket with SCM_RIGHTS (rustix, no `unsafe`), attached to
  a `wire.accept` frame; `boot`/`backend`/`splice`/`release` become frames on the multiplexed channel instead
  of a second line protocol.
- Version: `hello { protocol: 2, module, version, capabilities, manifest }`. The registry index gets
  `protocol: 2`, so the Go doze (which selects only `protocol == 1`) never picks a Rust module: **Rust
  modules can be published to the live registry before the Rust core ships without breaking anyone.**

**D4. A small, typed contract.** Static facts move into a `Manifest` sent once in `hello` (and printed by
`<module> --describe` for the registry): type, category, versions and exactness rule, default port, boot
budget, backend socket name template, supervised or lazy, versionless, console layout, admin actions, schema,
example. Per-instance calls:

| call | required | today's equivalent |
|---|---|---|
| `decode(spec, version) -> Ok \| diagnostics` | yes | DecodeConfig |
| `network(inst) -> listeners` | yes (default: one tcp `main`) | Network |
| `connection(inst, endpoint) -> (env var, url)` | yes | ConnString |
| `attributes(inst, endpoint) -> Value map` | optional | Attributes |
| `backend_url(inst)` | optional | BackendURL |
| `resolve(inst, platform, pins) -> toolchain + new pins`, may call host `fetch.*` | yes | Resolve |
| `provision(inst, toolchain) -> Fresh \| Existing` | yes | Provision + Provisioned |
| `plan(inst, toolchain) -> SpawnPlan` | yes | Plan |
| `template.ensure` / `template.clone` | optional | Templater |
| `converge`, `objects`, `prune` | optional, separate capabilities | Converger / Inventory / Pruner |
| `restart_policy(inst)`, `health(inst)` | optional (supervised modules) | Restartable / HealthChecker |
| `resources`, `run_action` | optional | Admin |
| `tail(inst, panel, selector) -> stream` | optional, independent of admin | Console |
| `wire.accept(fd, opts)` | optional | WireProxy |

Dropped: `Type`, `Capabilities`, `Versionless`, `Supervised`, `SlowBooter`, `DescribeConsole`, `Actions`,
`BackendSocket` (all static → Manifest), `Hooked`, `PortBinder`, `LegacySpawner`, `ErrorWriter`, in-process
`ProxyFilter`, `Instance.Port`.
In the SDK each optional call is its own trait (`Converge`, `Admin`, `Console`, `Wire`, …); the module
registers the ones it implements with a builder, so the advertised capability set is derived from the types
and cannot disagree with what the binary does.

**D5. The SDK ships the host side too.** `doze-module-host` (launch, supervise, call, serve `fetch.*`,
hand off fds) is built now because the test harness needs it, and the Rust core later uses the same crate.
The harness also contains a small SpawnPlan executor (order, readiness probes, hooks, process groups,
stop escalation); it is written as the core's future executor, not a throwaway.

**D6. Clients inside modules stay the engine's own CLIs** where they are today (psql, mariadb, temporal):
no extra weight, the toolchain already ships them, and behaviour matches. Ferret is the exception: the Go
module links the official mongo driver. In Rust, a minimal OP_MSG client on the `bson` crate (createUser,
createCollection, createIndexes, insert, drop, count) over the unix socket, measured against the `mongodb`
crate's size before deciding. ⚑ only if the measurement surprises.

**D7. Fix the audited bugs instead of porting them** (list below). Where a fix changes user-visible
behaviour it is listed as ⚑.

**D8. Where the code lives: decided 2026-11-12, build in doze-rs.** Original proposal: the SDK crates (`doze-module`, `doze-module-host`,
`doze-module-proto`, `doze-module-test`) in this workspace (`doze-rs`), next to kafka (and later core and
aws), so the protocol can change in one commit while it is fluid. The six modules: either also here
(`crates/modules/*`), or `doze-modules` becomes a Rust workspace depending on the SDK by git tag. Here is
simpler until protocol 2 is frozen; `doze-modules` keeps the contributor flow the docs promise. My pick:
build everything here, move the modules to `doze-modules` when protocol 2 freezes.

**D9. ⚑ Postgres `pgext` lands first.** The Rust postgres module targets `pgext` behaviour (catalog, bundles,
ledger, PG18 paths). Proposal: you merge and push `pgext` in Go (it is 8 unpushed commits) so the oracle
is the code that ships.

## Platforms (owner decisions 2026-11-12)

- **doze itself runs natively on macOS, Linux and Windows.** No WSL. Targets: `aarch64-apple-darwin`,
  `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`, `aarch64-pc-windows-msvc`.
  (Intel Mac stays out.)
- **Every module carries an explicit compatibility set, and ships only where it works.** What has no Windows
  engine (valkey, kvrocks, ferret today) does not ship for Windows; more come incrementally.
- **Users bring their own engine where no module exists**, with the built-in `process` engine and the
  command they run (e.g. a Redis-compatible server on Windows).
- **Run lines are portable**: one `doze.hcl` works on all three systems.

How the plan meets them:

**D10. Three platforms from the first commit.** CI runs every crate of this plan on all five targets from
step P1 on (the workspace's `ci.yml` already has a Windows compile-only job; it becomes a test job). A step is
done when it is green on all five, not "on Mac, Windows later". Local loop: the Mac, the Lima VM for Linux,
GitHub Actions for Windows (no Windows machine here).

**D11. One platform crate holds every OS difference** (`doze-os`): the module control channel (Unix:
socketpair as stdin; Windows: inherited pipe), handing a client connection to a module (Unix: SCM_RIGHTS;
Windows: `WSADuplicateSocketW`), process trees and kill-on-host-death (Unix: process group plus control-socket
EOF; Windows: a Job object with kill-on-close), graceful stop (Unix: signals; Windows: console break, or the
module's declared stop command), file locks, owner-only directories (0700 vs ACL), data-dir cloning (clonefile
/ reflink / plain copy), daemon detach, executable lookup (`PATH` + `PATHEXT`). Nothing outside it has
`#[cfg(unix)]` or `#[cfg(windows)]`. ⚑ The Windows half needs Win32 calls that are `unsafe` (socket
duplication, Job objects); the workspace forbids `unsafe`. Proposal: `doze-os` alone may use it, each block
with a `// SAFETY:` line, wrapped in safe functions, and covered by the W0 tests; prefer a maintained safe
wrapper crate wherever one exists. **Approved by the owner 2026-11-12.**

**D12. Compatibility is data, proven by tests.** The module manifest lists the targets the module supports;
the release tool builds and publishes an artifact for a target **only if the module's acceptance tests passed
on that target in CI**, against real engine binaries for that target. So the registry index can only offer
what was proven. The core then says plainly what is missing: "valkey has no Windows build. Run your own
Redis-compatible server with a `process` block: <docs link>". Engine availability comes from doze-binaries'
per-target index (Windows engine builds there are Go work in doze-binaries, outside this plan).

**D13. Backends are addresses, not socket paths.** A grant's backend is `unix:<path>` or `tcp:127.0.0.1:<port>`
(the core assigns the port). Modules emit whichever the target supports; postgres, mariadb and others use
loopback TCP on Windows. Readiness probes follow (`socket` → `tcp`).

**D14. No shell in plans or run lines.** SpawnSpec hooks become argument lists, not `sh -c` strings
(ferret's `CREATE EXTENSION` hook becomes a psql argv). For the `process` engine (core work, recorded here
because it fixes the contract): `run` is an argument list, or a string doze splits into words itself
(quotes, no pipes, redirection or `&&`); doze expands `${port}`-style references, finds the executable via
PATH/PATHEXT, and never calls a shell. Someone who needs a shell writes `run = ["sh", "-c", "…"]` and has chosen
a platform. Existing configs with shell syntax in `run` get a clear decode error with the rewrite.

**D15. A module declares how to stop each process** (`stop: signal(term) | command [argv] | break`), because
Windows has no SIGTERM. The core escalates to the hard kill (Job object / SIGKILL) after the grace period.

## Work, in order

Estimates are working days, review-gated, each step checked against its oracle. Spiked: S0. Each step is
done only when green on all five targets.

| # | step | days | done when |
|---|---|---|---|
| S0 ✓ | **Done 2026-11-12** (`spikes/s0-module-proto/REPORT.md`): design holds on macOS and Linux; minicbor chosen; fd 0 must be re-pointed at /dev/null; backpressure is step 1's job. **Protocol spike:** socketpair + fd 3, framing, minicbor vs postcard vs prost (size, speed, field evolution across two builds), multiplexed calls both ways, a stream, SCM_RIGHTS on the same socket, child exit on host death (macOS and Linux via the Lima VM) | 3 | numbers written; encoding chosen |
| W0 ✓ | **Done 2026-11-12** (`spikes/w0-windows/REPORT.md`, branch `w0-windows`): green on all five runners; engines need a non-elevated token on Windows (daemon re-launches itself restricted, proven in P1); Unix needs PDEATHSIG/reconciliation for engines. **Windows spike, on GitHub Actions (x64 and Arm):** S0's transport over an inherited pipe, module exit and engine-tree kill via a Job object when the host dies, a TCP client duplicated into the module with `WSADuplicateSocketW` and spliced to a backend, real postgres on Windows booted, probed over TCP and stopped gracefully, `PATHEXT` lookup; numbers next to S0's | 4 | report written; D11 confirmed or changed |
| P1 ✓ | **Done 2026-11-12** (`p1.md`, PR #1): 17 tests green on five targets. `doze-os`: the platform crate (D11) with its tests on all five targets; CI matrix turned on | 6 | green on five targets |
| 1 ✓ | **Built 2026-11-12** (`protocol.md`, PR #2). `doze-module-proto`: frames, messages, codec, version rules, per-stream flow control, compat test (old/new builds talk); transport over `doze-os` | 4 | compat test green on five targets |
| 2 ✓ | **Built 2026-11-12** (`sdk.md`). `doze-module`: `Module` trait + capability traits, builder, server loop (current-thread tokio), `Value` + `#[derive(Spec)]` (schema, decode, docs), helpers: resolve-via-pins, exact-version rules, template dir, settings pass-through with locked keys, CLI runner, stale socket/pid clearing | 6 | echo module passes the harness |
| 3 ✓ | **Built 2026-11-12**: launch, hello, typed calls, downloads and backend services, client hand-off with acquire/release, a pool that restarts the dead and stops the idle; SDK gains `Wire`. `doze-module-host`: launch, hello, calls, `fetch.*` service (fake fetcher in tests; real one is core's), wire handoff, restart on death | 4 | spike-level parity with the Go host on the echo module |
| 4 ✓ | **Built 2026-11-12**: literal HCL to values with spans, sandbox under /tmp (short socket paths), the core's boot sequence, a plan executor (order, readiness tcp/socket/http/exec/log line, hooks, declared stop); the echo module's published example boots on its real engine and stops gracefully. `doze-module-test`: literal HCL via hcl-edit into `Value`, schema validation, SpawnPlan executor with readiness probes, `boot`, `boot_example` (the published Example must converge), network purity check, sandboxed `DOZE_HOME` (never `~/.doze`) | 5 | valkey boots end to end |
| 5 ✓ | **Built 2026-11-12** (`tools/module-oracle/README.md`): 184 cases generated from the Go descriptions, Go's answers committed, `doze_module_test::oracle::compare` proved on the echo module; found two Go description bugs (ferret, postgres) and a kvrocks example only the core can evaluate. Go differential oracle: a Go program that imports each Go module's `Driver` in-process and dumps JSON (Network, ConnString, Attributes, Objects, Plan with argv/env/files, Describe) for a corpus of config blocks; the Rust side dumps the same through the harness; `bun` compares | 2 | corpus of ≥ 150 blocks across the six modules |
| 6 | Release tooling in Rust (`xtask`): build the module's supported targets, publish a target only if its acceptance passed there (D12), package `bin/<name>-plugin`, merge the schema-1 index with `protocol: 2`, `meta.yaml` from `--describe`, the immutability rules of `modtool`; CI workflow | 3 | a dry-run publish produces an index the registry's `publish.mjs` accepts |
| 7 | valkey, kvrocks (one RESP helper) | 2 | differential + real-binary acceptance |
| 8 | temporal | 2 | same |
| 9 | mariadb | 3 | same, plus a login as the declared user (the Go test never does) |
| 10 | ferret (two-process plan, port picking, template clone, OP_MSG client) | 5 | same |
| 11 | postgres core: schema, provision, plan, converge (roles two-pass, database, schemas, grants), inventory, prune, console/admin | 9 | same, plus the Example converging on 16, 17, 18 |
| 12 | postgres wire filter on `wire.accept` (SSLRequest/TLS, startup replay, cancel with synthetic keys, RequireTLS) | 3 | real psql through the filter, cancel works, byte-equal with Go on recorded sessions |
| 13 | postgres extensions: catalog, bundles via `fetch.*`, ledgered install 14-17, PG18 control/library paths, preload merge, CREATE/UPDATE at default_version | 9 | the `bundle_test` scenarios and the gated pgext e2e pass on 17 and 18 |
| 14 | kafka module face in this workspace (`doze-kafka` binary gains module mode: network, connection, topic converge, admin, console tail) | 3 | differential vs Go kafka module |
| 15 | aws module face (doze-rs-aws) | 4 | blocked on the aws port reaching a bootable stack |
| | **total** | **~78 (62 to 100)** | |

Steps S0 to 6 are the SDK and platform layer (~37 days: the three-platform work adds W0, P1, and about 6 days
spread over steps 1 to 6). Steps 7 to 13 are the six modules (~33, plus about 2 for Windows acceptance of
postgres, mariadb and temporal, once doze-binaries has Windows engine builds). kafka and aws faces (~7) depend
on their engines. Expected first compatibility sets: postgres, mariadb, temporal, kafka, aws on all five;
valkey, kvrocks, ferret on macOS and Linux only.
The core's own Windows work (daemon, names, local CA in the Windows store, named-pipe admin socket, the
portable `process` engine, the app's tray and notifications) belongs to the core plan, not this one.

## Bugs to fix rather than port (from the module audit)

1. ⚑ postgres `autovacuum`: docs say default true, code defaults to off. Pick one.
2. `declareWanted` process-global map in postgres extensions (fixed by D2).
3. Any executable in an extension bundle's `bin/` overrides a toolchain tool by name (could shadow `psql`).
   Only declared tools override.
4. ⚑ `source =` extension bundles: no checksum, no lock pin, no ledger, no lock. Require a `sha256` or pin it
   in `doze.lock` like every other download.
5. Non-deterministic choice of a bundle pin (map iteration). Deterministic.
6. ferret: `user` blocks ignored when no `database` is declared; seed JSON loses integer types; its private
   Postgres trusts every local TCP client. ⚑ The last one: listen on a unix socket only.
7. valkey/kvrocks: password not URL-escaped; valkey's `--requirepass` visible in `ps`. Escape; pass the
   password through a config file.
8. mariadb: privileges interpolated into SQL unvalidated; whether the anonymous account shadows `%` users is
   unverified. Validate against a whitelist; test a login.
9. temporal: Describe lists `["1"]` but the code and CI use "1.1"; any `describe` failure is read as "absent"
   and triggers `create`. Fix the label; only "not found" means absent.
10. ⚑ postgres prune: `DROP ROLE` without REASSIGN/DROP OWNED fails on owners; removed `member_of` is never
    revoked; removing a password never clears it. Converge to the declared state.
11. postgres settings: case-insensitive lookup vs case-sensitive override check. One rule.

## Things the core port inherits (not in this plan, recorded so they are not lost)

- The core reaches around the contract for the `process` engine (`NominalPort`, `Proxied`, `IdleAfter`,
  `BindPort`, `ForwardPort`, `RunLine`, runspec interfaces). `process` stays built into core; those become
  core's own types.
- Engine names hard-coded in core: `doze migrate` is postgres-only (runs initdb/pg_dumpall/psql itself), AWS
  fake credentials keyed on `AWS_ENDPOINT_URL`, `kind == "aws"` in `add`, event-engine lists in the app.
  Candidates for a `migrate` capability and manifest fields.
- `Release.doze` (minimum doze) is outside the signed index payload. Sign it in protocol 2's index.
- Port the binaries/lock code from **core's** `internal/binaries`, not the orphaned SDK copy.

## Not decided here

The kafka/aws engines' own plans stay in `docs/port/PLAN.md` and `docs/port/aws/PLAN.md`; this plan only adds
their module mode.
