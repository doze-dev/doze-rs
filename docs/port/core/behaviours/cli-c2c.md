# C2c: the CLI's daemon commands (behaviour catalogue, in progress)

Branch `core-c2c`, stacked on `core-c2b` (#53), with `core-d1` (#50, draft, still being built) merged in. Distilled from Go
`cmd/doze/{up,down,env,run,sync,migrate,daemonctl,remember}.go`, `internal/daemonctl` and `internal/endpoints` at doze
23b73d6, and the CLI proposal (rules 3, 7, 8; `--dry-run` table; exit codes).

## What D1 gives (merged at b51674d3)

- `doze __daemon` (the daemon is this binary, as Go's was) and `ctl` (start, pid, stop, the stack lock). Apps, services
  and tasks run; modules do not yet (waking one says so), and `InstanceView.env`/`client` are on the wire but empty
  until D1's module catalog lands. So `env`, `url` and `connect` still wait for those fields to be filled.
- A config that moves the state folder (`home`, `data_dir`): the CLI follows the loaded config's folder, as the
  daemon does (row 11). A doze.hcl that does not load leaves the default place.

## What D1 gave in session 9 (merged at ad060c37, core-d1 3e5ca62c)

- `InstanceView.connect`: the module's client command for the instance (`program`, `path` in the engine's own build once a
  start resolved it, `args` and `env` with `{host}`, `{port}`, `{url}`, `{password}` filled in, `fallbacks`). Row 19.
- The `plan` op: `Response.changes`, each `{instance, change, kind, name}`, nothing woken or written; a Go daemon answers
  "unknown op". Row 18.
- Converge on start, meters, `reset`/`resources`/`admin` and the console topic (the CLI's verbs and status already ask for
  them).

## Built

| # | Behaviour | Held by |
|---|---|---|
| 1 | `down` stops the daemon its pid file names, and only when that pid is still the daemon: a live process that leads its own group (the daemon runs in a session of its own; D1's `ctl` must keep that) and started no later than the pid file was written. It sends SIGTERM to that exact pid and waits up to 20 s (Go's budget). Go signalled whatever held the pid; a stale file naming a stranger, or another user's process (EPERM), now means "not running" and the file is cleared Go's result: `{"stopped": true}` and "✓ doze stopped", or `{"stopped": false}` and "doze is not running" | `tests/down.rs` (child processes play the daemon and a stranger; SIGTERM checked; pid 1), transcript `down` |
| 2 | Better than Go (rule 8): after the stop, no socket and no pid file are left. A leftover of a daemon that is already gone is removed, and the removal is said | `tests/down.rs` `leftovers_of_a_daemon_that_is_gone_are_removed_and_said` |
| 3 | A socket that answers with no daemon pid behind it is refused, with the reason (exit 1; Go said exit 0 and left it running). A daemon that does not stop is named with its pid (exit 8; Go said 1). | transcript `down-dry-run`; `tests/down.rs` `a_daemon_that_does_not_stop_is_named_with_its_pid_after_twenty_seconds` (a leader that ignores SIGTERM: exit 8 after 20 s, nothing cleared) |
| 4 | `down --dry-run` makes the same decision as the run: what is awake (from the snapshot) and the daemon stopping with its pid, or the leftovers it would remove, or the same refusal. It signals nothing. JSON adds `cleared` (leftovers removed) to Go's `{"stopped"}`, left out when empty | transcripts `down`, `down-dry-run` |
| 5 | On Windows a daemon is never identified (no process groups), so `down` clears leftovers and says "not running"; stopping waits for the named-pipe daemon (core plan W). | `tests/down_windows.rs` (a live pid in the file is never signalled; the file is cleared) |
| 6 | `down` remembers what was awake before it stops the daemon (Go `rememberDown`): the snapshot's awake services are joined to the set `up` recorded, as roots; `last_down` is stamped even with nothing running. A doze.hcl that does not load does not keep the stack from stopping (Go refused to stop it): it is a notice and nothing is remembered | `tests/down.rs` `down_remembers_what_was_awake_for_the_next_up`, `down_with_nothing_running_still_records_the_down` |
| 7 | `up` against a running daemon (Go `up.go`, `remember.go`): names checked against doze.hcl first (exit 4), `--last`/`--all` with names refused (exit 2), `-f` with `--json` refused before anything is asked or remembered (Go remembered, then refused). A bare `up` chooses: `--all` everything; `--last` last time's set (or everything, said); nothing smaller recorded: everything; a terminal with `-y`: last time's set, said; a terminal: "Last time: …. Start the same? [Y/n, a = everything]"; otherwise everything, with how to get last time's set. Then op `up` for each target and what it needs, needs first ("✓ db awake in 38ms", a task "done"), and "› up — supervised in the background. doze for the dash · doze down to stop"; `-f` streams the process services' logs until Ctrl-C (detaches). The set is written to `project.json` before the first `up` op, as Go writes it. `--dry-run` asks nothing and remembers nothing | `tests/up.rs` (requests and the record checked), transcript `up` |
| 8 | doze-projects writes the record (`update`, `save`) byte for byte as Go's `projects.Update`/`Save`: struct order, or all keys sorted when a newer doze's fields are kept (compacted), Go's string escapes and RFC 3339 nano times with their offset, a newline, written beside and renamed, mode 0600; an unreadable record set aside as `project.json.bad`. A newer doze's fields are kept as their bytes (`Raw`), invalid UTF-8 and lone surrogate escapes included, as Go's `json.RawMessage` keeps them: found by a byte scanner (`rawscan.rs`, fuzz target `fuzz_rawscan`, a read-write-read round trip) | `doze-projects/tests/write.rs` against `tests/fixtures/updates` (Go wrote them, tools/doze-oracle `update`, eight cases with `rawbytes`) and live when Go is here |
| 9 | `up` with no daemon running starts it (D1's `ctl::start`: `doze __daemon --config <abs>`, its own session, output to `run/doze.log`, `--var`s as `DOZE_VAR_*`, 10 s for the socket, the log's end quoted on failure) under the stack lock, then boots as row 7; a second `up` that waited for the lock finds it running and starts none. Row 12 has the lock. `up --dry-run` with no daemon says the daemon would start, then the wakes from doze.hcl | `tests/up.rs` `up_starts_the_daemon_and_down_stops_it` (the built doze as the daemon, the app ran, down leaves nothing), `with_no_daemon_up_starts_one_or_says_why_it_could_not` |
| 10 | `doze run -- <command>` (Go `run.go`): doze.hcl read first (exit 5), the daemon started if it is not running (as `up`, under the stack lock), then the command on this terminal; doze exits with its code (DZ0710, `Exit::Child`), and a command that cannot be started is DZ0711 (exit 1). Better than Go: a failing command's code is also said on stderr. `--env` says it waits for the modules' answers (DZ0001) | `tests/up.rs` `run_starts_the_daemon_then_the_command` |
| 11 | The CLI's state folder is the loaded config's `data_dir` (`home`/`data_dir`; a relative one from where doze runs, as the daemon started from here reads it), so `up`, `run`, `down`, `status` and the rest find a stack that moves it. The DZ0001 refusal is gone | `tests/up.rs` `a_moved_state_folder_is_followed` (the built doze as the daemon, record, socket and pid file in the moved folder) |
| 12 | The stack lock (`run/doze.ctl.lock`) is taken by `down` always (a stack that never ran gets its run folder for it; a lock that cannot be taken is exit 1, not ignored), by `up` against a running daemon too (held through the boots, released before `-f` follows), and by `run` while it starts the daemon. Waiting for another holder is said on stderr. A daemon that answers a moment before its pid file is written (D1 writes it just after the socket accepts) is waited for, up to 2 s, not refused | `tests/stack_lock.rs` (the test holds the lock; the pid file written 400 ms late) |
| 13 | **Better than Go, listed (the owner may veto):** `down` with a doze.hcl that does not load stops the stack and exits 0, with a warning naming the config's error ("doze.hcl does not load (…); the stack is stopped anyway, and what was awake is not remembered for the next up"). Go exited 5 and left running the stack the user asked to stop | `tests/down.rs` `a_broken_doze_hcl_still_stops_the_stack_with_a_warning` |
| 14 | `up`'s per-boot progress on stderr (Go `bootOne`, `relayBootProgress`): "→ name waking…", the daemon log's lines with downloading/verifying/fetching/extracting relayed as "  › …" (the daemon's own prefixes stripped), on a terminal the elapsed seconds from 3 s and the line cleared for the result. And Go's `trustHint` after the boots: "› <routes> needs a one-time setup here: doze trust (one sudo)", the authority that cannot be used, routes no stack declares, "not set up on this platform" off macOS/Linux | transcript `up`; unit tests in `cmd/progress.rs` (partial lines, invalid UTF-8, prefixes) and `cmd/trust_hint.rs`; oracle below |
| 15 | `open --console <name>` (new; the Go dash's `o`, `display.WebURL`): the instance's `console`, else its `web_url`, from the running stack, opened in the browser; with no display (SSH, Linux with none) the address is printed (stdout) for opening elsewhere. A name not declared is exit 4 (checked offline first when doze is stopped), doze stopped is exit 7, an instance without a console exit 1 (DZ0709), a browser that fails exit 10. It has a JSON result (`{name, url, opened}`); plain `open` still has none | transcripts `open`, `open-console`, `open-console-ssh` |
| 16 | `update`'s hand-over: after `brew upgrade doze`, each running stack, under its lock: what is awake read from its daemon, the daemon stopped as `down` stops it (identified pid only), a new one started from the `doze` on the PATH with the doze.hcl its record names, what was awake woken again. JSON adds `stacks: [{stack, moved, detail}]`; a stack not moved is named with why and the run exits 11. The services stop for that moment (passing listeners on is the daemon's, compat.md rule 5); `--var`s the stack was started with are not carried over | `tests/handover.rs` (the built doze moved to a new daemon, web woken again; a stack with no daemon pid: exit 11) |

| 17 | `env [service…] [--dotenv]`, `url <name>` and `run --env` (Go env.go, `urlCmd`): each module's own answer from the running stack (`InstanceView.env`, `https`/`url`/`endpoint`; D1's catalog at 872e3f18), nothing module-specific in the CLI. Names checked against doze.hcl first (exit 4, 5). With doze stopped the daemon is started for it (as `run` starts it; said on stderr; nothing wakes). Go's output kept: `export VAR='value'  # engine service`, `KEY=value` for `--dotenv`, a JSON list of `{service, engine, var, value}`, url's `{name, url}`; an instance's own variable (`env_var`) that two services answer keeps the first bare and suffixes the next (`_NAME`), any other variable keeps the first answer only (Go added AWS_REGION and the like once), an empty value is skipped; the doze authority's four variables for a stack that serves a domain from it. An instance's own variable comes first, then the rest in name order | transcript `env`; `tests/env.rs` (the built doze started, the app not woken) |
| 18 | `sync [service] [-y] [--dry-run]` (Go sync.go) through the daemon's `plan` then `apply`: names first (exit 4); the change list first, as Go's `renderPlan` (a group per instance, `+`/`~`/`-` per object, the shared `--dry-run` drawing); no changes: "no changes — declared structure is up to date", exit 0, nothing asked (as Go); else "Apply these changes? [y/N]" on a terminal, exit 9 with no one to answer and no `-y`, "✓ sync complete". `--dry-run` is the list (`{dry_run, applies_with, plan}`, the other dry runs' JSON). With doze stopped the daemon is started for it and stopped after (as Go's own runtime was). Against a Go daemon (no `plan` op, "unknown op") it asks every time and `--dry-run` is refused (exit 1) with how to run the stack on this doze | transcripts `sync`, `sync-go` |
| 19 | `connect <name>` (new, proposal B6): the module's own client from `InstanceView.connect`, nothing module-specific in the CLI. The daemon is started when doze is stopped (as `env`); the instance is woken first, its progress on stderr as `up` shows it; then the client: the engine build's own program (`path`), else `program` on `PATH`, else each fallback on `PATH` (a name with a separator is never taken), said as "→ redis-cli on cache (127.0.0.1:6379) · valkey-cli is not installed here". Its secrets in its variables (`env`), never argv. doze exits with the client's code (DZ0710), as `run`. No client declared (or an app): exit 1 (DZ0709) pointing at `doze url`; none of its programs here: exit 1 (DZ0711); a name not declared: exit 4; `--json` refused (exit 2) | transcript `connect` (the fixture daemon, the host records the run); `doze-module tests/cli_connect.rs` (end to end: the CLI against an in-process daemon with the echo module, no `PATH`, the echo engine's own client run, a line comes back through doze's front) |
| 20 | **Better than Go:** while `run` and `connect` wait for their command, Ctrl-C is the command's (`doze_os::interrupt::pass_to_child`: a handler, not "ignore", so the command starts with the default). A database shell cancels its query and stays, and doze reports how it ended. Go's `doze run` died on Ctrl-C, leaving a command that stays without doze | `doze-os interrupt::tests::ctrl_c_passed_to_a_child_does_not_end_this_process` |
| 21 | doze-config's shape pass (what the CLI reads every doze.hcl with) has each `module.<name>.env.<VAR>` something reads, as "" (difference 11). Go's stand-in had an empty `env`, so such a read failed the shape pass: every CLI command on such a stack was exit 5 | `doze-config tests/differences.rs the_shape_pass_reads_a_modules_env_variables` |

## Session 11 (core-d1 7210ea01 merged: `migrate` as protocol call 25, the `lint` op)

| # | Behaviour | Held by |
|---|---|---|
| 22 | **The `__fetch` child and a dead address.** The finding "up hangs on a fresh home" was not the registry: modules came from the `file://` registry (`DOZE_REGISTRY`) as they should; the postgres *engine* comes from the engine mirror (`DOZE_MIRROR`, GitHub by default, as Go's). The child sat in SYN_SENT to one GitHub CDN address for most of a minute, because the 600 s download budget was also the connect's. Now a connection gets 10 s, shared by the host's addresses (ureq gives the first about half, then the next; curl `--connect-timeout`), as Go's dialer gave up after 30 s and tried the next address sooner. And the child says "downloading postgres 16.16.0 (aarch64-apple-darwin) from github.com…" on its stderr, which is now the daemon's log (it was /dev/null), so `up` relays it as Go's did; `up` showed nothing for 88 s before | `doze-fetch http::tests::a_host_that_never_answers_costs_the_connect_budget_not_the_downloads` (TEST-NET-1, nothing reaches a registry), `doze-cli tests/fetch_child.rs a_child_says_an_engine_download_on_its_log` |
| 23 | `wake`, `restart`, `reset`, `pin` and `task <name>` start the daemon when doze is stopped, as Go's did (`startDaemon`), under the stack lock and said on stderr ("started the doze daemon for doze reset…"). `reset` asks first (nothing is started for a declined reset). A dry run with doze stopped plans from doze.hcl, the daemon's start first, and starts nothing. `sleep`, `off`, `on` and `pin --off` do not start it (Go's did not) | `tests/up.rs a_verb_starts_the_daemon_when_doze_is_stopped` (the built doze: wake starts it, reset deletes the data folder; Go's `TestSetupRunsOncePerDataFolder`), transcript `config-down` (`wake --dry-run`), `exit_codes` |
| 24 | `doze task` lists doze.hcl's tasks (Go's `listTasks`): a manual one that never ran is "manual — never run", JSON `manual: true`; it no longer needs the daemon to know the names, and a doze.hcl that does not load is exit 5. A task name is checked against doze.hcl before anything starts (Go `taskNotFound`) | transcript `task` (Go's `TestAFailedTaskStopsWhatIsAfterIt`) |
| 25 | `sync` names the blocks of doze.hcl that differ from what the running daemon started with, and what applying them does (Go `renderBlocks`: "~ api app changed restarts", "+ mail service new starts when something uses it", "- old app removed stops; its data is kept", "1 block unchanged", "doze down && doze up applies this"). The daemon records `run/blocks.json` (Go's shape, `{name: {kind, type, fp}}`) from doze.hcl's shape once its lock is held; the CLI compares the shape now. Not against a Go daemon (its prints are made another way). Human output only, as Go's | `cmd::sync::tests::the_blocks_that_differ_are_named_with_what_applying_them_does`; Go's `TestSyncNamesTheBlocksThatDifferFromTheRunningDaemon` |
| 26 | `migrate <name>`: the module's own move of its data to the major doze.hcl declares, through the daemon's `migrate` call; the module's words printed ("✓ db migrated", then what it said), JSON `{name, result, restarted}`. Checked against doze.hcl first (exit 4; an app, service or task is exit 1). **Better than Go:** the stack may be up. When the block changed since the daemon started (`run/blocks.json`), the daemon is restarted first under the stack lock (what was awake wakes again, the instance being moved aside), as daemon.md's audit item 6 asks; with doze stopped it is started for the move and stopped after it (JSON `started`), as `sync` stops what it starts. What was awake before a restart wakes again *after* the move (before it, a dependant would wake the instance on the new major with the old data), and what a restart does not carry over (pins, `doze add` instances, off states) is said. Progress on stderr ("→ db migrating…" and the daemon log's download lines). `--dry-run` says the start or restart, then the move. A Go daemon (no `plan` op) is refused before anything is stopped: its `blocks.json` has Go's prints, never read as a reason to restart it | `doze-module tests/cli_migrate_lint.rs` (end to end with the echo module: its refusal, a task refused, a move with its words), transcripts `migrate`, `migrate-down`, `migrate-go` |
| 27 | `lint`'s module pass: the stack's daemon when it runs (the `lint` op on doze.hcl as on disk now), else the same check once in `doze __daemon --lint` (a child that serves nothing; its module processes are stopped before it answers: `Pool::stop_all`). Its warnings are lint's; a type the registry cannot be reached for and that is not on this machine is listed unchecked with why (exit 11); a mistake the modules find is exit 5 in the load's words | `cli_migrate_lint.rs` (passes; an edit to e's block after the daemon started; a bad reference is exit 5), transcript `lint` |
| 28 | Weight: `regex` without its `perf` engines (weight.md, "Session 11"): `doze` 8,607,968 B on x86_64 Linux (8,994,120 B before), 233 packages | weight.md |

## Independent audit (2026-11-15, fifth, session 11 at 4f951d14)

Fixed:

1. **Major:** the regex change made `fn2.regex3.07`'s verdict depend on the build set: Cargo turns `perf` back on when
   doze-desk (or another workspace crate) shares the build, so CI's workspace tests disagreed with `-p doze-hcl`. The
   known difference is back, doze-hcl's tests take `perf` as a dev-dependency (stable in every build), and hcl-values.md
   F22 says the shipped `doze` (no `perf`) matches Go there.
2. **Major:** lint's module pass ignored `--var`. The one-shot gets them as `DOZE_VAR_*`, and with `--var` given the
   running daemon (which has its own) is not asked.
3. **Major:** `migrate` against a Go daemon read Go's `blocks.json` (other prints), took every module block as changed,
   stopped the Go daemon and started a Rust one. Now a daemon without the `plan` op is refused first (transcript
   `migrate-go`).
4. **Major:** the restart woke dependants before the move. Now they wake after it, through the Asker (Ctrl-C works); what
   a restart drops is said; a start that fails after the stop says the stack is down.
5. `migrate`'s timeout is the longer of `--timeout` and 30 minutes; a daemon started for the move is stopped after it.
6. The one-shot's answer is its last stdout line. `ctl::blocks` has a round-trip test that also reads Go's shape.
   `Mirror::source` drops a `user:secret@`. `start_for`'s line no longer says "services wake on their first
   connection" (wrong for `wake` and `task`). Windows: the connect unit test and `up.rs` no longer assume `/bin/sh`
   or a control socket.

Open, with reasons:

- ~~No end-to-end test of the restart path.~~ Closed in session 12: `doze-module tests/migrate_restart.rs` (below).
- Block drift fingerprints doze.hcl's shape, where a module block's body is not evaluated: an edit inside a module block
  (a database, a role) is not "changed" in `sync`'s section, and `migrate` does not see it (its version is in the header,
  so a major change is seen). Go's daemon fingerprinted the full load. The fingerprint hashes `Debug` text, stable
  while those types hold no hash maps.
- ~~Lint maps every `ok:false` from the module pass to exit 5, also a registry error.~~ Closed in session 13 (row 31).
- A lint op that times out against a running daemon falls back to the one-shot while the daemon may still be writing
  doze.lock. (The one-shot's progress: closed in session 12.)
- Found while gating, for D1/E2: `doze-engine tests/tasks.rs a_stopped_task_is_asked_to_stop_and_ends_failed` failed
  once under a parallel load ("exited: signal: terminated"), then passed three times alone.
- Found in the real-daemon proof, for D1: the daemon's converge on start writes no `.doze/state.json` (Go's `up` saved
  it), so after `up` the `plan` op lists the module's objects as to be created though they exist (the proof's
  `sync --dry-run`: "+ role app, + database db" with the role there).

## Session 12

| # | Behaviour | Held by |
|---|---|---|
| 29 | `migrate`'s restart path with real processes: the stack runs echo engine 1 with `api` reading it; doze.hcl is edited to engine 2; `migrate --dry-run` plans the restart; `migrate` restarts the daemon (a new pid, the old one gone), the module moves the data (marker 1 to 2, the old major's build fetched for it), `api` is awake again; `down` leaves no process. The test runs itself as `doze __daemon` and `doze __fetch` (`harness = false`), with the echo module from a registry it signs and its engines from a `file://` mirror; nothing reaches the network. It asserts `api` awake after, not the order of its wake against the move (that is the code path's: `migrate::moved`) | `doze-module tests/migrate_restart.rs` |
| 30 | lint's one-shot passes its stderr to a person's terminal (not under `--json` or `-q`), so a download its `__fetch` children make is seen | by hand (`/private/tmp/claude-501/c2c/lintp`) |

## Session 13 (core-d1 11714a7a merged)

| # | Behaviour | Held by |
|---|---|---|
| 31 | lint's module pass with D1's `unchecked: [{type, why}]`: a type whose build could not be had (the registry unreachable or missing, a download or signature that failed) is not checked, said with its reason, exit 11; reasons that differ are said per type. A type not published at all is still the load's error (exit 5). JSON keeps Go's `unchecked` (the types) and `why`, and adds `reasons`. A `DOZE_REGISTRY` folder that is missing was exit 5 ("correct doze.hcl") and is now exit 11 with the reason | `cmd::lint::tests::each_unchecked_type_is_said_with_its_reason`; by hand with the real one-shot (`/private/tmp/claude-501/c2c/lint13/1/lint.log`) |

Not done (not cheap): drift that sees inside a module block's body needs the shape pass to keep a canonical encoding of
the block's evaluated attributes (doze-config), with references still unresolved there; listed as before.

## Open (session 11)

- `blocks.json` is written when the daemon starts; a block added or removed at run time (`doze add` against a running
  daemon) is not in it, so `sync` lists it as new until the next start. Go's daemon had the same record and no live add.
- A Go daemon's `blocks.json` is not compared (every block would read as changed); `sync` against a Go daemon shows no
  block section.
- Found in D1 while testing lint (for D1): an attribute the echo module does not declare (`colour = "red"`) passes alone,
  but makes `module.e.env` unknown to the blocks that read it, and lint then reports only that knock-on error.

## Open (session 10)

- `sync --dry-run` with doze stopped starts the daemon to ask for the plan (and stops it after). Starting it loads the
  modules, which can pin them in doze.lock; Go's offline dry run disarmed lock writes. A plan with no daemon needs the
  modules in the CLI's own process, which the owner's rule (the CLI is a client) argues against; listed.
- `connect` wakes the instance before it looks for the client: an instance whose client is nowhere is woken for
  nothing. Its engine build is only known after a start, so looking first would miss the build's own client.

## Independent audit (2026-11-15, fourth, at f25850db)

Fixed:

1. A command ended by a signal gave exit 1 and "exited with status 1"; now 128 + the signal (Ctrl-C: 130), as a shell
   says it (`doze_os::process::exit_code`).
2. `sync` cleared the socket and pid file of a daemon it could not identify or signal, orphaning it. Now it clears them
   only once the daemon is stopped, and otherwise says so (exit 1, `doze down`).
3. `connect` took the daemon's `path` if it was any file: now only an absolute, runnable one (else PATH and fallbacks).

Open, with reasons:

- A SIGINT sent to doze alone (not the terminal's group: an IDE's stop, `kill -INT`) is taken by `run`/`connect` and not
  passed on; the command does not hear it. Forwarding needs the child's pid, which `Host::run_env` does not expose yet.
- Ctrl-C at `sync`'s question with a daemon started for it ends doze before it stops that daemon (left running, as
  `env` leaves it). The `started` check is taken before the start, so a `doze up` in another terminal while the question
  waits would be stopped with it. Both want the start to say whether it spawned, and an answer read with Ctrl-C as "no".
- Go's AWS stand-in credentials in `env` (`AWS_ACCESS_KEY_ID` and the rest when an `AWS_ENDPOINT_URL*` is present) are
  the aws module's to answer in its `env`; the CLI does not add them.

## Oracle

- `down` against Go's daemon, recorded 2026-11-15 with `tools/doze-oracle/down.sh` (Go doze built from `rust-oracle` 57aef16,
  the Rust doze as `DOZE_BIN`): Go's `__daemon` started in a session of its own under a private `DOZE_HOME`, then the
  Rust `doze --json down` gave `{"stopped": true}`, the pid was gone, and neither the socket nor the pid file was left.
  The record it wrote (`last_down`, `config`, `name`) is the one Go reads.
- `up` against Go's daemon, recorded 2026-11-15 with `tools/doze-oracle/up.sh` (same Go build, the Rust doze as
  `DOZE_BIN`): the Rust `up web` booted the app through Go's `__daemon` (setsid, private `DOZE_HOME`); then Go's `up web`
  and the Rust `up web` against the same daemon gave identical stdout ("✓ web awake", "› up — supervised …") and
  stderr ("→ web waking…"); the Rust `down` stopped it and left nothing. The trust hint is not recorded there: an app
  with `domains` needs the machine's zone, which the oracle keeps off (Go's daemon refuses it with `DOZE_ZONE=off`).
