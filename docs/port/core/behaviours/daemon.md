# D1: the daemon (doze-daemon) — behaviours and protocol notes

What the Rust daemon does against Go's `internal/daemon`, `internal/daemonctl` and `internal/control/server.go`.

## Starting and stopping (`ctl`, Go's `daemonctl`)

| | Go | Rust | Why |
|---|---|---|---|
| Start | `doze __daemon --config <abs>`, `Setsid`, stdout and stderr appended to `run/doze.log`, `--var`s as `DOZE_VAR_<name>`; waits up to 10 s for the socket, quoting the log's last 15 lines on failure | the same (`ctl::start`) | |
| Zombie | the started process is released; a long-lived starter (the Go app) kept a zombie until it exited | a thread of the starter reaps it (`doze_os::detach::spawn_detached`) | the app starts stacks and lives for days |
| Pid file | `run/doze.pid`: the pid and a newline, written by the daemon | the same, written once the socket is up | |
| Stop | SIGTERM to the pid, wait 20 s | the same; on Windows, where another console's process cannot be sent CTRL_BREAK, it is ended (its Job objects end its engines) | |
| Stack lock | none | `run/doze.ctl.lock`, an advisory lock (`ctl::StackLock`) `up` and `down` hold while they start or stop the daemon; the OS drops it when its holder dies | two `up`s must not start two daemons for one stack |
| A second daemon | a second bind fails | `__daemon` refuses when a daemon already answers on the socket | |

## `__daemon` (`entry`)

doze.hcl through doze-config (this process's environment, `DOZE_VAR_*` included; doze-names' domain checks), `to_stack` (apps, services and tasks, an `after` on a task a need), the engine, the server on `<data_dir>/run/doze.admin.sock`; SIGTERM or SIGINT stops the server, then the engine stops everything it started, then the pid file goes.

Certificates are checked for presence only. The reaper and the health prober run for the daemon's life (`Engine::background`; before session 8 nothing ever slept), and the module pool stops module processes idle for 2 minutes.

## The module catalog (`catalog`, `planners`, `spec_tree`)

Nothing here names a module: everything comes from the module's own answers.

| | Go | Rust | Test |
|---|---|---|---|
| Finding a build | `internal/modules` in the daemon | a `__fetch` child resolves `doze/<type>` against the registry (`DOZE_REGISTRY`), verifies, caches and pins it in doze.lock (F1's `Request::Module`, `persist`) | `daemon_catalog.rs` (a stand-in `Finder`); live: the proof below |
| What a type declares | plugin RPCs per question | the module's manifest from its hello: versionless, default port, supervised, schema, client | `catalog_tests.rs` |
| A version it does not serve | refused at load | refused at load: "echo \"7\" is not a version the echo module serves (it serves 1, 2)" | `a_version_the_module_does_not_serve_is_refused_at_load` |
| A type with no build | refused at load | refused at load with the fetch's reason | `a_module_that_cannot_be_found_says_why_at_load` |
| The block | gob specs | the protocol's value tree: an argument a map entry, a block type a list of maps, labels named by the schema (an unknown block's by position, `label0`) | `spec_tree::tests`, the echo's `settings` and `channel` reach it |
| References (`module.db.url`, `.env.X`) | `attributesFor` | the module's `connection` and (with the capability) `attributes` | `published_as_the_module_says` |
| The main listener's backend | a socket in the run folder | a socket file `run/<name>/<backend_socket>` (`{port}` filled in) where the system has sockets, the listener is not `tcp` and the path fits 104 bytes; else a loopback port | live (postgres needs a socket); the echo test takes the long-path fallback |
| The endpoint | the grant | the grant, else the first process's check (postgres's check is a command) | live |
| Engine pins | written by `up` | each pin a module's `resolve` reports is kept for the instance's later starts and written to doze.lock (`record_pins`) | `the_engine_pin_a_module_reports_is_written_to_the_lock` |
| Pins in front of downloads | — | `Pinned<ChildDownloads>`: the lock's hash wins over the module's | `daemon_catalog` (`sha256:pinned`) |

## The front (`front`, D2)

