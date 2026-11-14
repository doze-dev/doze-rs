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
| L13 | Every decision is the sans-IO `machine`'s (PLAN.md 2.2), checked under arbitrary interleavings of wakes, holds, registers, exits, stops, restart timers, reaps, pins and the shutdown: one start at a time; nothing starts, restarts or registers after the shutdown, which ends quiet; a restart fires only from a timer no stop, newer start, newer timer or shutdown cancelled (the cancelled timers are kept and fired too; breaking the timer check fails it at once); at most one copy of an instance runs (units and units being stopped); holds add up; the reaper never stops a pinned instance; a running instance has a unit, stopped at most once; a crashing instance is within its budget and has its restart pending (a live timer, a restart wake still owed, which the test plays as the engine does, or a fresh start in flight); a failed instance has no restart pending; a restart owed keeps the stack from being quiet; a crash during `on_ready` (a rerun or the first start's) cancels the start, its waiters hear the crash, and its restart stays, even when the step reports after the cancel; a stop between a restart timer and its wake wins | PLAN.md 2.2 invariants | `tests/machine_props.rs` (3,000 sequences; found two bugs, fixed), `tests/machine_races.rs` (audits 3 and 4: the races, a late `on_ready` report, `quiet` with a restart owed), `tests/lifecycle_props.rs` |

## Wake, holds, stop (`Engine`)

| # | Behaviour | Go | Rust test |
|---|---|---|---|
| W1 | Concurrent wakes of a cold instance share one start (singleflight) | `bootCond` | `tests/wake.rs concurrent_wakes_of_a_cold_service_share_one_start` |
| W2 | A wake wakes its needs first, one after another in declared order, each to health, holding each (`Acquire`) while it runs; journal causes `dependency` naming the dependant | `bootDeps`, `TestDependencyBootAndHold` | `…a_wake_starts_needs_first_holds_them_and_releases_them_on_stop` |
| W3 | `Lazy` needs are wired, not woken nor held; before a need ever ran its address is what it declares (a command's port; a module's `Planner::endpoint`) | `bootDeps`, `runtime.go` dependency endpoints | `tests/health_and_needs.rs a_lazy_need_is_wired_with_its_declared_address_and_not_woken` |
| W4 | A failed need fails the start: "booting dependency \"x\": …", needs already held are released | `bootDeps` | `tests/supervision.rs a_start_that_fails_releases_its_needs_and_says_why` (release path) |
| W5 | Holders: dependants sorted by name (a starting one counts, D16), a pin, client connections (holds minus dependants); a dependant that stopped no longer holds | `TestHolders` | `tests/catalogue.rs holders_are_dependants_sorted_then_the_pin_then_connections`, `tests/wake.rs` |
| W6 | Stop: cancels a pending restart, resets the budget, runs `stop` steps (each within 10 s; when they succeed the command gets 10 s to exit, when one fails or overruns the signals follow at once) or the signals, releases held needs, removes the pidfile, reaped, journal `sleep` with its cause; a stop during a start wins (D8) | `Stop`, `TestStopCancelsPendingRestart` | `tests/supervision.rs a_stop_cancels_a_pending_restart`, `a_stop_takes_the_whole_tree`; `tests/leftovers.rs stop_steps_run_in_place_of_the_signals`, `a_stop_during_a_start_wins`; `tests/catalogue.rs a_hanging_stop_step_is_cut_at_ten_seconds_and_the_patience_is_ten_seconds`, `sleeps_are_journalled_with_their_cause`; `tests/machine_cases.rs` |
| W7 | StopAll: parallel, repeated until nothing runs and no restart is pending; the shutdown cancels a pending restart; a new start supersedes one | `StopAll`, `TestStopAllCancelsPendingRestart`, `TestScheduleRestartSupersedes` | `tests/leftovers.rs the_shutdown_cancels_a_pending_restart`; `tests/machine_cases.rs` (all four); `tests/wake.rs` shutdown paths |
| W8 | Off stops and refuses every start with "<n> is off: `doze on <n>` puts it back into this run"; on starts nothing | `SetOff` | `tests/wake.rs off_refuses_every_start_until_on` |
| W9 | Undeclared: "instance \"x\" is not declared in doze.hcl" (not-found class); after shutdown: "daemon is shutting down; not booting \"x\"" | `bootLocked`, `bootCond` | `…off_refuses_every_start_until_on` |
| W10 | Reaper tick = idle/10 within 250 ms–1 s; stops with cause `idle` (journal `sleep`, cause `idle`) | `RunReaper` | `reaper::tests::the_tick_follows_gos_rule`, `tests/wake.rs the_reaper_puts_an_idle_service_to_sleep_but_not_a_pinned_or_held_one`, `tests/catalogue.rs sleeps_are_journalled_with_their_cause` |
| W11 | Health prober every 3 s for supervised running commands, each probe within 2 s: a repeatable check decides; no check or a `log_line` one records healthy; it records only, never restarts | `RunHealthProber`, `CheckHealth` | `tests/health_and_needs.rs the_prober_records_each_running_commands_health` |
| W12 | Pin toggles and journals `pin` `on`/`off` by command | `ToggleKeepAwake` | `tests/catalogue.rs a_pin_toggle_is_journalled_on_then_off`, `tests/wake.rs` (reaper test) |
| W13 | A need woken with `When::Started` is waited for until its processes run, not until its `on_ready` ran (a start begun that way runs no `on_ready`; a `Healthy` wake joining it returns when it registers), as Go's `bootCond` | `bootCond` | `tests/machine_cases.rs`; code reading for the joining wake |

