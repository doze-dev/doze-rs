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

## Pending (each says so: DZ0001, exit 1)

- `env`, `url`: Go reads only the config, plus each module's `ConnString` (the plugin's plan). In Rust, a module's
  `env_var` and URL come from its plan (`doze-module-proto` `instance.rs`) through doze-module-host. The CLI must plan
  modules offline (cached build, hello, plan) or ask the daemon. The `InstanceView` on the wire carries no URL today.
- `run --env`: needs `env` (the modules' answers on the wire).
- `connect`: the client comes from the signed module's own hello (doze-fetch `meta.rs`), so it needs the module host.
- `sync` (`apply`), `migrate`, lint's module pass.

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