Every module that is not supervised and every app or service with a port it does not bind itself is fronted. Where each of its listeners goes comes from `doze-bind`'s plan (below, Names); a connection wakes it (cause `connection`; what it needs wakes as `dependency`), is a holder while it lasts, and is spliced: the main listener to the engine's endpoint (TCP or a socket file), another listener (kafka's console and registry) to the backend its module was granted for it (`ModuleInstance::listener_backend`, the loopback port chosen at that start). Tests: `wake_through_the_front` (conns 1 while open, 0 after), `daemon_live.rs`.

A connection is counted against one declaration of its instance (`Engine::hold`): one that outlives a remove and an add of the same name gives back nothing of the new one's (D1 audit item 3). Test: `doze-engine tests/live.rs a_connection_to_a_removed_instance_does_not_count_against_one_added_under_its_name`.

### `wire.accept` and client TLS

| | Go | Rust | Test |
|---|---|---|---|
| A module that takes its clients (`WireProxy`) | the proxy hands it the connection with the TLS key and cert (DER) and `RequireTLS` | the same: a module whose manifest declares `wire` is handed each main-listener connection (`Running::wire_accept`); it boots, acquires and releases through the host's callbacks (`catalog::WireHost`, the engine attached once built) | Go's `TestPostgresConverge`, `TestPostgresReconvergeAddsRole` through `DOZE_BIN` (psql through the postgres filter) |
| `tls { }` without files | a self-signed `localhost` certificate per project (`run/tls/doze.crt`), which no client can verify | a leaf minted from the machine's doze CA (`<home>/ca`): `*.<stack>.doze` with names on, `localhost` without; `doze trust` makes `sslmode=verify-full` pass | manual proof below |
| `tls { cert, key }` | loaded | loaded (PEM, the first certificate and the key in its own form), relative to doze.hcl | |
| `required = true` | a plain client refused | the same (the filter's words: `FATAL: doze: SSL connection required`) | manual proof below |

Proof (2026-11-19, the debug `doze`, the Rust postgres module from a local signed registry, `tls { required = true }`, postgres 16): psql `sslmode=require` gets `SSL connection (protocol: TLSv1.3, cipher: TLS_AES_256_GCM_SHA384)`; `sslmode=disable` gets `FATAL: doze: SSL connection required`; `host=localhost sslmode=verify-full sslrootcert=<home>/ca/root.crt` answers.

## Names (`names`, D2; Go's `bindplan.go`, `domains.go`, `ingress.go`, `listeners.go`, `dynamic.go`)

With `defaults { names = true }` the daemon takes the stack's name in the shared registry (`<home>/names.json`, owner `doze`; a reserved stack name and one another project holds are refused with Go's words), plans every fronted instance with `doze-bind` and binds each one: its name claimed, its listeners opened through `Lease::listen` (bound where the system can, a private port the network daemon translates on a Mac), its route names twinned and routed, and the grant its module is told. Planning happens before the engine is built, because the grant (the instance's name on each listener) is what the module plans with: `KAFKA_BROKERS='events.mods.doze:<port>'`, a broker advertising `events.mods.doze`.

| | Go | Rust | Test |
|---|---|---|---|
| Addressing | own address when names are on and `PoolUsable`; else 127.0.0.1, names still resolving to it | three, from `doze-bind`: `Off`, `Shared` (names on, 127.0.0.1, ports must differ), `Own`. On a Mac `Own` needs the network daemon to serve **this** zone (`zone_usable`: `<home>/netd.pid` alive and the gateway bindable): the machine's daemon serving `~/.doze` never translates a test zone's addresses | Go's `TestDozeAndAStandaloneToolShareTheZone` (macOS: Shared; Linux VM: Own), `TestANameAStandaloneToolHoldsStopsTheStack` (Linux VM) |
| Its own name held by another program | `up` fails, naming the name and the holder | the same with `Own`; with `Shared` the name is noted (`name_held`) and the instance is reached by port, as Go's `noteHeld` | the same |
| A route name held (`sync-aws`) | noted, the rest runs | the same (`BoundInstance::held`) | `doze-bind apply_tests a_route_name_another_program_holds_is_said_and_the_rest_is_bound` |
| A routed listener's port taken, on a shared address | the start failed | listens on a free port; the front door still reaches it by name; said in the log (`events: console: events.mods.doze:9093 is taken; it is reached by its name only`) | `doze-bind apply_tests on_a_shared_address_a_routed_listener_whose_port_is_taken_is_reached_by_name` |
| An app's port | the front door routes `<app>.<stack>.doze` (no port) to doze's listener; with an address of its own it is also on 127.0.0.1 at its port when free (`name_only` otherwise) | the same | `doze-cli tests/daemon_names.rs` |
| An app's `domains` | twinned onto its name, the person's cert recorded on them, `RecordDomains`; the CA made and `caEnv` set when one has no cert | the same (`Zone::domains`, `names::ca_env`); D1's `ca_env` read `<home>/tls/ca.pem`, a path nothing writes, so it is gone | |
| The resolver and the front door | joined (first-come, take-over) | the same: `doze_names::resolver::serve`, `front_door::serve`; left at shutdown, after the listeners close | `TestDozeAndAStandaloneToolShareTheZone` both orders |
| add / remove | claims and releases the name (and its twins), republishes routes | the instance is planned and bound alone (`Front::add`); remove drops its `Bound` (sockets closed, twins then name released) | `daemon_names.rs` |
| Views | `endpoint`/`bind` the real address, `domain` the zone name, `url` the front door's | the same; `domains`, `https`, `name_only`, `name_held` filled | `daemon_names.rs` |

Private zones (tests, two zones side by side) move the resolver and the front door with `DOZE_ZONE_RESOLVER`, `DOZE_ZONE_INGRESS` and `DOZE_ZONE_INGRESS_TLS`, as Go's.

Evidence (2026-11-19):
- Go's `test/integration` through `DOZE_BIN` (rust-oracle 57aef16), private zones: macOS — `TestDozeAndAStandaloneToolShareTheZone` (both orders) pass, `TestANameAStandaloneToolHoldsStopsTheStack` skips (as for Go: a Mac private zone has no own addresses); the Lima VM (Ubuntu 24.04 arm64) — all three names tests pass, the held-name one included.
- The kafka half of `TestModulesAnswerAtTheirNames` with the Rust kafka module (a scratch script, `bun kafka-names.ts`, its checks those of the Go test): `doze env` says `KAFKA_BROKERS='events.mods.doze:<port>'`; `events` and `registry.events` resolve; the broker advertises `events.mods.doze:<port>`; the console answers at `http://events.mods.doze`; a schema posted at `http://registry.events.mods.doze` is listed; asleep, the broker's port and the registry's name each wake it (15 ms, 25 ms). It found a kafka module bug, fixed: the registry refused the name doze routes it by (403).
- The aws half needs a Rust aws module (M3); the Go test itself builds Go plugins, which the Rust doze does not run.
- The macOS netd path (`Own` addressing, the system resolver, :80 and :443) needs root: `scripts/core-d2-names-e2e-darwin.sh` (owner-run).

## A service's `download` (D2; Go's `runtime.fetchDownload`, `binaries.EnsureDownload`)

| | Go | Rust | Test |
|---|---|---|---|
| The fetch | a `__fetch --url --sha` child | a `__fetch` child (`Request::Download`); the engine asks the wiring's `Fetch` before each start, and the command runs in the download unless its block set `dir` | Go's `TestADownloadedServiceIsPinnedAndAChangeIsRefused` through `DOZE_BIN` (macOS and the Linux VM) |
| Cache | `<home>/downloads/<sha256(url)[:8]>`, `root/` and the digest | `<home>/downloads/<sha256(url)[:16 hex]>`, the same layout | `doze-fetch download::tests` |
| Pin | doze.lock `downloads:`, first fetch pins, a changed source refused before unpacking | the same, written under the home's lock on the file | the same |
| Formats | `.tar.gz`, `.zip`, a bare executable | `.tar.gz` and a bare executable; a `.zip` is refused with a word (the reader would weigh on every `doze`) | `a_bare_executable_gets_a_folder_and_a_zip_is_refused` |
| By platform | `mac`, `mac_intel`, `linux`, `linux_arm`, or `""` | the same (`doze_config::stack::platform_key`); none for this machine says which it has at start | |

## The app on the Rust daemon (session 8 proof)

A local registry signed with a throwaway key (a `dozetest` build reads `DOZE_TEST_REGISTRY_KEY`; release builds never do) served the Rust postgres and valkey modules (core-m1). A shop stack (web → api → postgres, valkey, a migration task) under its own DOZE_HOME, the unchanged app (app-finish `cf23b9bf`, release-app, fixtures for the driver) hands-off. Cold curl of web: 1.33 s, the whole chain woken through the fronts; everything slept after 45 s idle; env/URL/client from the modules on the tiles. kafka and aws faces were not part of it.

Closed in session 9: converge on a fresh provision (below), stats (below). Closed in D2: a task view showed `conns=1` (a task a running app needed was held by it as a need); a task is no longer held once it ran (`Machine::need_held`; `doze-engine tests/tasks.rs a_service_waits_for_the_task_it_needs`).

## Protocol 2: additive fields (Rust daemon only)

Go's clients ignore fields they do not know, so these leave every field Go defines byte-identical.

- `InstanceView.env` (object, omitted when empty): the variables a client sets to reach the instance (`DATABASE_URL` and the rest), as the instance's module answers them (its attributes through doze-module-host). Never computed by module-specific code in the daemon.
- `InstanceView.client` (string, omitted when empty): the client program the module declares, for `doze connect`.

Both are filled from the module's own answer (`Catalog::published`); apps, services and tasks carry none.

## D1 catalog audit (session 8, independent): what was found and done

1. Two instances recording pins at once lost pins and failed the save (shared temporary name): `record_pins` takes a process-wide lock. *Fixed; no concurrent test.*
2. The module reaper could stop a module mid-call (idle counted from a call's start; a download in `resolve` takes minutes): a module is idle only with no call in flight, counted from the last call's end (`Running::idle_for`). *Fixed; no test yet.*
3. Supervised modules slept and got a front: `ModuleInstance::supervised` from the manifest; no front for them. *Fixed.*
4. The backend port is chosen at daemon start and could be taken later; when no port could be had the backend fell back to the declared address, so the front dialled itself. *Fixed (session 10): a loopback backend is granted as `127.0.0.1:0` and the engine chooses the port at each start (`ModuleInstance::choose_ports`), kept for that start's later calls; no free port fails the start with why.*
5. Front errors went to `tracing`, which `doze` does not install: they go to the daemon's log. *Fixed.*
6. Mutations that survived: pins kept for a later start (now `the_engine_pin_a_module_reports…`: the mirror is asked once over two starts), the enabled filter (a disabled module in the test gets no front), the socket branch and its fallbacks (`planners::tests`). The endpoint taken from the grant and `run()`'s wiring: tested in session 10 (below).
7. `listen = "unix:/x"`: the declared socket is the engine's backend; a port that is not a number takes the manifest's default. *Fixed.*
8. A second instance of a type with another major uses the first's build (one build per type). *Fixed (session 10): when the first build does not serve a major, the build found for it runs as `type@major` in the pool (`catalog_builds.rs`); test `two_majors_no_build_serves_both_run_on_two_builds`.*
9. Pins were filtered by type, not engine: every lock pin is passed and the module takes its own. *Fixed.*
10. A second daemon starts modules before refusing. *Fixed (session 10): `entry::no_daemon_yet` reads the data folder from the shape pass (no modules) and refuses before the catalog exists; test in `doze-cli tests/daemon.rs`.*
11. The `dozetest` key: said on stderr when in use or not a key. *Fixed.*

## Session 9: converge on start, meters, the data ops and the console

| | Go | Rust | Test |
|---|---|---|---|
| Converge on start | the boot guard: `fresh \|\| !marked \|\| drifted` converges after the backend is ready and before `on_ready`; a failure taints, stops it and fails the start | the same (`doze-engine/src/converge.rs`), before the start registers; marker `.doze-converged` holds the fingerprint, no newline, as Go's; `sync` writes it too | `tests/sync.rs a_start_converges_a_fresh_or_changed_instance_once` |
| Fingerprint | sha256 of the plugin's gob spec bytes | sha256 of the version and the block's value tree with every source span cleared (an edit elsewhere in doze.hcl changes nothing) | `modules::tests::where_a_value_was_written_is_not_part_of_the_structure` |
| Meters | `ps -Ao pid=,ppid=,rss=,%cpu=` once for every root: the root's RSS, the subtree's CPU, an app's children (comm basename, depth first) | the same through `doze_os::meters` (one `ps`; Windows measures nothing, as Go got nothing); children for apps, services and tasks | `doze-os meters::tests`, `doze-daemon tests/meters.rs`, `fuzz_ps` |
| Stats ring | 240 frames, readings closer than 800 ms are one, unwatched every 5 s while something runs; a subscriber gets the ring first | the same (`doze-daemon/src/meters.rs`) | `tests/meters.rs` |
| `reset` | stop, remove the data and socket folders, forget a task's run | the same | `doze-module tests/daemon_admin.rs` |
| `resources`, `admin` | the module's Admin; empty without it; "is not running — boot it first" | the same, from the module's manifest (`admin` capability) and calls | the same |
| `console` topic | spec and actions first, the stream panel's rows, tables every 2 s, `_runtime` for undeclared rows; never wakes | the same (`doze-daemon/src/console.rs`) | the same |
| `add`, `remove` | a block wired into the running stack | done in session 10 (below) | |

Differences: an instance's engine word in "has no data actions" is the config's type (`process` for a task) where Go said `app`.

### For the CLI (core-c2c)

- `connect`: `InstanceView.connect` (additive, omitted when the module declares no client): `program`, `path` (the program in the engine's own build, set once a start resolved it), `args` and `env` with `{host}`, `{port}`, `{url}`, `{password}` filled from doze's address for the instance and its connection URL, `fallbacks` (tried on `PATH` when `path` is empty and `program` is not found). The CLI runs it as given. Test: `daemon_admin.rs the_client`; rendering: `client::tests`.
- `sync --dry-run` and sync's question: the `plan` op (additive; a Go daemon answers "unknown op", so a client cannot apply by mistake) answers `Response.changes`, each `{instance, change: create|update|delete, kind, name}` in the order `apply` would make them. Each module says what its declaration wants (`objects`), against `.doze/state.json`; nothing is woken, converged or written. An empty list means sync has nothing to do (ask only when it is not empty, as Go did). Tests: `doze-engine tests/sync.rs a_plan_lists_the_changes_and_changes_nothing`, `daemon_admin.rs` (the wire).
- `migrate` and `lint`: served from session 10 (below).

### Session 9 proof: a fresh home, no manual step

`/private/tmp/claude-501/dzb9` (the session 8 registry and shop, a new empty DOZE_HOME, `doze` release with `dozetest`, 7,014,080 B): the daemon started, `task migrate` was asked over the socket (`proof-migrate.json`: ok in 21 s, the postgres engine downloaded first); postgres's start converged its database (the one its URL names) before the task ran, `.doze-converged` holds the fingerprint, and the migration applied (`logs/migrate`). No `apply`. A curl of web then woke api and cache through the fronts (`proof-curl.txt`, the first one waited out the valkey download) and status shows resident memory per instance (`proof-meters.txt`: db 12.2 MB, api 22.0 MB, cache 5.3 MB, web 17.0 MB). Not yet in this proof: the app's shots.

## Session 10: add and remove, migrate, lint's module pass, the audit's open items

| | Go | Rust | Test |
|---|---|---|---|
| `add` | the block parsed alone (`config.Parse`), its engine looked up, a listener and a name, a supervised process booted; references to other instances refused with "pass the literal value instead" | the same through doze-config's `parse` and the catalog (`live.rs`): the engine's service (`Engine::add`: name new, needs declared, no cycle, checked as at build), labels, a front for what doze fronts; an app without a port or a supervised module starts at once, and a refused start or a port someone holds takes the add back. Answers the new instance's view | `doze-engine tests/live.rs`, `doze-module tests/daemon_live.rs`, the real `__daemon` in `doze-cli tests/daemon.rs` |
| `remove` | stops it, closes its listener and name, drops it from the config; `wipe` deletes its data, logs and sockets; a need on it was left dangling | the same; refused while another instance needs it ("is needed by \"api\"; remove it first"); it is taken out of the run before it is stopped, so nothing wakes it meanwhile, then the machine forgets it | the same |
| `migrate` (additive op) | the CLI's own postgres dump and restore, stack down | protocol call `migrate` (method 25, capability `MIGRATE`), the instance's module does its own move; `fetch.*` callbacks are allowed during it (a callback's scope is now a list: `resolve` or `migrate`) so a module fetches the old major. The daemon takes the instance out of the run, asks, puts it back; `Response.result` is what the module said. A module without the capability: "db is valkey; its module has no migration (…)" | `daemon_live.rs migrate_is_the_module_s_own_with_the_instance_stopped` (echo) |
| `lint` (additive op) | the CLI loaded doze.hcl with its modules | the stack's doze.hcl, as on disk now, loaded through the daemon's catalog; `ok` with warnings in `lines`, `unchecked` for types the registry could not be reached for; the load's error otherwise; another stack's file is refused | `daemon_live.rs lint_loads_the_file_on_disk_through_the_modules` |

Open for other tracks: the postgres module (core-m1) answers `migrate` with Go's dump, set-aside and restore (`cmd/doze/migrate.go`, through `cx.fetch()` for both majors); the CLI (core-c2c) sends `migrate` and `lint` and prints their answers.

Tests added for the audit's untested paths: the endpoint taken from the grant (the echo with `settings.ready = "log"` has no check naming its address), `run()`'s wiring (add/remove and a second daemon on the real binary), the module idle fix (`doze-module tests/host.rs a_module_is_not_idle_while_a_call_is_in_flight`), the pin writer's lock (`catalog::tests::pins_recorded_at_once_are_all_kept`).

### Session 10 audit (independent, at 4d7c98c4): 14 findings

1. Per-major builds and doze.lock (one pin per module source): the second build was resolved through the first's pin and failed. *Fixed: a second build (`type@major`) is resolved from the registry with no lock and recorded nowhere; pinning it needs a lock schema with a pin per source and major (F1's).*
2. A dropped module call (a cancelled start, a plan timeout) left the module "in flight" for good, so it was never reaped. *Fixed: the count is an RAII guard (`Busy`).*
3. A connection that outlived a remove recreated the instance on release. *Fixed: acquire and release do nothing for a name no longer declared.* A stale release after the same name is added again counted against the new one: *fixed in D2* (`Engine::hold`, per declaration; the front's splice holds one). A module's own `wire.release` callback is still by name.
4. A remove that failed after taking the instance out of the run left it off. *Fixed: it goes back as it was; a failed take-back of an add is said in the log.*
5. Two migrates (or an add/remove) of one name at once. *Fixed: one add, remove or migrate per name at a time (`Daemon::claim`).*
6. `migrate` uses the block the running daemon loaded, not doze.hcl on disk. *By design for now: the CLI restarts the daemon after an edit before it asks (C2c), as Go's migrate needed the stack down.*
7. Lint wrote doze.lock and was stuck behind cached lookup failures. *Lookups that failed are asked again by each lint and add (`retry_failed`); lint pinning a new type is documented (as `up` would).*
8. Two daemons starting at once both passed the check. *Fixed: `run/doze.daemon.lock`, held for the daemon's life.*
9. Concurrent add and remove of one name. *Fixed by 5.*
10. A disabled block went into the run; a name disabled in doze.hcl could be added; a fronted module with no address was silently unfronted. *Fixed: a disabled block is added out of the run, names doze.hcl disables count as taken, and the portless module is refused with Go's words.*
11. A failed `type@major` lookup reads as "not a version the module serves". *Fixed in D2*: `pg "14": the pg build for 14 could not be had: <why>` (`catalog_tests a_failed_lookup_of_a_majors_build_says_why`).
12. A new call and a widened callback scope with no protocol minor. *Fixed: minor 1; `migrate` is since 1.*
13. A remove then an immediate add on the same address can find it still bound. *Fixed: the bind is tried again for half a second while the address is in use (the busy-port refusal waits that long too).*
14. The per-start port is chosen by binding port 0 and letting go. *Inherent; known.*

### Session 10: Go's integration suites through `DOZE_BIN` (doze rust-oracle, 57aef16)

The Rust `doze` was D1 (91ec2495, then with the watch fix) merged locally with #56 (core-c2c 5328d9e9) on a scratch branch, never pushed; `release-doze` with `dozetest`, 6,791,232 B. Logs: `/private/tmp/claude-501/dzb10/go/`.

| Suite | Result | Why |
|---|---|---|
| `TestAServiceWakesOnConnectAndSleepsWhenIdle`, `TestAFailedOnReadyKeepsTheCommandAndRerunsOnlyTheStep`, `TestStopStepsAndSIGTERM`, `TestATaskRerunsOnlyWhenItsInputsChange` | pass | |
| `TestWatchRestartsAnAppAndRerunsItsTasks` | fail, then pass | doze-config never carried `watch` to the engine; fixed (4d7c98c4) |
| `TestPostgresConverge`, `TestPostgresReconvergeAddsRole` | fail, then pass with `DOZE_REGISTRY` at the local signed registry (the Rust postgres module) | the harness hands doze Go plugins (`DOZE_TEST_*_PLUGIN`), which the Rust doze does not run; the first run, without the variable, asked the real registry's index once (it timed out after 3 s) |
| `TestMariaDBConverge` | fail | as postgres, and no Rust mariadb build in the local registry |
| `TestSetupRunsOncePerDataFolder` | fail | C2c: `reset` with the stack down says the daemon is needed (Go removed the folder itself) |
| `TestAFailedTaskStopsWhatIsAfterIt` | fail | C2c: `doze task` does not say `manual` (Go read it from the config) |
| `TestADownloadedServiceIsPinnedAndAChangeIsRefused` | fail | a service's `download` is not fetched (doze-config's open gap) |
| `TestSyncNamesTheBlocksThatDifferFromTheRunningDaemon` | fail | C2c: `sync --dry-run` still DZ0001; the daemon's `plan` op is there |
| `TestDozeAndAStandaloneToolShareTheZone` | fail; the other names suites and ferret, temporal skip | names are D2 |

Found on the scratch merge, for C2c: its `__fetch` child hung on the postgres engine download from the public mirror (GitHub doze-binaries, where the D1 child also fetches engines; the local registry holds modules only), so `up` on the merge hangs on a fresh home.

### Session 10 proof: the app, a fresh home, no manual step

`/private/tmp/claude-501/dzb10/tools/proof.sh` (the D1 `doze` at cabe4ad6, `dozetest`, 7,146,368 B; the unchanged app at app-finish a7af57a4, `release-app` with `fixtures`, driven by `app/proof.txt`). Evidence: `app/driver.log`, `app/proof.log`, `shots/01…09`, the first attempt in `run1/`. The stack is started as `up` starts it (the `__daemon`, then the `up` op: the CLI's `up` is C2c's); then: up in 87 s with the postgres engine downloaded and the migration run as api's need; everything asleep; a curl of web woke it in 37 ms through its front (`03`); the map, all logs, db's page with its real memory and CPU graphs, "api needs it" and its 10 connections (`06`); the daemon stopped with nothing left behind.

Not shown: a module console tab (neither registry build, postgres or valkey 0.9.0, declares a console; the daemon's console topic is tested with the echo); admin/reset through the app (the script's ⌘K typing did not open the palette; reset is tested over the wire). Seen, for the app track: the tiles and the sidebar show 0 for memory while the page's graph shows 12 MB. The `instances` topic carries no meters, as Go's did not (`handler.go`: "without meters"); status and the `stats` topic do. An earlier run's sidebar showed them, so the app reads them from one of those only at times.

### From C2c (session 11): what the CLI's track added to the daemon

- `run/blocks.json` (Go's `recordBlocks`): written by `entry::no_daemon_yet` once the daemon's lock is held, from doze.hcl's
  shape, through `ctl::write_blocks`; read by `ctl::blocks`. `doze sync` names the blocks that differ, and `doze migrate`
  restarts the daemon first when the instance's block differs (audit item 6 above).
- `doze __daemon --lint --config <path>` (`entry::LINT_ARG`): the `lint` op's module pass once, with no daemon serving, its
  answer as one JSON line; the catalog's module processes are stopped before it answers (`Pool::stop_all`, doze-module-host).
- The `__fetch` child's stderr is inherited (the daemon's log), and the child says an engine download as it starts.

### Session 11: from the C2c builder

- **Windows:** `doze-daemon tests/meters.rs` and `tests/wire.rs` skip where the system has no local control socket, as the doze-module daemon tests do.
- **Converge on start records what it made:** `.doze/state.json` takes the module's objects after a converge on start, as `apply` would, so `plan` (and `sync --dry-run`) lists nothing to create afterwards. Go wrote nothing there, and its plan then listed objects that existed. A start inside a running `sync` finds the state locked for more than a second and leaves the recording to the sync.
- **`lint` answers `unchecked` with reasons** (additive, Rust daemon only): `"unchecked": [{"type": "postgres", "why": "fetching module index doze/postgres/index.yaml: …"}]`, with `ok: true`.
  - A type is unchecked when its build could not be had: the registry unreachable, or a download or signature that failed.
  - A type that is not published at all is the load's error, as before.
  - The types come from the catalog's failed lookups in this load, not from a second read of doze.hcl: that read stops at the knock-on unknown references.
- **Flaky `a_stopped_task_is_asked_to_stop_and_ends_failed`:** its waits are 20 s under load, and its journal check waits for the events.
- **Not done, for doze-config (H4):** in the shape pass, a reference into a module's attributes (`module.e.env.X`) is an "Unsupported attribute" error, not an unknown value. A lint whose module could not be had then reports that knock-on error. The daemon's lint no longer depends on it.

## D2 gates (2026-11-19)

Go's `test/integration` (doze `rust-oracle` 57aef16) against the Rust `doze` (debug build of core-d2) through `DOZE_BIN`, modules from a local
registry signed with a throwaway key (`DOZE_REGISTRY`, `DOZE_TEST_REGISTRY_KEY`; the Rust postgres, valkey, mariadb, ferret, temporal and
kafka modules built from this branch), `DOZE_ZONE_INGRESS_TLS` moved for the names tests so nothing binds :443.

| Test | macOS (arm64) | Linux VM (Ubuntu 24.04 arm64) | Why, where not a pass |
|---|---|---|---|
| `TestDozeAndAStandaloneToolShareTheZone` (both orders) | pass | pass | |
| `TestANameAStandaloneToolHoldsStopsTheStack` | skip | pass | a Mac private zone has no own addresses (Go skips the same way) |
| `TestNamesThroughTheSystemResolver` | skip | not run | needs the machine's zone set up: `scripts/core-d2-names-e2e-darwin.sh` (owner-run) does the same checks on a throwaway zone |
| `TestModulesAnswerAtTheirNames` | skip | not run | builds Go kafka and aws plugins; its kafka half passes against the Rust kafka module (above); the aws half waits for a Rust aws module (M3) |
| `service_test.go`, all nine | pass | pass | `TestADownloadedServiceIsPinnedAndAChangeIsRefused` new in D2 |
| `TestPostgresConverge`, `TestPostgresReconvergeAddsRole` | pass | not run | psql through the front and the postgres filter (`wire.accept`) |
| `TestMariaDBConverge` | fail | not run | the test declares `version = 11`; the Rust mariadb module serves 11.4, 11.8, 12.2 (M1's, not the daemon's) |
| `TestFerretConvergeAndSeed`, `TestTemporalNamespace` | skip | not run | no `mongosh`, no `temporal` CLI on this Mac |

Wake latency and weight: docs/port/weight.md, "D2".

Open after D2:
- A module's own `wire.release` callback counts by name, so a hand-off that outlives a remove and a re-add of the same name still
  releases against the new one (the front's splice holds per declaration).
- `Serving::stop` wakes its accept loop by connecting to the socket path: if the path was replaced meanwhile (another daemon on the same
  folder) the stop waits for ever. Found when a test deleted a running daemon's folder; D1's.
- The CLI keys a project by `$PWD` when it names the working folder: run from `/tmp/x` and from `/private/tmp/x` (one folder on a Mac)
  it finds two projects. Go's behaviour, kept.
- `Labels.url` for an app is fixed at start: Go asked the registry at each view whether a doze program holds :80.
