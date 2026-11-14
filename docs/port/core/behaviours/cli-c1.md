# C1: the CLI on the control protocol (behaviour catalogue)

Distilled from Go `cmd/doze/{tree,tree_render,verbs,logs,task,output,exitcode}.go` and their tests at doze 23b73d6.
Each item names the Rust test or oracle that holds it. "Go oracle" is `scripts/c1-oracle-go.sh` (the Rust CLI beside the
Go CLI against a running Go daemon: 29 of 29 commands identical on stdout and exit code, 2026-11-14).

| # | Behaviour | Held by |
|---|---|---|
| 1 | `status` draws Go's grouped table (Modules, Services, Apps, Tasks; names sorted; glyph, engine, state word, endpoint with the zone name, conns, memory, CPU, note) and the "N awake · M resident" line | `tests/go_goldens.rs` (status.golden byte for byte), Go oracle |
| 2 | `status --json` is Go's `statusJSON` (field order, `live` = the daemon's view byte for byte, services in Go's order) | transcripts `status.txt`, Go oracle |
| 3 | `status --graph` draws the dependency tree from each instance's `needs` | transcript, Go oracle |
| 4 | `status -w` redraws on each instance change and meter frame, never for the opening snapshot; piped, a blank line between draws; `--json`, one `status` line per change, never a repeat, then `{"type":"end"}`; Ctrl-C or the stack stopping ends it with exit 0 | go_goldens (status-watch.golden), transcript, manual SIGINT/SIGTERM runs against the Go daemon |
| 5 | `wake` prints `✓ name awake in <journal's ready time>` per service, in order, from a journal subscription; a verb that answered at once waits for no event | go_goldens (wake.golden) |
| 6 | the first verb that fails stops the run; the lines before it stand; exit 1 with the daemon's words | go_goldens (wake-failed.golden, stdout part), transcript |
| 7 | `sleep` with names sleeps each; without, everything awake (the daemon keeps running) and the result names what was awake; with doze down, "nothing is awake", exit 0 | go_goldens (sleep-restart-reset-pin), exit_codes |
| 8 | `pin` is idempotent: the daemon's toggle is sent only to the ones that differ; each result says whether it changed; `--toggle` flips; `--off` with doze down succeeds | go_goldens, transcript `verbs.txt`, Go oracle |
| 9 | `reset` asks on a terminal and refuses with exit 9 without one (or under `--json`) unless `-y` | transcript, exit_codes |
| 10 | `logs`: one service's run, oldest first; several merged by time with `name │`; `err` for stderr, `[step]`, `proc:`; local wall clock (`TZ`); `--run 0/-N/N`, `-n`, `--since`; "has not printed anything yet" | go_goldens (five logs goldens), Go oracle |
| 11 | `logs` with doze stopped reads the log store on disk (Go's segment format) | `doze-logstore` tests, transcript `logs-down.txt` |
| 12 | `logs -f` follows from one subscription (backlog and new lines); `--json` is NDJSON with an end line; Ctrl-C ends it with exit 0 | go_goldens (logs-follow.golden), manual SIGINT run against the Go daemon |
| 13 | `task` lists the tasks from `.doze/state.json`; `task <name>` runs it ("→ running…" on stderr); a non-task or unknown name exits 4 | transcript `task.txt`, Go oracle |
| 14 | exit codes 0 1 2 4 6 7 8 9 (10, 11 unused here) and `--json` envelopes with `error.code`/`error.exit` | `tests/exit_codes.rs` |
| 15 | piped output is plain whatever TERM, CI, FORCE_COLOR say | `tests/exit_codes.rs` |
| 16 | every verb in the table is a command and every stack command a verb | `tests/parity.rs`, `doze-control` `actions` tests |
| 17 | the envelope is written as Go's encoder writes it: field order, `<>&` escaped, Go floats | `doze-control` `json` tests, transcripts |

New in Rust (proposal): `why` (state in the app's words, the journal's last wake, ready and stop; works with doze down)
and `--dry-run` on `wake`, `sleep`, `restart`, `reset`, `pin`, `off`, `on`, `task` (transcript `verbs-dry-run.txt`).

## Differences from Go, by design or until a later phase

- **Needs doze.hcl (C2)**: with doze stopped Go's `status` lists the declared services; this one says doze is not
  running. `wake`, `restart`, `reset`, `pin`, `task` start the daemon in Go; here they exit 7 until the Rust daemon
  (D1) and `up` (C2) exist. The state folder is found at the default place (a `home`/`data_dir` in doze.hcl is not
  read). A task's `manual` flag and a forwarded app's public port are not shown. Within modules and apps,
  `status --json` keeps the daemon's order, not doze.hcl's.
- **Errors**: a failure is a diagnostic on stderr (`error[DZ07xx]: … fix: …`) where Go printed `✗ name: …`; its JSON
  `error.message` is prefixed with the service's name, and `error.id`/`notes`/`fix` are added. Unknown names say "the
  stack has no service called …" with the list as a note.
- **Better than Go**: "0 awake ·  resident" (Go, nothing awake) reads "0 awake"; `-f` with `--run` and `--since 0` are
  usage errors (exit 2; Go exits 1).
