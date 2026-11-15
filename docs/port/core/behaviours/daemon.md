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

## The front (`front`, the plain-TCP part of D2)

Every module that is not supervised and every app or service with a port it does not bind itself gets a listener on its declared address. A connection wakes it (cause `connection`; what it needs wakes as `dependency`), is a holder while it lasts, and is spliced to the engine's endpoint (TCP or a socket file). No names, TLS or `wire.accept` yet (D2). Test: `wake_through_the_front` (conns 1 while open, 0 after).

## The app on the Rust daemon (session 8 proof)

A local registry signed with a throwaway key (a `dozetest` build reads `DOZE_TEST_REGISTRY_KEY`; release builds never do) served the Rust postgres and valkey modules (core-m1). A shop stack (web → api → postgres, valkey, a migration task) under its own DOZE_HOME, the unchanged app (app-finish `cf23b9bf`, release-app, fixtures for the driver) hands-off. Cold curl of web: 1.33 s, the whole chain woken through the fronts; everything slept after 45 s idle; env/URL/client from the modules on the tiles. kafka and aws faces were not part of it.

Closed in session 9: converge on a fresh provision (below), stats (below). Still open: a task view shows `conns=1`.

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
