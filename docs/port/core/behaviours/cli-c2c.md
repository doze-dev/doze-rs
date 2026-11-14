# C2c: the CLI's daemon commands (behaviour catalogue, in progress)

Branch `core-c2c`, stacked on `core-c2b` (#53), with `core-d1` (#50, draft, still being built) merged in. Distilled from Go
`cmd/doze/{up,down,env,run,sync,migrate,daemonctl,remember}.go`, `internal/daemonctl` and `internal/endpoints` at doze
23b73d6, and the CLI proposal (rules 3, 7, 8; `--dry-run` table; exit codes).

## What D1 gives today, and what it does not

- `doze-daemon` (#50) is a library: `Daemon::new(engine, labels, info).serve()` over control protocol 2. It answers
  `status`, `up`, `wake`/`boot`, `sleep`/`down`, `restart`, `task`, `pin`, `off`/`on`, `apply`, `destroy`, `logs` and the
  streams. `reset`, `add`, `remove`, `resources` and `admin` are refused.
- Not in #50 yet: the `__daemon` entry (config → module pool with F1's downloads and doze.lock → `to_stack` → engine →
  daemon), and `ctl` (spawn detached, wait for the socket, pid file, stop). Both are in D1's row of the plan. Until they
  land, the CLI cannot **start** a daemon. Every command that would start one says so (DZ0001, exit 1); none fakes it.

## Built

| # | Behaviour | Held by |
|---|---|---|
| 1 | `down` stops the daemon its pid file names, and only when that pid is still the daemon: a live process that leads its own group (the daemon runs in a session of its own; D1's `ctl` must keep that) and started no later than the pid file was written. It sends SIGTERM to that exact pid and waits up to 20 s (Go's budget). Go signalled whatever held the pid; a stale file naming a stranger, or another user's process (EPERM), now means "not running" and the file is cleared Go's result: `{"stopped": true}` and "✓ doze stopped", or `{"stopped": false}` and "doze is not running" | `tests/down.rs` (child processes play the daemon and a stranger; SIGTERM checked; pid 1), transcript `down` |
| 2 | Better than Go (rule 8): after the stop, no socket and no pid file are left. A leftover of a daemon that is already gone is removed, and the removal is said | `tests/down.rs` `leftovers_of_a_daemon_that_is_gone_are_removed_and_said` |
| 3 | A socket that answers with no daemon pid behind it is refused, with the reason (exit 1; Go said exit 0 and left it running). A daemon that does not stop is named with its pid (exit 8; Go said 1). | transcript `down-dry-run`; `tests/down.rs` `a_daemon_that_does_not_stop_is_named_with_its_pid_after_twenty_seconds` (a leader that ignores SIGTERM: exit 8 after 20 s, nothing cleared) |
| 4 | `down --dry-run` makes the same decision as the run: what is awake (from the snapshot) and the daemon stopping with its pid, or the leftovers it would remove, or the same refusal. It signals nothing. JSON adds `cleared` (leftovers removed) to Go's `{"stopped"}`, left out when empty | transcripts `down`, `down-dry-run` |
| 5 | On Windows a daemon is never identified (no process groups), so `down` clears leftovers and says "not running"; stopping waits for the named-pipe daemon (core plan W). | `tests/down_windows.rs` (a live pid in the file is never signalled; the file is cleared) |

## Pending (each says so: DZ0001, exit 1)

- `up`: needs D1's `__daemon` and `ctl` to start a daemon. Against a running one it is `wake` over the closure (C1's
  path) plus Go's `remember` choice (`--last`, `--all`, the question). Next to build: the target choice and
  `remember.go`'s state, then the `wake` loop. Then spawn, once D1 has `__daemon`.
- `down` does not yet remember what was awake for the next `up` (Go's `rememberDown`). That comes with `up`'s choice.
- `env`, `url`: Go reads only the config, plus each module's `ConnString` (the plugin's plan). In Rust, a module's
  `env_var` and URL come from its plan (`doze-module-proto` `instance.rs`) through doze-module-host. The CLI must plan
  modules offline (cached build, hello, plan) or ask the daemon. The `InstanceView` on the wire carries no URL today.
- `run`: needs `up`'s daemon start; `--env` needs `env`.
- `connect`: the client comes from the signed module's own hello (doze-fetch `meta.rs`), so it needs the module host.
- `sync` (`apply`), `migrate`, lint's module pass, `open --console` (the `web_url` is on the wire), `update`'s
  hand-over (stop the old daemon, start the new one: needs `ctl`).

## Oracle

- `down` against Go's daemon is not yet recorded. The pid file format is Go's (first line the pid), and so are the
  socket and the run directory, so the Rust `down` should stop a Go daemon. The next step is to record it through
  `rust-oracle`'s `DOZE_BIN`.

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
   - A daemon whose long-lived parent never reaps it stays a zombie, which `kill(pid, 0)` still sees. D1's `ctl` must
     detach it (double fork or reap).
   - `clear` after a stop could remove the files of a daemon that a concurrent `up` has just started. This needs the
     stack lock that `up` will take.
6. Tests now cover the main cases, with SIGTERM asserted, and the 20 s timeout (row 3).
7. Windows: row 5, tested by `tests/down_windows.rs` (CI's Windows runners).
8. Inherited: `fuzz_porttrap` was never run in CI. It now runs in the fuzz job.