- The record writer against Go's `projects.Update`: eight cases byte for byte (`rawbytes`: `\xff\xfe`, `\ud800`, `\udfff`
  and `<\xc3` in a newer doze's fields, written back as Go wrote them) (doze-projects `tests/write.rs`).

## Independent audit (2026-11-15)

Audited at fc84b793. The merge was found clean: both sides' functions, modules and features are kept, doze-journal and
doze-logstore lose no public item, and every CI job of both sides is there. The audit's findings, and what was done:

1. **Major:** a stale pid file made `down` SIGTERM an unrelated process. This was parity with Go, not better. Fixed: the
   identity check (row 1). Test: `a_stale_pid_file_naming_a_stranger_signals_nobody`.
2. **Major:** another user's process under the pid (EPERM) made every `down` fail, and printed `kill <pid>` as the fix.
   Fixed: it is treated as stale and cleared, and no `kill` advice is printed for a pid that was not identified. Test:
   `a_pid_file_of_another_users_process_is_stale_not_a_wall`.
3. `--dry-run` disagreed with the run. Fixed: both use one decision, a snapshot error is passed on rather than shown as
   "not running", and the leftovers are listed.
4. `cleared` is an addition to Go's JSON, and the exit codes changed (refusal 1, timeout 8). Now in rows 3 and 4.
5. **Suspicion, open:**
   - A daemon whose long-lived parent never reaps it stays a zombie. Closed by D1: `spawn_detached` reaps it on a thread.
   - `clear` after a stop could remove the files of a daemon that a concurrent `up` has just started. Closed: `down`
     and `up` take D1's stack lock (`run/doze.ctl.lock`).
