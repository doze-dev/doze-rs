# The doze CLI: proposal

Asked 2026-11-13 ("make the CLI super awesome"). For the owner's review before anything is built. The Rust core has
no `doze` CLI yet; it is built in the core port, command by command, as the daemon API it calls lands. This says what
it must be, measured against the Go CLI as it behaves today, and what can be built before the core.

Inputs: the Go CLI run end to end on 2026-11-13 (an isolated `DOZE_HOME`, a one-service stack, `init`, `lint`, `up`,
`status`, `url`, `env`, `logs`, `doctor`, `add`, `down`); `doze/docs/design/next` (daemon-api.md "CLI on the new
API", screens.md page 5), whose decisions this keeps; docs/port/modules/compat.md (update notices); the protocol-2
module manifest (`client`, `detect`, `default_version`, diagnostics with spans); prior art below.

## What the Go CLI already gets right (kept)

- Grouped help that reads as a table of contents (The stack, One service, Before-the-dash tools, …).
- Three outputs: rich on a terminal, plain when piped, `--json` as one envelope (`schema: doze/v1`); `-q` where the
  exit code is the answer; `NO_COLOR`, `--yes`.
- `doze` alone opens the app, the dash over SSH, the status table when piped.
- `doze up` in 6.7 s from nothing (download included): one line per service with its time, "cache awake in 1.6s",
  then what to do next. `status` is a clear table with a summary line and the next action.
- `lint` names typos with "did you mean"; `add --dry-run`; every dash action has a CLI form, enforced by a test.

## What it gets wrong (found running it)

| # | What happened | Whose problem the CLI made it look like |
|---|---|---|
| F1 | `registry.doze-dev.com` has no DNS record (Go `main` points at it since 7253bd8, 2026-11-07); `doze-dev.com/registry` answers 525. A fresh install cannot resolve any module. | Ops, not the CLI: **owner action** (DNS / Cloudflare). |
| F2 | That failure printed 3 to 5 times per command, each time the full Go error with the URL. | Noise that buries the answer. |
| F3 | Without a cached module, `status`, `env` and `url` say "the stack's config has a mistake — no module called valkey". | A network problem blamed on the user's file. |
| F4 | `lint` printed "✓ doze.hcl is valid", exit 0, having checked none of the module's arguments ("not checked: registry cannot be reached"). A typo passes. | A green tick for work not done. |
| F5 | `doctor` printed "✓ all checks passed" while the registry was unreachable: it does not check the registry. | The diagnostic tool misses the outage. |
| F6 | `init -y` on a project whose go.mod requires go-redis wrote a **postgres** block, with no word why; the go-redis requirement was not used (the project had no main package, so it had no imports to read). | Invented config. |
| F7 | `down` printed "✓ doze stopped" and left the valkey module process running, reparented to PID 1, still running 37 s later (seen once; a test will say how often). | Lies about stopping. |
| F8 | `--help` shows an empty "The dash:" group (its command is hidden). | Polish. |

F1 needs the owner now; F2 to F7 are what the rules below exist to prevent, each with a test.

## The rules

1. **Every message knows whose problem it is.** Yours (the config: file, line, column, the offending text, a fix),
   the machine's (a port, the network, a permission: what doze tried and the exact command that fixes it), or doze's
   (a bug: one line and `doze bug` to report it with the log attached). Never a raw library error on its own.
2. **Say it once.** Notices are collected for the run, deduplicated, and printed once, last, one line each.
3. **Never claim what was not checked.** A tick means verified. Partial is said as partial ("1 of 2 checked; valkey
   not checked: the registry can't be reached, using cached 0.2.1") and exits non-zero under `lint`/`doctor`.
4. **Always a next step.** A result ends with what to do next (as `up` and `init` already do); an error with the fix.
5. **Read commands never wait on the network.** `status`, `env`, `url`, `lint` use the cached index and refresh it in
   the background; only `add`, `upgrade` and a first resolve go out, with a 3 s budget and visible progress.
   Budget, checked in CI (a target to confirm once measured): `doze status` / `env` / `url` under 15 ms warm on the reference Mac (Go today: 27 to 54 ms).
6. **One model, three renderings.** Every command builds a typed result once; the terminal, plain and JSON renderers
   draw it. Plain and JSON are stable contracts (snapshot-tested); the terminal adds colour and live progress only.
7. **The terminal is a client of the daemon, as the app is.** Same actions, same words (daemon-api.md); the parity
   test stays.
8. **Leave nothing behind.** `down` verifies: no doze process of this stack, no socket, no listener left; if one is,
   it says which and stops it.

## The command surface

Kept from Go (same names, same meaning): `up`, `down`, `status [-w]`, `env`, `run`, `sync`, `logs [-f] [--run]
[--since]`, `wake`, `sleep`, `restart`, `reset`, `pin`, `on`, `off`, `task`, `url`, `migrate`, `add`, `init`, `lint`,
`fmt`, `doctor [--fix]`, `trust`, `dns-setup`, `open`, `modules`, `upgrade`, `version`, `completion`.

New:

| Command | What | Grounded in |
|---|---|---|
| `doze connect <name>` | Opens the engine's own client on the instance (`valkey-cli`, `psql`), through doze's address so it wakes it, the password in the client's own variable (never argv). Go has only `url` to copy. | manifest `client` (step 7) |
| `doze open <name>` | Opens a module's web console (temporal's Web UI, the aws console). `open` alone opens the app, as now. | `ConsoleSpec.web_url`, `{address:ui}` (step 8) |
| `doze why <name>` | Why it is in its state: what woke it last and when, what keeps it awake (a pin, open connections), when it sleeps, its last exit and restarts. | daemon journal (daemon-api.md) |
| `doze sync --dry-run` | What `sync` would change, object by object (a role's password, a namespace's retention), not only which blocks. `--dry-run` everywhere a command writes. | module `Objects` hashes (protocol.md row 15) |
| `doze update` | Updates doze the way it was installed (brew, installer, app), and hands the daemon over without stopping services. | compat.md rules 4 and 5 |
| `doze bug` | Writes a report (versions, the stack's shape without values, the last log lines) to a file and says where. | rule 1 |

Completions are dynamic: service names, module names and their arguments, actions, from the cached manifests.

## What it looks like

A mistake in the config, from the module's own diagnostic and the span the core keeps (the shape rustc and cargo
made familiar):

```
error: "allkeys-lur" is not an eviction policy
  --> doze.hcl:4:22
   |
 4 |   maxmemory_policy = "allkeys-lur"
   |                      ^^^^^^^^^^^^^ did you mean "allkeys-lru"?
   |
   = valkey refuses this when it starts; doze checks it now
```

The registry down (F1 to F4 as they should have read):

```
✓ doze.hcl checks out: 1 module, 3 arguments
! the module registry can't be reached (registry.doze-dev.com: no DNS answer)
  using this machine's copies: valkey 0.2.1 · it was checked with that version
  adding modules waits until it is back · doze doctor checks it again
```

`down` that verifies (F7):

```
✓ cache asleep (data kept)
✓ doze stopped · nothing left running
```

`connect`:

```
$ doze connect cache
→ valkey-cli on cache (127.0.0.1:6390) · password from doze.hcl · ^D to leave
127.0.0.1:6390> 
```

`why`:

```
$ doze why cache
cache is awake
  woke 14:02:11 for a connection from api (pid 4121)
  kept awake by 2 open connections · sleeps 2 min after the last closes
  last exit: 13:55 asleep (idle) · no restarts today
```

`init` that says why, and never invents (F6):

```
✓ wrote doze.hcl
  valkey "cache"   go.mod requires github.com/redis/go-redis/v9
  next: doze up
```

and with nothing found: "found nothing doze runs here; wrote a commented starter (doze add <module> to fill it)".

## Errors and exit codes

| Exit | Meaning |
|---|---|
| 0 | done |
| 1 | it failed (a service did not start, an action was refused) |
| 2 | usage: unknown command or flag |
| 3 | the config has a mistake (the message points at it) |
| 4 | the machine: network, port taken, permission (the message names it and the fix) |
| 5 | partial: done, but not all of it could be checked or reached |

JSON errors carry the same class, a stable code (`DZ0301` …), the span when there is one, and the fix.

## Prior art taken

- **rustc / cargo**: errors with the source line, a caret under the span, `help:` and `=` notes; codes with an
  explainer. doze has the spans (protocol 2 keeps them from HCL to the module's diagnostics).
- **clig.dev** (the CLI guidelines): human output first, machine output on request, say what to do next, respect
  NO_COLOR and pipes, never silent on long work.
- **gh**: one JSON shape per command, interactive only on a terminal, `--yes` for scripts.
- **fly** (`fly launch`): detect, then write config that says why; **mise** / **bun**: a CLI that is instant.
- **Terraform**: a plan before a change, object by object.
- Docker Compose is what people come from: `up`, `down`, `logs -f` and `ps`-like status keep their muscle memory.

## How it is built and tested

Crate `doze-cli` (binary `doze`) in doze-rs:

```
crates/doze-cli/
  src/main.rs          clap derive: groups, global flags (--json -q --no-color -y -c --var --timeout)
  src/cmd/*.rs         one file per command: parse args, call the daemon client, return a typed result
  src/render/{tty,plain,json}.rs   the three renderings of one result
  src/diag.rs          the error renderer: span → source line → caret → hint (from module Diagnostics)
  src/notice.rs        collected, deduplicated notices printed once at the end (rule 2)
  src/exit.rs          the exit-code classes and JSON error codes
  src/complete.rs      dynamic completions from cached manifests
  tests/transcripts/   one file per scenario: the stack, the commands, the expected plain and JSON output
```

Tests: transcript tests run each scenario in a sandboxed `DOZE_HOME` (the module harness's sandbox) and compare plain
and JSON output; a pty test per interactive command; a budget test for read-command latency; F1 to F8 each become a
scenario (registry down, module not cached, typo, partial lint, down with a lingering process).

## Size and order

| Part | When | Days |
|---|---|---|
| Error renderer (`diag.rs`), notices, exit classes, the three-renderer shape, transcript harness | now: they need only module diagnostics and spans, which exist | 5 |
| Each command, as the core's daemon API for it lands (core port phases) | core port | 14 to 18 |
| `connect`, `open`, `why`, `update`, `bug`, dynamic completions | core port, after the daemon journal | 5 |

About 25 days of CLI work, most of it inside the core port.

## For the owner

1. **F1, now**: the registry hostname has no DNS record; fresh installs of doze cannot fetch modules. Point
   `registry.doze-dev.com` at the registry (or revert the Go constant to a host that serves it).
2. Approve the rules and the command surface (or change them).
3. `doze connect` as the name (alternatives: `shell`, `cli`).
4. `--dry-run` as the one preview flag everywhere (Go has `add --dry-run`; `sync` has none).
5. Fix F2 to F7 in the Go CLI too, since it ships today? Recommended for F3, F4, F5 and F7 (they mislead); F2 and F6
   can wait for Rust.
