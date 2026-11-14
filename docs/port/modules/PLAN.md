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

**D16. Go is the floor, not the bar** (owner, 2026-11-13: "make sure we build the modules better than what they are
today"). Each module step starts with a written "better than Go" list, the owner sees it, and every item becomes an
acceptance test on the real engine, not a claim. The oracle still holds a port to Go's answers, and every case that
differs must differ only by a listed improvement (`doze-module-valkey/tests/oracle.rs` is the pattern). Where to look:
what a user sees go wrong (valkey lost its keys at every idle reap), what the engine refuses only at boot or accepts
and gets wrong (refuse it at its line, with a way out, and prove the engine's own behaviour in a test), what the SDK
offers that Go left unused (console, admin, streams), and what the core hard-codes about the engine (now manifest
fields: `default_version`, `client`, `detect`).

**D17. Compatibility and telling people** (owner, 2026-11-13): [compat.md](compat.md). Nothing published breaks a
working stack; the minimum doze is computed and signed; newer parts degrade; people are told at three levels and never
nagged; updates never stop services; old major protocols are supported for 12 months.

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
| 6 ✓ | **Built 2026-11-12 as a dry run** (`crates/doze-module-release`, `tools/module-release/registry-check.mjs`): packages one built module for one target only with `--accepted` and only for a declared target; reproducible archive; merges the schema-1 index (protocol 2) without touching published Go releases; meta.yaml from `--describe`; the registry's own parser and publish checks accept the output. The upload workflow waits for the first real module and the owner's go. `MIN_DOZE` is a placeholder (0.3.0). Release tooling in Rust (`xtask`): build the module's supported targets, publish a target only if its acceptance passed there (D12), package `bin/<name>-plugin`, merge the schema-1 index with `protocol: 2`, `meta.yaml` from `--describe`, the immutability rules of `modtool`; CI workflow | 3 | a dry-run publish produces an index the registry's `publish.mjs` accepts |
| 7 ✓ | **Built 2026-11-13** (`crates/doze-module-valkey`, `crates/doze-module-kvrocks`, `crates/doze-resp`): **Then made better than Go (D16)**: valkey keeps its keys across sleep (`shutdown-on-sigterm save` unless `save` is set; `save = ""` keeps nothing); both refuse before boot what the engine refuses only at boot (policy, memory size, schedule; workers 0, a token equal to the password) or accepts and gets wrong (two namespaces sharing a token; owned settings), each proven against the engine; console numbers, live `MONITOR` stream, actions (empty, save now; empty a namespace, recount, compact); manifest `default_version`/`client`/`detect`; valkey 8 and 9 both accepted in CI. SDK gained `help::{one_of, dial_backend, dir_size}`, a transposition-aware did-you-mean, `doze_os::net`, `Booted::sleep_and_wake`. Before that, the first real Rust modules, macOS and Linux only. Oracle: kvrocks matches Go on all 17 cases (a deliberate break was caught on all 10 planned ones); valkey on 16 of 21, and the 5 that set a password are checked to differ only in where it goes (bug 7). Acceptance on valkey 9.1.1 and kvrocks 2.16.0: the published examples boot, need their passwords, apply their settings, kvrocks keeps namespaces apart, both stop gracefully. New CI `modules` job runs it per declared target with `DOZE_ACCEPTANCE=1` (a missing build fails, never skips). Shared pieces it added: `doze_os::fs::write_private`, `help::{empty_data_dir, prepare_backend, host_port, io_error}`, `doze_module_test::acceptance`. Both listen where the host grants (D13); conf-bound values with control characters are refused; kvrocks's example quotes its dotted key so it decodes without the core | 2 | differential + real-binary acceptance |
| 8 ✓ | **Built 2026-11-13** (`crates/doze-module-temporal`), better than Go (D16), each proven on temporal 1.8.2: listens where the host grants (new `Listener::tcp`: no socket mode), so two projects both run it and its Web UI gets `ui-<name>`; declared namespaces exist at ready (`start-dev --namespace`); converge changes only what differs, only "is not found" means absent (bug 9), `-n` not the deprecated positional; retention in days, search attributes (case as temporal reads it) and dynamic config checked at their line; ready = SERVING (exec readiness), Health, Restarts; console opens the Web UI (`{address:ui}`) and lists namespaces with running workflows; `version = 1` = newest 1.x (owner; doze-binaries `also_major_parts`); targets include Windows (doze-binaries PR #9 builds it). Oracle: every differing case differs only by the three listed improvements; the oracle now grants TCP listeners their conventional ports (`regrant`). SDK `help::object_hash` (protocol.md row 15). Windows acceptance waits for the published Windows builds. | 2 | same |
| 9 ✓ | **Built 2026-11-14** (`crates/doze-module-mariadb`, core plan M1), better than Go (D16), each proven on mariadb 11.4.13 and 12.2.2: privileges checked per level against MariaDB's lists with did-you-mean, character set, collation, database and table names checked, owned settings refused, all at their line (bug 8); converge keeps the declared state (grant, then revoke what is no longer declared, GRANT OPTION included; a removed password cleared; a changed character set applied) and grants are objects so prune revokes them; listens where the host grants (D13); `client` (`mariadb`, password in `MYSQL_PWD`) and `detect`. Oracle: every differing case differs only by the two listed improvements. 906 KB release. Follow-up (session 4): engine tests for the template path (made once, cloned, converged; it found the SDK's `template.clone` never made the data directory's parent, fixed in `doze-module`) and the TCP path (the server on a loopback grant, converge and a login over TCP), through the harness's new `Boot { tcp, template }`. | 3 | same, plus a login as the declared user (the Go test never does) |
| 10 ✓ | **Built 2026-11-14** (`crates/doze-module-ferret`, core plan M1), better than Go (D16), each proven on FerretDB 2.7.0 over documentdb 0.112-0: nothing listens on TCP (PostgreSQL on its socket only, the extension's self-connection pointed at it with `documentdb.localhost_connection_string`, the gateway's debug handler off), so no port is picked at all (Go picked two at random per boot and trusted every local TCP client: bug 6); seeds keep integer kinds (bug 6); an index whose keys or options changed, or whose name changed, is rebuilt, and indexes are objects so prune drops a removed one; seed inserts are cut by count and by size; an independent audit (14 findings) fixed in full; `writeErrors` fail the converge (Go's driver did too, a hand client must); PostgreSQL stops with a fast shutdown; the template is cloned copy-on-write; names, index keys, owned settings and a missing or broken seed file are refused at their line. `auth = true` and `user` blocks are refused at their line: proven on the engine, the gateway refuses doze's own setup once auth is on (`createUser requires authentication`), so Go's scaffold failed at boot (bug 6's first half is moot; ⚑ real auth needs a SCRAM client and a doze-owned admin user, about a day). D6: a hand-written BSON and OP_MSG client (+0 packages) after measuring the `bson` crate (+447 KB) and the driver (+2.4 MB). `client` (`mongosh`), `detect`. Contract: `Instance.dir` (the block's folder; ferret's `seed`, postgres's extension `source`). Oracle: every differing case differs only by the listed improvements. 1,006,976 B release. | 5 | same, plus a raw MongoDB hello through the socket |
| 11 ✓ | **Built 2026-11-14** (`crates/doze-module-postgres`, core plan M1), better than Go (D16), each proven on PostgreSQL 17 and 18: converge keeps the declared state (bug 10: a removed membership revoked; a removed password, expiry, role setting or comment cleared; database options and owner put back; a grant is REVOKE ALL then GRANT in one transaction, so a privilege taken out of the list goes) and grants are objects, revoked by prune; a pruned role's objects pass to `postgres` in every database (REASSIGN OWNED, DROP OWNED), so its data stays; `autovacuum` on unless declared off (bug 1); setting names have no case anywhere (bug 11); refused at their line: a setting doze owns, two names PostgreSQL reads as one, a value that would break doze.conf, a privilege the target does not take (per level, did-you-mean), an unknown extension (did-you-mean), a reserved role, a second grant on one target, duplicate schemas, a `source` that is not a file; SQL goes to psql on stdin (no password on a command line), no `PG*` variable or psqlrc reaches it; a database closed to connections is closed after converge; the cluster is made beside its folder and renamed in; fast shutdown; socket only (a TCP grant is refused: no Windows build yet). Console and admin as Go (activity, sessions, databases; cancel, terminate). `client` (`psql {url}`), `detect`. Extensions: contrib only (checked per major, created, updated in place to a declared version, dropped); doze's catalog is known by name and alias but refused unless `optional`, and `source` bundles are resolved against `Instance.dir` and checked, both installed by step 13. An independent audit (12 findings) fixed in full and each proven on the engine (`tests/declared.rs`): narrowing or pruning a grant never takes an owner's rights on what it owns (revoked object by object from what others own), revokes CASCADE so a grantee's re-grant cannot block converge; `standard_conforming_strings` is forced on and `lock_timeout` set per session, so a person's `standard_conforming_strings = off` cannot turn a quoted value into SQL or a psql command; a closed database is opened for prune and closed again, and a pruned role's objects are reassigned in every database, closed ones included; a converge racing another (boot and first connection) succeeds, as in Go; an extension or schema is dropped without CASCADE (Go dropped the columns of an extension's types and every table of a schema), `plpgsql` is refused (built in); list settings of a role (`search_path`) are lists; reserved role, schema and instance names, the include directives and owned or doubled role settings are refused at their line; a removed schema owner returns to `postgres`, a changed extension schema is applied, and the database's fixed fields left out of its hash. Oracle: every differing case differs only by the three listed improvements. Object hashes are the protocol's (CBOR), compared by kind and name. 1,039,040 B release before step 12. | 9 | same, plus the Example converging on 16, 17, 18 |
| 12 ✓ | **Built 2026-11-14** (`src/wire.rs` parsing, fuzzed as `fuzz_wire`; `src/filter.rs`): SSLRequest answered, TLS terminated with the host's certificate (rustls, ring), GSSENC refused, RequireTLS unless local, the startup replayed after the host boots the server, cancel keys swapped for random ones of the real length (protocol 3.2's 32-byte keys on PostgreSQL 18 too; Go passed those through, so cancel failed there), CancelRequest routed with the real key; better than Go: 30 s to send a startup, a second encryption request refused. Proven on 17 and 18: psql through the filter, TLS required and terminated, psql's own cancel (SIGINT), a hand-written driver on protocol 3.2 sees a synthetic key and cancels with it, acquire/release balance. Not done: byte-equal replay against recorded Go sessions (no recorder exists; the driver test checks the bytes that differ). Weight: TLS takes the release to 1,929,600 B (+890 KB for rustls and ring; recorded, see the M1 report). | 3 | real psql through the filter, cancel works, byte-equal with Go on recorded sessions |
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

1. ✓ postgres `autovacuum`: docs say default true, code defaults to off. Pick one. (Step 11: on, as the description
   always said and PostgreSQL's own default; `autovacuum = false` turns it off.)
2. `declareWanted` process-global map in postgres extensions (fixed by D2).
3. Any executable in an extension bundle's `bin/` overrides a toolchain tool by name (could shadow `psql`).
   Only declared tools override.
4. ⚑ `source =` extension bundles: no checksum, no lock pin, no ledger, no lock. Require a `sha256` or pin it
   in `doze.lock` like every other download.
5. Non-deterministic choice of a bundle pin (map iteration). Deterministic.
6. ✓ ferret: `user` blocks ignored when no `database` is declared; seed JSON loses integer types; its private
   Postgres trusts every local TCP client. ⚑ The last one: listen on a unix socket only. (Step 10: socket only, seeds
   keep integers; users and auth are refused at their line until auth works, see step 10.)
7. ✓ valkey/kvrocks: password not URL-escaped; valkey's `--requirepass` visible in `ps`. Escape; pass the
   password through a config file. (Fixed in step 7: `doze_resp::redis_url`, valkey's 0600 `doze-auth.conf`.)
8. ✓ mariadb: privileges interpolated into SQL unvalidated; whether the anonymous account shadows `%` users is
   unverified. Validate against a whitelist; test a login. (Answered in step 9: privileges checked against MariaDB's lists, and
   the acceptance test logs in as the declared user; no anonymous account shadows it.)
9. ✓ temporal: Describe lists `["1"]` but the code and CI use "1.1"; any `describe` failure is read as "absent"
   and triggers `create`. Fix the label; only "not found" means absent. (Fixed in step 8: `version = 1` is the newest 1.x;
   only "is not found" is absent, and converge proves a dead server errors instead.)
10. ✓ postgres prune: `DROP ROLE` without REASSIGN/DROP OWNED fails on owners; removed `member_of` is never
    revoked; removing a password never clears it. Converge to the declared state. (Step 11: a pruned role's objects
    pass to `postgres` in every database and its privileges go, so the data stays; memberships, password, expiry,
    role settings, comments and database options are put back to what is declared; grants are objects, revoked when
    removed and narrowed when a privilege leaves the list.)
11. ✓ postgres settings: case-insensitive lookup vs case-sensitive override check. One rule. (Step 11: names have no
    case everywhere; two names PostgreSQL reads as one, and a setting doze owns, are refused at their line.)
12. ⚑ ferret auth (follow-up, coordinator 2026-11-14): `auth = true` and `user` blocks stay refused at their line.
    Real auth needs a SCRAM client in the module and a doze-owned admin user that converge authenticates as (about
    a day). Not built in M1.
13. Open from the M1 re-audit (2026-11-14; its high and medium findings are fixed: ferret's socket cleared before
    its running check, postgres's racing template makers, keepalive on handed-over clients, versions CI never accepted
    (mariadb 11.8, postgres 16 added); also mariadb's pid check, ferret's owned keys, the filter's 10 000-byte startup
    limit, a reply on every refused path, direct SSL named, a bad handshake ending the session):
    ferret prune drops collections nobody declared with a removed database (as Go; postgres refuses that);
    mariadb owned options can be spelled as prefixes (`sock`, `datad`); a template clone that fails half way cannot be
    retried (clone beside and rename); ferret compound index keys lose their order (BTreeMap, as Go); fuzz markers for
    ferret's seed reader, mariadb's grant-name and SHOW GRANTS readers, postgres's psql row split; filter tests for
    GSSENC, a second encryption request and the startup timeout; small consistency nits (unknown prune kinds,
    `tool()` and `tc.tools`, socket paths unescaped in URLs, "already running" wording).

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
