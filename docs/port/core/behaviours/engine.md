# Behaviours: the engine (phase E1)

Distilled from Go's `internal/registry`, `internal/runtime` (`runtime.go`, `plan.go`, `holders.go`, `steps.go`),
`internal/supervisor`, `internal/health` and `engine/process`, at doze `origin/main 23b73d6`. Each item names the Rust
test that holds it. "Go test" names the Go test the item comes from, where there is one.

## Lifecycle (sans-IO `lifecycle::Table`)

| # | Behaviour | Go | Rust test |
|---|---|---|---|
| L1 | State words are `reaped booting active idle crashing failed done`, byte for byte | `registry.go` | `state::tests::words_are_the_protocol_words` |
| L2 | A running instance with a hold is active; at zero holds it is idle and its idle time starts; never below zero | `TestConnLifecycle` | `lifecycle::tests::connections_move_between_active_and_idle` |
| L3 | Reapable = idle, zero holds, not pinned, idle for ≥ its own time else the stack's; neither set: never | `TestReapableOnlyAfterTimeout`, `TestKeepAwakeExemptsFromReaper`, `TestReapableNotWhenConnected` | `…reapable_only_after_the_idle_time_and_never_when_held_or_pinned`, proptest |
| L4 | Idle time is per instance | `TestIdlePolicyIsPerInstance` | `…idle_time_is_per_instance` |
| L5 | Always-on runs as active with nothing holding it and is never reaped | `TestAlwaysOnIsActiveWithNoConnections` | `…always_on_is_active_with_nothing_holding_it` |
| L6 | Reaped/failed/crashing/done clear pid, backend, holds, times, health; taint and pin survive; a new start clears the error, not the taint | `TestMarkReapedClears`, `TestTaintPersistsUntilCleared` | `…reaped_clears_what_describes_a_process_and_keeps_the_rest`, proptest |
| L7 | Snapshot sorted by name | `TestSnapshotSorted` | `…snapshot_is_sorted` |
| L8 | The transition feed ignores connection churn; state, health, error, taint, restarts, pin are transitions; a version moves on every change | `TestSubscribeEmitsOnTransitions`, `TestSubscribeIgnoresConnChurn`, `TestRestartCountAndPinEmit` | `…transitions_ignore_connection_churn` |
| L9 | Crash under a policy: `crashing` with "<exit>; restarting (n/max) in <go duration>", backoff from base doubling to 30 s; past the budget `failed` "<exit>; gave up after max restarts" | `TestCrashLoopIsCrashingThenFailed`, `TestBackoffFor` | `…a_crash_loop_is_crashing_then_failed`, `restart::tests::*`, `tests/supervision.rs a_crash_loop_restarts_then_gives_up_and_leaves_nothing` |
| L10 | A process up ≥ 60 s before crashing starts the budget over | `restartStabilityWindow` | `…a_long_life_resets_the_budget` |
| L11 | No policy: a supervised process that exited non-zero is `failed` "<exit>; not restarted"; exit 0 is reaped with "exited with status 0 and was not restarted"; a backend is `failed` "exited unexpectedly; it wakes again on the next connection" | `runtime.watch` | `…exits_without_a_restart_say_why` |
| L12 | Nothing restarts while shutting down | `BeginShutdown` | `…nothing_restarts_while_shutting_down` |
| L13 | `done` only from a completed run; restart attempt never exceeds its budget; idle implies no holds and may sleep | PLAN.md 2.2 invariants | `tests/lifecycle_props.rs` (2,000 random sequences) |

## Wake, holds, stop (`Engine`)

| # | Behaviour | Go | Rust test |
|---|---|---|---|
| W1 | Concurrent wakes of a cold instance share one start (singleflight) | `bootCond` | `tests/wake.rs concurrent_wakes_of_a_cold_service_share_one_start` |
| W2 | A wake wakes its needs first, one after another in declared order, each to health, holding each (`Acquire`) while it runs; journal causes `dependency` naming the dependant | `bootDeps`, `TestDependencyBootAndHold` | `…a_wake_starts_needs_first_holds_them_and_releases_them_on_stop` |
| W3 | `Lazy` needs are wired, not woken nor held | `bootDeps` | code (`boot::wake_needs`); exercised in D1/D2 |
| W4 | A failed need fails the start: "booting dependency \"x\": …", needs already held are released | `bootDeps` | `tests/supervision.rs a_start_that_fails_releases_its_needs_and_says_why` (release path) |
| W5 | Holders: running dependants (sorted), a pin, client connections (holds minus dependants) | `TestHolders` | `tests/wake.rs` (both tests) |
| W6 | Stop: cancels a pending restart, resets the budget, runs `stop` steps (then waits 10 s for an exit) or the signals, releases held needs, removes the pidfile, reaped, journal `sleep` | `Stop`, `TestStopCancelsPendingRestart` | `tests/supervision.rs a_stop_cancels_a_pending_restart`, `a_stop_takes_the_whole_tree` |
| W7 | StopAll: parallel, repeated until nothing runs and no restart is pending | `StopAll`, `TestStopAllCancelsPendingRestart` | `tests/wake.rs` shutdown paths |
| W8 | Off stops and refuses every start with "<n> is off: `doze on <n>` puts it back into this run"; on starts nothing | `SetOff` | `tests/wake.rs off_refuses_every_start_until_on` |
| W9 | Undeclared: "instance \"x\" is not declared in doze.hcl" (not-found class); after shutdown: "daemon is shutting down; not booting \"x\"" | `bootLocked`, `bootCond` | `…off_refuses_every_start_until_on` |
| W10 | Reaper tick = idle/10 within 250 ms–1 s; stops with cause `idle` | `RunReaper` | `reaper::tests::the_tick_follows_gos_rule`, `tests/wake.rs the_reaper_puts_an_idle_service_to_sleep_but_not_a_pinned_or_held_one` |
| W11 | Health prober every 3 s for supervised commands with a repeatable check (not `log_line`), records only | `RunHealthProber` | code (`reaper::probe_once`) |
| W12 | Pin toggles and journals `pin` `on`/`off` by command | `ToggleKeepAwake` | `tests/wake.rs` (reaper test) |

