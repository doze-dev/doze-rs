# CI health

The jobs of `.github/workflows/ci.yml`, what each guards, who owns it, and how it did on main over the 40 finished runs from
2026-11-12 11:41 to 2026-11-15 02:01 (run 43211347364 to 43665814939; 32 of the 40 red). "Owner" is the session whose crates the job
exercises and who fixes it when it goes red; a shared job names the core session.

Auto triggers (push and pull_request) come back when main has stayed green and every job has an owner. Until then CI is started by
hand: `gh workflow run ci --ref <branch>` (PR #62).

| job | guards | owner | red on main (of runs it ran in) | causes, and what fixed them |
|---|---|---|---|---|
| `check (ubuntu-24.04)` | fmt, clippy `-D warnings`, the workspace tests, the lean and TLS variants, the storage simulation, codegen, themes, size, fuzz-check, the catalogues, rustdoc | core (each failing test: its crate's session) | 32 of 40 | 11-12 to 11-13: kafka process tests (`console::unprotected_starts…`, `users::…`), not seen after 11-13 08:00; 11-13 to 11-14: rustdoc broken links (6 runs); 11-14: a port taken between probe and bind (`module_boot`, fixed in 10a6aa36 by handing out ports below the ephemeral range); 11-15: **`live_words` test ran `/bin/sh`, which is dash on Ubuntu and has no `$'…'`** (kafka, e2a4b289) — fixed here, the test runs bash |
| `check (macos-latest)` | the same on macOS | core | 31 of 40 | 11-12 to 11-13: `cargo test -p kafka-server -p doze-kafka --features tls` (17 runs, the TLS step before TLS was in the default build); rustdoc broken links (9 runs); one closed-port race in `probe::tests` (run 43501441380); 11-15: **rustdoc: `start_transactions` linked the private `super::alarm`** (kafka, fb8791ad) — fixed here. The probe test also fixed here (below) |
| `fuzz` | 30 s of every parser's fuzz target on nightly | core (each target: its crate's session) | 28 of 40 | 26 runs: the fuzz build itself failed (green since 11-14); 1 run: a recovery input that looped (kafka-storage, fixed: read on from the header's offset span); 11-15: **kafka-registry `codec`: an empty packed run decoded as `"c": []`, which wrote back as nothing and read as `{}`** (input `00 1a 00`) — fixed here in the Protobuf decoder, with a regression test; 1.67 M runs clean locally after |
| `weight` | doze-kafka and doze-aws release size, packages, idle memory (`xtask/weight-limits.txt`) | kafka (doze-kafka line), aws (doze-aws line) | 30 of 40 | every one a line set from an estimate that CI then measured over (binary over 7.8 MB; idle over 6,144 / 8,192 / 9,761 / 9,996 / 9,413 KB). Green since 11-14 11:23, when the lines became CI's measurements |
| `weight-lean` | the broker with no console: size (exact line), packages, idle | kafka | 31 of 40 | 23 runs over the 5.5 MB line, 3 over 5.2 MB, 4 over 5,263,296 B (lines from estimates or a measurement the next commit moved); 11-15: **over 5,274,120 B by 175,952 B** — the kafka efficiency pass (fix-correct, fix-idle, fix-idiom) grew the lean binary and 6985735d moved only the default line. Bisected and explained in weight.md ("The lean line after the efficiency pass"); the line is now 5,450,072 B |
| `weight-tls` (retired 11-14) | the TLS build's size and idle | kafka | 27 of 29 | the TLS build failed to build in CI, then size and package lines from estimates; retired when TLS joined the default build |
| `weight-tls-idle` | idle memory with a certificate loaded | kafka | 2 of 11 | idle over 10,610 KB (an estimate); the line became CI's measurement plus 5% on 11-14 |
| `supply-chain` | cargo-deny: advisories, bans, licences, sources | core | 16 of 40 | the action's container could not install the musl toolchain `rust-toolchain.toml` named (11-12); fixed the same day |
| `web` | the committed React bundle is what the sources build; typecheck, lint, unit tests | kafka | 3 of 17 | the committed bundle was stale (11-13); green since |
| `os` (5 targets) | doze-os, the module protocol and host, names, control, the CLI | core | macOS 2 of 27, others 0 | macOS: `a_client_is_handed_over_spliced_and_accounted`, the hand-off race, fixed by #60 (7f051dbc) |
| `modules` (3 targets) | each module boots doze-binaries' engine build | core (modules) | x86_64 1, arm64 1, macOS 0, of 26 | one temporal and one kvrocks acceptance start each (11-12, 11-13); not seen since |
| `windows` | the Windows build and tests | core | 0 of 40 | |
| `app` | the desktop app's macOS build and size | app session | 0 of 4 | |

## Fixed in main-green-2 (2026-11-15)

| failure | root cause | fix | caused by |
|---|---|---|---|
| `check (ubuntu)`: `live_words::tests::a_shell_reads_every_hostile_id_back_as_one_word_and_runs_none_of_it` | the test ran `/bin/sh`; on Ubuntu that is dash, which reads `$'line\nnext'` as `$` and a quoted string. `shell_word` names bash, zsh, ksh and POSIX 2024, so the code is right and the test was not | the test runs bash, as its neighbour does | kafka session, e2a4b289 |
| `check (macOS)`: rustdoc `private-intra-doc-links` | `start_transactions`' doc linked the private `super::alarm` | the doc names the module in plain code | kafka session, fb8791ad |
| `fuzz`: `kafka-registry` `codec.rs:40` | the Protobuf decoder made an empty packed run (`1a 00`) an empty array; protobuf's JSON mapping has an empty repeated field the same as an absent one, and the encoder writes nothing for it | an empty run adds nothing; `an_empty_packed_run_reads_as_the_field_left_out_and_round_trips` | kafka session (registry Phase 10b; found now by the fuzzer) |
| `weight-lean`: over 5,274,120 B | the efficiency pass's code (response sizing, idle chores' tasks, the group table), +3.3%, line not moved | line to 5,450,072 B, CI's measurement, with the bisection in weight.md | kafka session, merges 086e4b28, 0ee5cf74, 07c07a88 |
| `check (macOS)` once: `probe::tests::a_closed_port_is_refused…` | the "closed" port was a listener bound then dropped; a parallel test could take it | a port below the ephemeral range, checked by a refused connect. Under 32 test threads on macOS the old test failed 4 of 30 runs, the new one 0 of 40 | kafka session (console probe) |

## After merging the aws session's data control (78ba6190, 2026-11-15)

Run 43703449990 on dae5512c failed `weight` (doze-aws idle 10,548 KB over 10,526) and `weight-lean` (5,450,504 B, +432 B). Both are measured and
explained in weight.md ("Data control reaches main"); the lines moved to 11,075 KB and 5,450,504 B.

`modules (x86_64)` once (run 43720667194, PR #65): temporal `a_restart_keeps_its_data_and_the_app_sees_the_namespaces` got a retryable "Namespace orders is not found" from `workflow count` just after the restart. Ready is honest (temporal writes declared namespaces to SQLite before it serves and its frontend loads them before SERVING; a probe found the namespace countable at the moment of ready in 30 of 30 restarts), and temporal's registry reports any persistence error on a lookup miss as "not found"; the test now asks again on a retryable error for up to 15 s (branch `temporal-restart-flake`). Not reproduced on arm64 Linux: 0 of 208 suite runs before the fix, 0 of 120 after.

## ci-flakes-2 (2026-11-15)

**`os (macOS)`: `detach.rs` `a_detached_daemon_outlives_its_launcher_and_stops_an_engine_gracefully`** failed once (run 43728583994) with
an empty report. The test polls for the report file and reads it as soon as it exists; the helper's daemon wrote it with `fs::write`, which
creates the file empty and then writes, so a poll in that gap read "". Reproduced exactly by widening the gap (create, 300 ms, write: fails
at once with "could not stop its engine gently: "). The daemon now writes `<out>.tmp` and renames it into place; with the same 300 ms between
write and rename it passes. Under load on macOS (8 concurrent test processes, RUST_TEST_THREADS=8, a `yes` hog per core): 120 of 120 passed
before and 120 of 120 after, so the natural window is too small to hit locally and the widened-gap run is the evidence.

**`weight-tls-idle`: noise on top of a step, not noise alone.** It failed once at a median of 11,596 KB against the 11,479 KB line
(run 43728144148, PR #68, no kafka change). Idle samples from 50 CI runs, 2026-11-15 00:12 to 17:36 (median of three per run):

- The last 20 runs: medians 10,892 to 11,596 KB, median 11,162 KB. The same kafka code measures 11,012 and 11,356 (691279c4 and its merge
  450d228e) and 11,596 and 11,412 (88a18160 and 67c0cf63), so one tree moves about 350 KB between runners.
- Main's runs: 10,592, 10,960, 10,824 KB (63ec3d25, 674e0bf0, 987233f8), then 11,396 at 3636c3d2 and 11,244 to 11,356 on every main run
  since. Branches based before 3636c3d2 kept measuring 10.6 to 11.1 MB in the same hours.

A step, so the line was not moved. Bisected on aarch64 Linux (Lima, release build, TLS configured, 7 fresh starts each; absolute numbers
lower than CI's x86_64, steps comparable):

| commit | what | idle median |
|---|---|---|
| 987233f8 | main before | 8,816 KB |
| 33ed4871 | kafka side before its efficiency pass | 8,824 KB |
| 086e4b28 | `fix-correct` merged | 8,956 KB (+132) |
| 0ee5cf74 | `fix-idle` merged | 9,024 KB (+68) |
| 07c07a88 | `fix-idiom` merged | 9,024 KB (0) |
| 3636c3d2 | the rest (6985735d, interop-java, Metadata v12 by id) | 9,084 KB (+60) |

+268 KB (+3.0%) in all, mostly `fix-correct` (086e4b28). The weight line 11,479 KB was set from 10,932 KB measured before this step, so
the post-step main median (about 11,300 KB) sits 1.5% under it and runner noise crosses it. Owner: kafka session, to explain the step or
take it back; until then the job will fail now and then. (The comment in ci.yml says the line is "median 10,104 KB plus 5%"; the
weight.md table it cites has 10,932, which is what 11,479 is.)

**The step explained and taken back (kafka session, 2026-11-18).** Re-measured on aarch64 Linux (Lima), each tree built in a clean target
dir of its own (a shared one, with `git archive`'s commit-time mtimes, hands cargo stale crates: that is why 33ed4871 "did not build"),
TLS configured, `/proc/<pid>/status` and `smaps` read 3 s after start:

- `fix-correct` (33ed4871 to 086e4b28) is file-backed pages alone: the heap (416 KB) and anonymous memory are the same before and after,
  the binary's resident pages +192 KB (the binary +196 KB: the varint codec, ListOffsets by record time, the produce answers). Code that
  is mapped and touched at start, not memory the broker holds.
- The heap step is the console schemas merge (31d63f8e1, Phase 10d): heap 356 to 416 KB. With `--console=false` both trees idle at 224 KB,
  and `--schema-registry=false` changes nothing, so it is the console. Built again without `.merge(schemas::routes(web))` the heap is
  back at 348-356 KB: the 9 routes it added cost 60 KB. axum boxes each route's service and its layers (about 7 KB a route), built at
  start and held while the console runs, whether or not anyone opens it.
- Taken back: the console now builds its router on its first connection (`kafka-console/src/serve.rs`, `accept_loop`). main c863cecdf
  against the change, 5 starts each: idle 9,056-9,076 KB (anonymous 816-820) to 8,568-8,580 KB (anonymous 672-676), **-492 KB**, below
  where the step began; after one console request 9,108-9,124 to 9,256-9,264 KB, so the router costs what it did once it is used.
  The TLS idle line is left as it is until CI measures the change.

**`weight` (doze-aws idle) on this PR:** run 43744892056 measured 11,424 / 11,156 / 11,292 KB, over the 11,075 KB line, with no aws change.
Medians over the 20 runs above: 10,404 to 11,068 KB, main's latest (450d228e) 11,068, 7 KB under the line. Same shape as the TLS line: set
from a measurement with no room for runner spread. Not moved here; for the aws session.

## Landing core M1 (#71, 2026-11-19)

The acceptance harness (`doze-module-test`) leaked one System V shared memory segment per failed boot: a boot that failed
after its processes started (a readiness or hook failure in `run::start`, or a converge error in `boot_with`) dropped them,
and `Tree`'s drop kills. A killed PostgreSQL leaves its 56-byte segment behind, and macOS allows 32 (`kern.sysv.shmmni`), so
a few runs fill the table and every later PostgreSQL fails to start. Found with ferret's
`a_seed_the_server_refuses_fails_the_converge` (DocumentDB is PostgreSQL): one new segment per run, none after the fix. Both
paths now stop the started processes the way the module declared (30 s grace) before returning the error. A test that
panics while its instance runs still kills on drop.

## Open items

| item | owner | evidence |
|---|---|---|
| doze-aws's start-time size check (`mention_size`, 9346ebbc) costs about 210 KB idle on Linux: a one-shot OS thread whose arena and stack glibc keeps | aws session | aarch64 Linux A/B: 8,596 KB with it, 8,384 KB without (7 samples each); weight.md |
| The kafka and aws sessions pushed straight to main (3636c3d2, 78ba6190), so their CI ran after the fact and main went red both times | the sessions; main is now protected (PR and 18 checks) | this file's history |
