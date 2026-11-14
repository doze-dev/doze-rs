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
| L6 | Reaped/failed/crashing/done clear pid, backend, times, health; taint and pin survive; a new start clears the error, not the taint (see D9 for `booting`, D7 for holds) | `TestMarkReapedClears`, `TestTaintPersistsUntilCleared` | `…reaped_clears_what_describes_a_process_and_keeps_the_rest`, proptest |
| L7 | Snapshot sorted by name | `TestSnapshotSorted` | `…snapshot_is_sorted` |
| L8 | The transition feed ignores connection churn; state, health, error, taint, restarts, pin, off are transitions; a version moves on every change (see D10: one transition per decision) | `TestSubscribeEmitsOnTransitions`, `TestSubscribeIgnoresConnChurn`, `TestRestartCountAndPinEmit` | `…transitions_ignore_connection_churn` |
| L9 | Crash under a policy: `crashing` with "<exit>; restarting (n/max) in <go duration>", backoff from base doubling to 30 s; past the budget `failed` "<exit>; gave up after max restarts" | `TestCrashLoopIsCrashingThenFailed`, `TestBackoffFor` | `…a_crash_loop_is_crashing_then_failed`, `restart::tests::*`, `tests/supervision.rs a_crash_loop_restarts_then_gives_up_and_leaves_nothing` |
| L10 | A process up ≥ 60 s before crashing starts the budget over | `restartStabilityWindow` | `…a_long_life_resets_the_budget` |
| L11 | No policy: a supervised process that exited non-zero is `failed` "<exit>; not restarted"; exit 0 is reaped with "exited with status 0 and was not restarted"; a backend is `failed` "exited unexpectedly; it wakes again on the next connection" | `runtime.watch` | `…exits_without_a_restart_say_why` |
| L12 | Nothing restarts while shutting down | `BeginShutdown` | `…nothing_restarts_while_shutting_down` |
| L13 | Every decision is the sans-IO `machine`'s (PLAN.md 2.2), checked under arbitrary interleavings of wakes, holds, registers, exits, stops, restart timers, reaps, pins and the shutdown: one start at a time; nothing starts, restarts or registers after the shutdown, which ends quiet; a restart fires only from a timer no stop cancelled; holds add up; the reaper never stops a pinned instance; a running instance has a unit, stopped at most once; a crashing instance is within its budget | PLAN.md 2.2 invariants | `tests/machine_props.rs` (3,000 sequences; found two bugs, fixed), `tests/lifecycle_props.rs` |

## Wake, holds, stop (`Engine`)