## Processes and readiness

| # | Behaviour | Go | Rust test |
|---|---|---|---|
| P1 | Plan processes start in `after` order (depth first, declaration order); unknown and cycle errors | `TestOrderSpecs*` | `exec::tests::*` |
| P2 | Each process gated on its check: probe at once, back off 10→100 ms within the budget; no check: alive after 750 ms; "exited during startup", "exited immediately", "did not become ready within <d>" (a timeout is the `Timeout` fault); the error quotes this run's last 200 log lines; a `log_line` check reads this run only | `TestWaitReady*`, `TestProbe*`, `recentLogs` | `ready::tests::*` (a `log_line` probe reads at most the log's last 200 KiB, from a whole line), `tests/supervision.rs a_log_check_waits_for_this_runs_line`, `tests/catalogue.rs a_failed_start_quotes_this_runs_last_200_lines` |
| P3 | Hooks run after a process is ready, output into the log; a failed hook fails the start with "<name> hook \"<line>\": exit status N: <its output>" | `executePlan` | `tests/leftovers.rs hooks_run_after_readiness_and_a_failure_says_why` |
| P4 | A unit of several processes: any exit stops the rest gracefully, each as its plan says, in reverse order; every process's whole tree goes with it | `Composite`, `TestACommandThatExitsTakesItsChildrenWithIt` | `tests/units.rs when_one_process_exits_the_others_are_stopped_gracefully`; `tests/supervision.rs a_stop_takes_the_whole_tree` |
| P5 | A fronted command (port, not direct) gets a fresh private port at every start as `$PORT`, `$HOST=127.0.0.1`; `$DATA` always; `env` over `env_file` over the inherited environment | `TestAProxiedAppListensOnItsPrivatePort`, `TestADirectAppBindsItsOwnPort`, `TestMergedEnvPrecedence` | `process::tests::*` |
| P6 | Default check: `tcp` on its port; a portless worker gets the liveness check; `log("text")` matches the text literally | `readySpec` | `process::tests::checks_translate_with_the_port` |
| P7 | A fronted command sleeps by default (stack time), others stay on; `sleep` decides last | `TestAFrontedAppSleepsByDefault`, `TestModuleSleepSetsItsIdlePolicy` | `process::tests::a_fronted_app_sleeps_by_default_and_sleep_decides_last` |
| P8 | Missing `dir` or `env_file` is said before the start, in Go's words, after `setup` ran (so `setup` may write the `env_file`) | `Plan`, `envFileMissing`, `bootLocked` order | `process::tests::a_missing_dir_or_env_file_is_said_before_the_start`; `tests/catalogue.rs setup_may_write_the_env_file_the_start_then_reads` |
| P9 | Commands are asked with SIGTERM (Windows: CTRL_BREAK), killed after the grace; a module's `Interrupt` stop is Go's database sequence: SIGINT up to 5 s, SIGQUIT 3 s, then the kill | `TermFirst`, `supervisor.go` stop stages | `doze-os` `tests/tree.rs` (`an_unheeded_interrupt_is_followed_by_quit_then_the_kill`), `tests/supervision.rs` |
| P10 | `setup` runs once per data folder (marker `.doze-setup` = sha256 hex of the lines joined by "\n", plus "\n"); a failure taints and fails the start; a failed step says "<moment> step \"<line>\" failed: exit status N" | `setupNeeded`, `runSteps` | `steps::tests::the_marker_holds_gos_hash_of_the_lines`, `tests/supervision.rs setup_runs_once…`, `tests/catalogue.rs a_failed_setup_taints_and_runs_again_next_time` |
| P12 | `ready = tcp(…)`/`http(…)` needs a `port` (Go's third `BindPort` refusal); a fronted command may not set `PORT` or `HOST` in `env` (it would listen where doze is not forwarding): the engine refuses the block when it is built, in Go's words, naming `direct = true` as the way out; a portless or direct one may | `TestAProxiedAppMayNotSetItsOwnPort` | `process::tests::a_fronted_app_may_not_choose_its_own_port` |
| P13 | `sleep = "<duration>"` needs a fronted port (only a connection through doze wakes it): refused at build for a portless or direct command; `sleep = "never"` is fine | `TestSleepNeedsDozeToHoldThePort` | `process::tests::only_a_fronted_app_may_sleep` |
| P14 | doze's own variables (`Builder::extra_env`: the CA bundle, filled by H4) reach every process of a plan, steps and hooks, on top of the inherited environment; a variable the block chose wins, one that only repeats what doze inherited is replaced; an exec check runs in doze's own environment, as Go's `sh -c` | `TestExtraEnvReachesSpecsTasksAndStepsAndKeepsTheInheritedEnvironment`, `TestAProcessesOwnVariableWinsOverDozesAndAnInheritedOneDoesNot` | `process::tests::a_chosen_variable_wins_over_dozes_and_an_inherited_one_does_not`, `tests/catalogue.rs dozes_own_variables_reach_steps_and_the_process` |
| P11 | A failed `on_ready` leaves the command running but the instance failed; the next wake runs only `on_ready` again | `bootLocked`, `rerunOnReady` | `tests/supervision.rs setup_runs_once_per_data_folder_and_a_failed_on_ready_runs_again_alone` |

## Nothing left behind

Every process the engine starts is a doze-os tree, recorded in the pidfiles from the moment it is spawned: the plan's
processes, and each short command while it runs (`setup`, `on_ready` and `stop` steps, hooks, exec probes, a module's
declared stop command). Layout: `run/backend-<name>.pid` (Go's file: the first process), and `run/backend-<name>.pids/`
with `<n>.pid` for the plan's other processes and `run-<pid>.pid` for a short command. Each record carries the start
time the system reports for the process right after its spawn (the sweep reads the same source, so a clock step moves
both), and the sweep kills a recorded pid only while it is still that process. The sweep runs only while the engine is
quiet, under its lock (`Engine::reconcile` refuses otherwise).

Open from the third audit, for a later pass: on Linux a record's start time is `btime + ticks/hz`, which a clock step or suspend moves (the raw tick count would not); macOS reads start times by running `ps` (one syscall, `proc_pidinfo`, would do); a stop during `setup` kills the step, so the next start runs `setup` again on what it left (Go let the start finish); the host-killed tests hang rather than fail when a record never appears; the health prober probes one instance after another; the Linux spawner falls back to the calling thread without a word if its thread cannot start.

Limits, known and accepted: a process that leaves its process group (`setsid`, `setpgid`) escapes both the stop and the
sweep on Unix, as it escaped Go's; between the sweep's identity check and its kill there is a window of microseconds in
which the pid could be reused (Linux could close it with a pidfd; the odds do not justify it in E1); an `Engine` dropped
without `shutdown` keeps its processes until the runtime ends (its tasks hold it), so the daemon always shuts down.

| # | Behaviour | Rust test |
|---|---|---|
| N1 | After a stop: the leader and its grandchildren are gone (exact pids) | `tests/supervision.rs a_stop_takes_the_whole_tree` |
| N2 | After the host is killed outright, while running, while a start waits for readiness, and during a `setup` step: Linux PDEATHSIG and the Windows Job object end the leaders (shown on their own, before any sweep, at all three moments), and the next engine's sweep (macOS; Linux grandchildren) leaves nothing and no record | `tests/leftovers.rs the_host_killed_while_running`, `…while_a_start_waits_for_readiness`, `…during_a_setup_step` |
| N3 | After a crash loop: every run's leader and grandchild gone by pid, no pidfile | `tests/leftovers.rs a_crash_loop_leaves_no_process_by_pid`, `tests/supervision.rs a_crash_loop_restarts_then_gives_up_and_leaves_nothing` |
| N4 | After shutdown every test checks its pids are gone; a start that registers after the shutdown began is stopped at once | every real-process test; `tests/machine_props.rs` invariant 2 |
| N5 | A recorded pid that now belongs to another process is never killed; a Go pidfile (no start time) is swept when its pid leads its own group and started at most 15 minutes before the file was written (D3), and left alone otherwise | `pidfile::tests::a_live_pid_is_killed_only_while_it_is_the_recorded_process`, `tests/sweep.rs a_go_record_is_swept_when_it_still_names_a_tree_leader`, `tests/sweep.rs a_go_record_is_swept_up_to_15_minutes_after_the_spawn`, `tests/sweep.rs the_sweep_refuses_once_something_runs` |
| N6 | A module's declared stop command is recorded while it runs; when it overruns, it and what it started go, and the process is killed after the grace | `tests/leftovers.rs a_stop_command_is_recorded_while_it_runs` |
| N7 | What an exited leader left in its tree goes when the tree is waited for or dropped: the group is signalled before the leader is reaped (by code reading: while the leader is unreaped its group id cannot be reused); a tree whose leader is reaped is never signalled again by a unit's stop or kill. The wait for an exit wakes on SIGCHLD, not a timer: 20 idle trees cost 6 context switches in 10 s (1,192 when it polled) | doze-os `tests/tree.rs what_an_exited_leader_left_goes_on_drop_and_on_wait` |
| N8 | Linux: every tree is forked by one thread that lives as long as doze, so a tree spawned from a thread that exits (an idle blocking thread) does not get the parent-death signal; Windows: a tree starts suspended, joins its Job object, then runs, so nothing it starts escapes the job | doze-os `tests/tree.rs a_tree_outlives_the_thread_that_asked_for_it` (Linux), the Windows `os` jobs |

## Where Rust differs from Go, on purpose

| # | Go | Rust | Why |
|---|---|---|---|
| D1 | `run`, steps, hooks and `exec(…)` checks go through `sh -c` | argument lists, never a shell; a bare program is looked up on the `PATH` the command is given | modules plan D14 (a string is split into words by doze-config) |
| D2 | A stop that cancels a pending restart leaves the instance showing `crashing … restarting in 2s` for a restart that never comes | it is `reaped` | the state must not lie |
| D3 | The sweep kills a recorded pid if that pid is alive | only while it is still the recorded process (start time; for Go's records, a group leader started at most 15 minutes before the record, which Go wrote once the start was ready), and also when the group it led still has members; never on Windows | a reused pid must never be killed; Linux PDEATHSIG reaches only the leader; the Job object already ended every tree |
| D4 | A plan of several processes records only the first pid; steps, hooks and stop commands are not recorded | every process and every short command is recorded, a module's stop command included (layout above) | each module process is its own tree, and a daemon killed during a step must leave nothing |
| D5 | An HTTP check follows redirects to the final status | a 3xx counts as ready | no HTTP client in the engine; a redirect means something answers |
| D6 | Logs go to the log store with stream tags (`s:setup`, `s:hook`) | closed in E2: the same store and tags (e2.md G7) | E1 wrote `run/<name>.log` until the store came |
| D7 | A held need's count drops to zero when the need itself stops, while its dependant still runs | the dependant's hold survives the need's stop | the count must say who holds it; zeroing it let the reaper and `holders` lie (found by the machine proptest) |
| D8 | A stop or `off` during a start is lost: the start completes and the instance runs | the start is cancelled at once wherever it is (needs, `setup`, readiness, hooks, `on_ready`): what it started is killed with its trees, and it is `reaped`, not `failed`; a start that registered before the cancel landed is stopped at once (`tests/leftovers.rs a_stop_during_a_start_wins`, `tests/machine_cases.rs`) | a stop must win, and `off` must not lie; nothing failed |
| D15 | A wake during a stop starts a second copy beside the one being stopped | it waits until the old run is gone (`Decision::Busy`); the proptest asserts one copy per instance (`tests/leftovers.rs a_wake_during_a_stop_waits_for_the_old_run`) | two copies on one data folder, and an old copy whose record was overwritten, are what "nothing left behind" forbids |
| D9 | `booting` keeps the last run's pid, backend and health | `booting` clears them | they describe a process that is gone; the proptest showed a stale pid under `booting` |
| D10 | A crash publishes `reaped`, then the restart count, then `crashing` | one transition per decision (`crashing` with its count) | intermediate states nobody acts on are noise; the version still moves |
| D11 | An invalid `log_line` regex counts as "not yet" until the timeout | the start fails at once, naming the regex | waiting a minute for a typo helps nobody |
| D12 | `ToggleKeepAwake` on an undeclared name creates a placeholder | `not declared` (the not-found fault) | a typo must not pin a ghost |
| D13 | `sh -c` steps may leave background processes running | whatever a step, hook or probe started is killed when it ends | "nothing left behind" holds for steps as for processes |
| D14 | Steps and hooks run under the boot context, with no limit of their own | a `setup`/`on_ready` step or hook gets 10 min, a `stop` step 10 s, a module's plan 10 min; the shutdown waits at most 2 min | a hung step must not hang every waiter and the shutdown |
| D16 | `holders` lists only running dependants | a dependant still starting is listed too (it holds the need, which is why the need is not reaped) | the list must say who keeps it awake |
| D17 | Sweeping leftovers is the daemon's business, at its start | `Engine::reconcile` refuses unless the engine is quiet, and sweeps under its lock | called late, it would kill the engine's own processes, whose start times match |
| D18 | PLAN.md 2.2 sketched one actor per stack with a `DelayQueue` | a sans-IO `machine` behind one mutex, with spawned tasks for starts, stops and restart timers (a timer that a stop or newer start cancelled is ignored when it fires) | the invariants are the machine's and are proptested; the shell only runs effects |
| D19 | `sleep = "10m"` is quoted as written in the refusal | quoted as Go formats a duration (`10m0s`) | the engine gets a duration, not the text; H4 can pass the text if it matters |

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