## Processes and readiness

| # | Behaviour | Go | Rust test |
|---|---|---|---|
| P1 | Plan processes start in `after` order (depth first, declaration order); unknown and cycle errors | `TestOrderSpecs*` | `exec::tests::*` |
| P2 | Each process gated on its check: probe at once, back off 10→100 ms within the budget; no check: alive after 750 ms; "exited during startup", "exited immediately", "did not become ready within <d>"; the error quotes the log | `TestWaitReady*`, `TestProbe*` | `ready::tests::*` |
| P3 | Hooks run after a process is ready; a failed hook fails the start | `executePlan` | code (`exec::gate`) |
| P4 | A unit of several processes: any exit stops the rest (reverse order); every process's whole tree goes with it | `Composite`, `TestACommandThatExitsTakesItsChildrenWithIt` | `exec`, `supervise`; `tests/supervision.rs a_stop_takes_the_whole_tree` |
| P5 | A fronted command (port, not direct) gets a fresh private port at every start as `$PORT`, `$HOST=127.0.0.1`; `$DATA` always; `env` over `env_file` over the inherited environment | `TestAProxiedAppListensOnItsPrivatePort`, `TestADirectAppBindsItsOwnPort`, `TestMergedEnvPrecedence` | `process::tests::*` |
| P6 | Default check: `tcp` on its port; a portless worker gets the liveness check; `log("text")` matches the text literally | `readySpec` | `process::tests::checks_translate_with_the_port` |
| P7 | A fronted command sleeps by default (stack time), others stay on; `sleep` decides last | `TestAFrontedAppSleepsByDefault`, `TestModuleSleepSetsItsIdlePolicy` | `process::tests::a_fronted_app_sleeps_by_default_and_sleep_decides_last` |
| P8 | Missing `dir` or `env_file` is said before the start, in Go's words | `Plan`, `envFileMissing` | `process::tests::a_missing_dir_or_env_file_is_said_before_the_start` |
| P9 | Commands are asked with SIGTERM (Windows: CTRL_BREAK), killed after the grace | `TermFirst`, supervisor | `doze-os` tree tests, `tests/supervision.rs` |
| P10 | `setup` runs once per data folder (marker `.doze-setup` = sha256 hex of the lines joined by "\n", plus "\n"); a failure taints and fails the start | `setupNeeded` | `steps::tests::the_marker_holds_gos_hash_of_the_lines`, `tests/supervision.rs setup_runs_once…` |
| P11 | A failed `on_ready` leaves the command running but the instance failed; the next wake runs only `on_ready` again | `bootLocked`, `rerunOnReady` | `tests/supervision.rs setup_runs_once_per_data_folder_and_a_failed_on_ready_runs_again_alone` |

## Nothing left behind

| # | Behaviour | Rust test |
|---|---|---|
| N1 | After a stop: the leader and its grandchildren are gone (exact pids) | `a_stop_takes_the_whole_tree` |
| N2 | After the host is killed outright: Linux PDEATHSIG / Windows Job object, and the next engine's sweep of `run/backend-<name>.pid` (macOS; Linux grandchildren whose leader died) leave nothing | `nothing_is_left_after_the_host_is_killed` |
| N3 | After a crash loop: no pidfile, nothing running | `a_crash_loop_restarts_then_gives_up_and_leaves_nothing` |
| N4 | After shutdown every test checks its pids are gone | every `tests/wake.rs` test |

## Where Rust differs from Go, on purpose

| # | Go | Rust | Why |
|---|---|---|---|
| D1 | `run`, steps, hooks and `exec(…)` checks go through `sh -c` | argument lists, never a shell | modules plan D14 (a string is split into words by doze-config) |
| D2 | A stop that cancels a pending restart leaves the instance showing `crashing … restarting in 2s` for a restart that never comes | it is `reaped` | the state must not lie |
| D3 | The sweep kills a recorded pid only if that pid is alive | also when the process group it led still has members | Linux PDEATHSIG reaches only the leader, so grandchildren outlived a killed daemon |
| D4 | A plan of several processes records only the first pid | `backend-<name>.<n>.pid` for the others (Go's sweep reads them too) | each module process is its own tree |
| D5 | An HTTP check follows redirects to the final status | a 3xx counts as ready | no HTTP client in the engine; a redirect means something answers |
| D6 | Logs go to the log store with stream tags | E1 writes `run/<name>.log` | the log store is E2; the three calls (`stdio`, `line`, `tail`) stay |

Not in E1 (later phases, per PLAN.md): tasks and `inputs`, `watch`, the log store and journal writer, applied state and
`sync`/prune, converge and the converged marker, the module pool (E2); the port trap hint (E2); per-instance listeners
and wake on connect (D2).