| # | Behaviour | Go | Rust test |
|---|---|---|---|
| W1 | Concurrent wakes of a cold instance share one start (singleflight) | `bootCond` | `tests/wake.rs concurrent_wakes_of_a_cold_service_share_one_start` |
| W2 | A wake wakes its needs first, one after another in declared order, each to health, holding each (`Acquire`) while it runs; journal causes `dependency` naming the dependant | `bootDeps`, `TestDependencyBootAndHold` | `…a_wake_starts_needs_first_holds_them_and_releases_them_on_stop` |
| W3 | `Lazy` needs are wired, not woken nor held; before a need ever ran its address is what it declares (a command's port; a module's `Planner::endpoint`) | `bootDeps`, `runtime.go` dependency endpoints | `tests/health_and_needs.rs a_lazy_need_is_wired_with_its_declared_address_and_not_woken` |
| W4 | A failed need fails the start: "booting dependency \"x\": …", needs already held are released | `bootDeps` | `tests/supervision.rs a_start_that_fails_releases_its_needs_and_says_why` (release path) |
| W5 | Holders: running dependants (sorted), a pin, client connections (holds minus dependants) | `TestHolders` | `tests/wake.rs` (both tests) |
| W6 | Stop: cancels a pending restart, resets the budget, runs `stop` steps (each within 10 s, then waits 10 s for an exit) or the signals, releases held needs, removes the pidfile, reaped, journal `sleep`; a stop during a start wins (D8) | `Stop`, `TestStopCancelsPendingRestart` | `tests/supervision.rs a_stop_cancels_a_pending_restart`, `a_stop_takes_the_whole_tree`; `tests/leftovers.rs stop_steps_run_in_place_of_the_signals`, `a_stop_during_a_start_wins`; `tests/machine_cases.rs` |
| W7 | StopAll: parallel, repeated until nothing runs and no restart is pending; the shutdown cancels a pending restart; a new start supersedes one | `StopAll`, `TestStopAllCancelsPendingRestart`, `TestScheduleRestartSupersedes` | `tests/leftovers.rs the_shutdown_cancels_a_pending_restart`; `tests/machine_cases.rs` (all four); `tests/wake.rs` shutdown paths |
| W8 | Off stops and refuses every start with "<n> is off: `doze on <n>` puts it back into this run"; on starts nothing | `SetOff` | `tests/wake.rs off_refuses_every_start_until_on` |
| W9 | Undeclared: "instance \"x\" is not declared in doze.hcl" (not-found class); after shutdown: "daemon is shutting down; not booting \"x\"" | `bootLocked`, `bootCond` | `…off_refuses_every_start_until_on` |
| W10 | Reaper tick = idle/10 within 250 ms–1 s; stops with cause `idle` | `RunReaper` | `reaper::tests::the_tick_follows_gos_rule`, `tests/wake.rs the_reaper_puts_an_idle_service_to_sleep_but_not_a_pinned_or_held_one` |
| W11 | Health prober every 3 s for supervised running commands, each probe within 2 s: a repeatable check decides; no check or a `log_line` one records healthy; it records only, never restarts | `RunHealthProber`, `CheckHealth` | `tests/health_and_needs.rs the_prober_records_each_running_commands_health` |
| W12 | Pin toggles and journals `pin` `on`/`off` by command | `ToggleKeepAwake` | `tests/wake.rs` (reaper test) |

## Processes and readiness

| # | Behaviour | Go | Rust test |
|---|---|---|---|
| P1 | Plan processes start in `after` order (depth first, declaration order); unknown and cycle errors | `TestOrderSpecs*` | `exec::tests::*` |
| P2 | Each process gated on its check: probe at once, back off 10→100 ms within the budget; no check: alive after 750 ms; "exited during startup", "exited immediately", "did not become ready within <d>" (a timeout is the `Timeout` fault); the error quotes this run's last 200 log lines; a `log_line` check reads this run only | `TestWaitReady*`, `TestProbe*`, `recentLogs` | `ready::tests::*`, `tests/supervision.rs a_log_check_waits_for_this_runs_line` |
| P3 | Hooks run after a process is ready, output into the log; a failed hook fails the start with "<name> hook \"<line>\": exit status N: <its output>" | `executePlan` | `tests/leftovers.rs hooks_run_after_readiness_and_a_failure_says_why` |
| P4 | A unit of several processes: any exit stops the rest gracefully, each as its plan says, in reverse order; every process's whole tree goes with it | `Composite`, `TestACommandThatExitsTakesItsChildrenWithIt` | `tests/units.rs when_one_process_exits_the_others_are_stopped_gracefully`; `tests/supervision.rs a_stop_takes_the_whole_tree` |
| P5 | A fronted command (port, not direct) gets a fresh private port at every start as `$PORT`, `$HOST=127.0.0.1`; `$DATA` always; `env` over `env_file` over the inherited environment | `TestAProxiedAppListensOnItsPrivatePort`, `TestADirectAppBindsItsOwnPort`, `TestMergedEnvPrecedence` | `process::tests::*` |
| P6 | Default check: `tcp` on its port; a portless worker gets the liveness check; `log("text")` matches the text literally | `readySpec` | `process::tests::checks_translate_with_the_port` |
| P7 | A fronted command sleeps by default (stack time), others stay on; `sleep` decides last | `TestAFrontedAppSleepsByDefault`, `TestModuleSleepSetsItsIdlePolicy` | `process::tests::a_fronted_app_sleeps_by_default_and_sleep_decides_last` |
| P8 | Missing `dir` or `env_file` is said before the start, in Go's words, after `setup` ran (so `setup` may write the `env_file`) | `Plan`, `envFileMissing`, `bootLocked` order | `process::tests::a_missing_dir_or_env_file_is_said_before_the_start`; order in `boot::prepare` |
| P9 | Commands are asked with SIGTERM (Windows: CTRL_BREAK), killed after the grace; a module's `Interrupt` stop is Go's database sequence: SIGINT up to 5 s, SIGQUIT 3 s, then the kill | `TermFirst`, `supervisor.go` stop stages | `doze-os` `tests/tree.rs` (`an_unheeded_interrupt_is_followed_by_quit_then_the_kill`), `tests/supervision.rs` |
| P10 | `setup` runs once per data folder (marker `.doze-setup` = sha256 hex of the lines joined by "\n", plus "\n"); a failure taints and fails the start; a failed step says "<moment> step \"<line>\" failed: exit status N" | `setupNeeded`, `runSteps` | `steps::tests::the_marker_holds_gos_hash_of_the_lines`, `tests/supervision.rs setup_runs_once…` |
| P11 | A failed `on_ready` leaves the command running but the instance failed; the next wake runs only `on_ready` again | `bootLocked`, `rerunOnReady` | `tests/supervision.rs setup_runs_once_per_data_folder_and_a_failed_on_ready_runs_again_alone` |

