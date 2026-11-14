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
| 1 | `down` stops the daemon its pid file names. It sends SIGTERM to that exact pid and waits up to 20 s (Go's budget). Go's result: `{"stopped": true}` and "✓ doze stopped", or `{"stopped": false}` and "doze is not running" | `tests/down.rs` (a real child process plays the daemon), transcript `down` |
| 2 | Better than Go (rule 8): after the stop, no socket and no pid file are left. A leftover of a daemon that is already gone is removed, and the removal is said | `tests/down.rs` `leftovers_of_a_daemon_that_is_gone_are_removed_and_said` |
| 3 | A socket that answers with no live pid behind it is refused, and the reason is named (exit 1). It is never reported as stopped. A daemon that does not stop is named with its pid (exit 8) | `cmd/down.rs` |
| 4 | `down --dry-run` lists what is awake (each would sleep, its data kept), then the daemon stopping. It asks nothing. With the daemon down it says so | transcripts `down`, `down-dry-run` |
| 5 | On Windows, stopping by pid is refused by doze-os. `down` says it waits for the named-pipe daemon (core plan W) | `cmd/down.rs` |

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
