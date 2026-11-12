# W0: the module transport and process control on Windows (docs/port/modules/PLAN.md, D11)

Date 2026-11-12. GitHub Actions run 43204939229 on branch `w0-windows` (doze-dev/doze-rs): `windows-latest`
(x64), `windows-11-arm`, `ubuntu-24.04`, `ubuntu-24.04-arm`, `macos-latest`. The same code runs everywhere; every
OS difference is in `src/os.rs` (the shape of `doze-os`). CI runners are shared VMs: compare platforms by order of
magnitude, not by the digit (the same macOS code ran 2.4x faster on the M5 Max than on the runner).

## Verdict

**D11 holds. Native Windows works for the module protocol and for process control, and does better than Unix at
one thing: when the host dies, a Job object takes the module and every engine with it.** One real finding changes
the plan: postgres refuses to run under an elevated administrator token, so doze needs a way to start engines with
a restricted token on Windows (below).

## Results

| check | Windows x64 | Windows Arm | Linux x64 / Arm (CI) | macOS (CI) |
|---|---|---|---|---|
| launch → hello | 3.9 ms | 8 to 10 ms | 0.8 ms | 2.7 ms |
| plan call p50 / p99 | 29 / 43 µs | 52 to 68 / 83 to 91 µs | 34 to 44 / 40 to 85 µs | 64 / 182 µs |
| 5,000 calls, 100 in flight | 35 ms | 48 to 53 ms | 46 to 50 ms | 55 ms |
| module calls host back twice | 64 µs | 105 to 116 µs | 46 to 50 µs | 110 µs |
| stream 12/12, cancel seen | 0.04 ms | 0.10 ms | 0.04 to 0.05 ms | 0.07 ms |
| client socket handed to the module, p50 | 0.43 ms (p99 31 ms, one outlier) | 1.5 ms | 0.38 to 0.44 ms | 0.6 ms |
| 200 concurrent handoffs, unique tokens | 61 ms, 0 crossed | 235 to 324 ms, 0 crossed | 22 to 37 ms, 0 crossed | 20 ms, 0 crossed |
| crash: both calls fail | 2.2 ms | 1.1 to 1.7 ms | 0.5 to 0.7 ms | 0.8 ms |
| **host killed: module gone** | 0.8 to 0.9 ms (10/10) | 1.9 to 3.2 ms (10/10) | 1.3 to 2.4 ms (10/10) | 1.5 to 3.1 ms (10/10) |
| **host killed: engine gone** | **0.8 to 0.9 ms (10/10)** | **1.9 to 3.2 ms (10/10)** | still running (10/10) | still running (10/10) |
| graceful stop (Windows: CTRL_BREAK to a new process group; Unix: SIGTERM) | 1.3 ms | 1.9 to 2.5 ms | 0.3 to 0.4 ms | 0.7 ms |
| `which` via PATH (+ PATHEXT): a `.cmd` script found and run with `["a b", "c"]` | `[a b] [c]` | `[a b] [c]` | `[a b] [c]` | `[a b] [c]` |
| idle memory, host / module | 6.1 / 4.8 MB (working set) | 7.1 / 6.1 MB | 2.9 to 3.4 / 2.4 to 2.9 MB | 2.6 / 2.1 MB |
| binary | 0.93 MB | 0.86 MB | 1.09 MB | 0.94 MB |

Postgres (only the x64 Windows image ships it, 17.11; the Arm image has none):

| step | Windows x64 |
|---|---|
| initdb | 4.8 s |
| `postgres.exe` started directly by the supervisor | **refused**: "Execution of PostgreSQL by a user with administrative permissions is not permitted" (the runner's token is an elevated admin) |
| `pg_ctl start -w` (which drops admin rights with a restricted token) | ready in 271 ms, listening on 127.0.0.1 |
| `psql` over TCP | `PostgreSQL 17.11 on x86_64-windows` |
| `pg_ctl stop -m fast -w` | 119 ms |

(The Linux postgres rows failed for a reason of the test, not the platform: the Ubuntu build's default socket
directory `/var/run/postgresql` is not writable by the runner, and the test did not pass `-k`. doze always passes
its own socket directory; the Go modules have run postgres on Linux for months.)

## Findings that change the design

1. **Engines must not run with an elevated admin token on Windows.** Postgres refuses outright (MariaDB and others
   warn or refuse too). Many Windows developers run an elevated shell or have UAC off. Options:
   - (a) `doze-os` starts engine processes with a restricted token (`CreateRestrictedToken` minus Administrators,
     `CreateProcessAsUserW`), exactly what `pg_ctl` does. Needs our own process-spawn on Windows (std's `Command`
     cannot take a token), ~300 lines of FFI.
   - (b) The daemon, when started elevated, re-launches itself once with a restricted token, so everything it starts
     inherits a normal token. One place, and the daemon should not hold admin rights anyway.
   - (c) Modules declare a launcher (`pg_ctl start`). Rejected: the engine then is not doze's child, so readiness,
     logs and supervision suffer.
   Recommendation: (b), with (a) as the fallback if a restricted daemon cannot do something it must (names and the
   local CA need admin rights only at setup, through `doze setup` elevation, not at run time). Prove it in P1.
2. **Kill-on-host-death is stronger on Windows than on Unix.** The Job object took the engine down in under 3 ms in
   every run; on Linux and macOS the engine survived its host. Unix needs its own answer in `doze-os`: on Linux,
   `PR_SET_PDEATHSIG` on each engine (the Go core already sets Pdeathsig); on macOS, which has none, the existing
   pidfile reconciliation at daemon start. Record as a `doze-os` requirement, not a protocol one.
3. **Socket handoff costs 0.4 to 1.5 ms per connection on Windows** (WSADuplicateSocketW + WSASocketW), against
   0.1 ms on a local Mac and 0.4 ms on CI Linux. Fine for development traffic (it is once per client connection, not
   per query). Keep it; revisit only if a benchmark of connection churn says otherwise.
4. **The control channel works as a named pipe with an unguessable name, and the host verifies the client's pid**
   (`GetNamedPipeClientProcessId`). No handle inheritance was needed, and the module's stdin is plain null.
5. **Graceful stop by CTRL_BREAK works** for a process started in its own process group, even on a runner with no
   console. That is the default `stop` for D15 on Windows; postgres overrides it with `pg_ctl stop -m fast`.
6. **`PATHEXT` lookup plus std's batch-file handling** run a `.cmd` with arguments intact. D14 (no shell) is workable.
7. Windows idle memory is higher (working set counts mapped system DLLs): 4.8 to 6 MB per module against 2 to 3 MB.

## Not covered (carry into P1)

- The restricted-token launch (finding 1), on a real elevated and a real non-elevated session.
- Unix engine cleanup on host death (finding 2): Linux `PR_SET_PDEATHSIG`, macOS reconciliation.
- Postgres on Windows Arm (no binaries on the runner; EDB ships x64 only, which runs under emulation).
- AF_UNIX on Windows (not needed with TCP backends; tokio has no AF_UNIX on Windows).
- A real interactive desktop session (runners are services); console behaviour may differ.

## Reproduce

Push to `w0-windows` (workflow `.github/workflows/w0.yml`), or locally on macOS/Linux:
`cargo build --release && target/release/w0-windows host|orphan|break|pg|which` (`PGBIN` for `pg`).
Type-check Windows from a Mac: `cargo check --target x86_64-pc-windows-msvc` (needs the std target only).
