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

Still open: `apply` is not run by the daemon at start (the CLI's `up` calls it; postgres creates the instance's database in `converge`); stats (RAM, CPU) read zero; a task view shows `conns=1`.

## Protocol 2: additive fields (Rust daemon only)

Go's clients ignore fields they do not know, so these leave every field Go defines byte-identical.

- `InstanceView.env` (object, omitted when empty): the variables a client sets to reach the instance (`DATABASE_URL` and the rest), as the instance's module answers them (its attributes through doze-module-host). Never computed by module-specific code in the daemon.
- `InstanceView.client` (string, omitted when empty): the client program the module declares, for `doze connect`.

Both are filled from the module's own answer (`Catalog::published`); apps, services and tasks carry none.