## Nothing left behind

Every process the engine starts is a doze-os tree, recorded in the pidfiles from the moment it is spawned: the plan's
processes, and each short command while it runs (`setup`, `on_ready` and `stop` steps, hooks, exec probes). Not yet: a
module's declared stop command (`StopHow::Command`) runs as a tree of its own but is not recorded (open, audit 2 M2). Layout: `run/backend-<name>.pid` (Go's file: the first process), and `run/backend-<name>.pids/` with
`<n>.pid` for the plan's other processes and `run-<pid>.pid` for a short command. Each record carries its spawn time,
and the sweep kills a recorded pid only while it is still that process.

| # | Behaviour | Rust test |
|---|---|---|
| N1 | After a stop: the leader and its grandchildren are gone (exact pids) | `tests/supervision.rs a_stop_takes_the_whole_tree` |
| N2 | After the host is killed outright, while running, while a start waits for readiness, and during a `setup` step: Linux PDEATHSIG and the Windows Job object end the leaders (shown on their own, before any sweep), and the next engine's sweep (macOS; Linux grandchildren) leaves nothing and no record | `tests/leftovers.rs the_host_killed_while_running`, `…while_a_start_waits_for_readiness`, `…during_a_setup_step` |
| N3 | After a crash loop: every run's leader and grandchild gone by pid, no pidfile | `tests/leftovers.rs a_crash_loop_leaves_no_process_by_pid`, `tests/supervision.rs a_crash_loop_restarts_then_gives_up_and_leaves_nothing` |
| N4 | After shutdown every test checks its pids are gone; a start that registers after the shutdown began is stopped at once | every real-process test; `tests/machine_props.rs` invariant 2 |
| N5 | A recorded pid that now belongs to another process is never killed; a Go pidfile (no spawn time) is not killed | `pidfile::tests::a_live_pid_is_killed_only_while_it_is_the_recorded_process` |

## Where Rust differs from Go, on purpose