6. Tests now cover the main cases, with SIGTERM asserted, and the 20 s timeout (row 3).
7. Windows: row 5, tested by `tests/down_windows.rs` (CI's Windows runners).
8. Inherited: `fuzz_porttrap` was never run in CI. It now runs in the fuzz job.

## Independent audit (2026-11-15, second, at 9ad5676b)

Fixed:

1. **Major:** a doze.hcl that sets `home` or `data_dir` puts the daemon's socket where the CLI does not look (stack.rs
   uses the default place), so `up` would wait 10 s and orphan the daemon. `up` and `run` now refuse that case with
   DZ0001 before anything is asked, remembered or started; following the config's folder is open (stack.rs).
2. **Major:** `main` read argv with `std::env::args`, which panics on a non-UTF-8 argument; now `args_os`.
3. A snapshot that fails no longer fails `up` (Go goes on without it).
4. The recorded `config` is cleaned, as Go's `filepath.Abs` (`-c ../x/doze.hcl`).
5. `up --dry-run` on a terminal plans the default answer (last time's set), as the run without input would.
6. Windows: `up` and `run` say the daemon cannot run there yet (no local socket, core plan W) instead of spawning one
   that fails.
7. The real-daemon tests stop their daemon by its pid whatever fails; the writer's test compares raw bytes.

Open, with reasons:

- The stack lock is advisory: a Go doze (which has none) is not excluded by it. Closed for the Rust CLI: row 12.
- `down` with a doze.hcl that does not load stops the stack (exit 0) where Go refused with exit 5: decided by the
  coordinator as a listed improvement (row 13), for the owner to veto.
- `doctor` and `bug` still find the state folder at the default place (they do not load the config).
- `run/doze.ctl.lock` and `run/doze.log` stay after `down` (the socket and pid file do not).

## Independent audit (2026-11-15, third, at f124cc25)

Fixed:

1. **Major:** `update` could stop every stack when the new doze is broken. It now runs `<new doze> version` first and stops
   nothing if that fails; after a stack whose new daemon did not start, the rest are left on the old doze. Writing the
   test for this found a real bug: the halt check was evaluated after the stack had already been handed over. Tests:
   `tests/handover.rs` `a_new_doze_that_does_not_run_stops_nothing`.
2. **Major:** `env` suffixed and reordered shared variables (AWS_REGION per aws instance). Only an instance's own variable
   is suffixed now; the rest keep the first answer; empty values are skipped.
3. **Major:** `sync` on a stopped stack left the daemon running. It is now stopped after. The exit 9 with no change list
   is listed (row 18).
4. A wake failure after a successful hand-over says `moved: true` with the failure in the detail.
5. `up` takes the lock after the question, not before. `down` waits for the pid file only while it is missing. The
   broken-config warning no longer claims the stack was stopped. The trust hint is not probed under `--json`. Host
   `run`'s doc comment is restored.

Open, with reasons:

- On Windows, `env`, `url` and `sync` with doze stopped are DZ0001: they start the daemon, which has no local socket
  there yet (core plan W). Go read the config offline. This lands with W.
- `down` waits on the stack lock without a bound while an `up` boots. A wedged boot is ended by Ctrl-C on that `up`.
- A relative `data_dir` resolves against the current folder (as Go). `update`'s hand-over only finds stacks under
  `<home>/projects`, so a stack whose state folder is moved is not handed over (it keeps the old doze until restarted).
- With a broken doze.hcl, `down` looks at the default state folder. A stack that moved it is not found there.
- The fuzz target checks a stable round trip, not each kept field against its source bytes.
- doze.hcl is read twice per command (`Stack::find`, then the command).