| # | Go | Rust | Why |
|---|---|---|---|
| D1 | `run`, steps, hooks and `exec(…)` checks go through `sh -c` | argument lists, never a shell; a bare program is looked up on the `PATH` the command is given | modules plan D14 (a string is split into words by doze-config) |
| D2 | A stop that cancels a pending restart leaves the instance showing `crashing … restarting in 2s` for a restart that never comes | it is `reaped` | the state must not lie |
| D3 | The sweep kills a recorded pid if that pid is alive | only while it is still the recorded process (start time), and also when the group it led still has members; never on Windows | a reused pid must never be killed; Linux PDEATHSIG reaches only the leader; the Job object already ended every tree |
| D4 | A plan of several processes records only the first pid; steps and hooks are not recorded | every process and every short command is recorded (layout above) | each module process is its own tree, and a daemon killed during a step must leave nothing |
| D5 | An HTTP check follows redirects to the final status | a 3xx counts as ready | no HTTP client in the engine; a redirect means something answers |
| D6 | Logs go to the log store with stream tags (`s:setup`, `s:hook`) | E1 writes `run/<name>.log` (this run; the previous one is `<name>.prev.log`), untagged | the log store is E2; the calls (`stdio`, `line`, `tail`, `size`, `since`) stay |
| D7 | A held need's count drops to zero when the need itself stops, while its dependant still runs | the dependant's hold survives the need's stop | the count must say who holds it; zeroing it let the reaper and `holders` lie (found by the machine proptest) |
| D8 | A stop or `off` during a start is lost: the start completes and the instance runs | the start is cancelled at once wherever it is (needs, `setup`, readiness, hooks, `on_ready`): what it started is killed with its trees, and it is `reaped`, not `failed`; a start that registered before the cancel landed is stopped at once (`tests/leftovers.rs a_stop_during_a_start_wins`, `tests/machine_cases.rs`) | a stop must win, and `off` must not lie; nothing failed |
| D15 | A wake during a stop starts a second copy beside the one being stopped | it waits until the old run is gone (`Decision::Busy`); the proptest asserts one copy per instance (`tests/leftovers.rs a_wake_during_a_stop_waits_for_the_old_run`) | two copies on one data folder, and an old copy whose record was overwritten, are what "nothing left behind" forbids |
| D9 | `booting` keeps the last run's pid, backend and health | `booting` clears them | they describe a process that is gone; the proptest showed a stale pid under `booting` |
| D10 | A crash publishes `reaped`, then the restart count, then `crashing` | one transition per decision (`crashing` with its count) | intermediate states nobody acts on are noise; the version still moves |
| D11 | An invalid `log_line` regex counts as "not yet" until the timeout | the start fails at once, naming the regex | waiting a minute for a typo helps nobody |
| D12 | `ToggleKeepAwake` on an undeclared name creates a placeholder | `not declared` (the not-found fault) | a typo must not pin a ghost |
| D13 | `sh -c` steps may leave background processes running | whatever a step, hook or probe started is killed when it ends | "nothing left behind" holds for steps as for processes |
| D14 | Steps and hooks run under the boot context, with no limit of their own | a `setup`/`on_ready` step or hook gets 10 min, a `stop` step 10 s; the shutdown waits at most 2 min | a hung step must not hang every waiter and the shutdown |

## process-wrap, evaluated (PLAN.md §7)

`process-wrap` 10.0.1 (watchexec) wraps a tokio `Command` in layers: process group or session (Unix), Job object
(Windows, kill-on-close when asked), kill-on-drop, creation flags, sigmask reset. Against `doze-os::tree`:

- it has no parent-death signal (Linux `PR_SET_PDEATHSIG`), which N2 relies on;
- it does not send CTRL_BREAK to a console group, which is how a Windows tree is asked to stop (P9);
- the declared stops (D15: signal, interrupt with Go's stages, a command) would still be ours on top;
- it pulls in `nix` and `windows` (the workspace uses `rustix` and `windows-sys`), `futures` and `indexmap`.

`doze-os::tree` already covers what process-wrap does that doze needs, in under 400 lines with the W0 measurements
behind it. It stays.

Not in E1 (later phases, per PLAN.md): tasks and `inputs`, `watch`, the log store and journal writer, applied state and
`sync`/prune, converge and the converged marker, the module pool (E2); the port trap hint (E2); per-instance listeners
and wake on connect (D2).
