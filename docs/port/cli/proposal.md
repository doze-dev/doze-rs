# The doze CLI: proposal

Asked 2026-11-13 ("make the CLI super awesome"). For the owner's review before anything is built. The Rust core has
no `doze` CLI yet; it is built in the core port, command by command, as the daemon API it calls lands. This says what
it must be, measured against the Go CLI as it behaves today, and what can be built before the core.

Inputs: the Go CLI run end to end on 2026-11-13 (an isolated `DOZE_HOME`, a one-service stack, `init`, `lint`, `up`,
`status`, `url`, `env`, `logs`, `doctor`, `add`, `down`); `doze/docs/design/next` (daemon-api.md "CLI on the new
API", screens.md page 5), whose decisions this keeps; docs/port/modules/compat.md (update notices); the protocol-2
module manifest (`client`, `detect`, `default_version`, diagnostics with spans); prior art below. Checked the same
day against every command the doze website documents: [Command coverage](#command-coverage).

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
   the background; only `add`, `modules upgrade`, `modules verify`, `update`, `modules docs` of an uncached module and a first
   resolve go out, with a 3 s budget and visible progress.
   Budget, checked in CI (a target to confirm once measured): `doze status` / `env` / `url` under 15 ms warm on the reference Mac (Go today: 27 to 54 ms).
6. **One model, three renderings.** Every command builds a typed result once; the terminal, plain and JSON renderers
   draw it. Plain and JSON are stable contracts (snapshot-tested); the terminal adds colour and live progress only.
7. **The terminal is a client of the daemon, as the app is.** Same actions, same words (daemon-api.md); the parity
   test stays.
8. **Leave nothing behind.** `down` verifies: no doze process of this stack, no socket, no listener left; if one is,
   it says which and stops it.

## The command surface

Kept from Go (same names, same meaning, same flags and aliases; [Command coverage](#command-coverage) lists every
one): `up [-f] [--last] [--all]`, `down`, `status [-w] [--graph]` (aliases `tree`, `ls`, `ps`), `env [--dotenv]`,
`run [--env] -- <cmd>`, `sync [--dry-run]`, `logs [-f] [--run] [--since] [-n]`, `wake` (`boot`), `sleep` (`reap`),
`restart`, `reset`, `pin [--off] [--toggle]` (`keepawake`), `on`, `off`, `task`, `url` (`copy`), `migrate`,
`add [--version] [--port] [--as] [--dry-run]`, `init [--wizard] [--force]`, `lint [--offline]`, `fmt [--check]`,
`doctor [--fix] [--print-fix]`, `trust [--check] [--print] [--remove] [--rotate]`, `dns-setup [--check] [--print]
[--uninstall]`, `open [dir|doze://link] [--show]`, `modules upgrade [--check]` (`modules` alias `mod`), `version`,
`completion`, `help`. Bare `doze` keeps its three faces (app, dash over SSH, status table when piped). Global flags as
Go: `-c/--config`, `--var`, `--json` (`DOZE_JSON`), `-q`, `-y/--yes` (`DOZE_YES`), `--timeout`, `--no-color`
(`NO_COLOR`).

Dropped from Go: the hidden `doze dash` (deprecated; the dash stays behind bare `doze` over SSH) and the hidden no-op
`up --detach`.

New:

| Command | What | Grounded in |
|---|---|---|
| `doze connect <name>` | Opens the engine's own client on the instance (`valkey-cli`, `psql`), through doze's address so it wakes it, the password in the client's own variable (never argv). Go has only `url` to copy. Name decided by the owner. | manifest `client` (step 7) |
| `doze open --console <name>` | Opens a module's web console (temporal's Web UI, the aws console) in the browser. A flag, not a positional: `open [dir]` already takes one, and a monorepo with a folder `api` and a service `api` would be ambiguous. | `ConsoleSpec.web_url`, `{address:ui}` (step 8) |
| `doze why <name>` | Why it is in its state: what woke it last and when, what keeps it awake (a pin, open connections), when it sleeps, its last exit and restarts. | daemon journal (daemon-api.md) |
| `--dry-run` on every mutating command | What the command would change, and nothing changed; for `sync` object by object (a role's password, a namespace's retention), not only which blocks. Owner decision; the table below says what each prints. | module `Objects` hashes (protocol.md row 15) |
| `doze update` | Updates doze the way it was installed (brew, installer, app), and hands the daemon over without stopping services. | compat.md rules 4 and 5 |
| `doze bug` | Writes a report (versions, the stack's shape without values, the last log lines) to a file and says where. | rule 1 |
| `doze modules` | Lists the modules the stack uses: the pin in `doze.lock`, the newest release that fits, which services use it. Go prints help here; the website already describes "`doze modules` output". | `doze.lock`, the cached index |
| `doze modules docs <module> [--web]` | A module's description, an example and every argument (type, required, default, which engine versions take it), from the manifest; `--web` opens its Reference page instead. The website advertises it. | protocol-2 schema-declared config |
| `doze modules verify` | Re-checks every pin in `doze.lock` against the signed index: the key, each artifact's sha256, the signature, printed per module. The website lists it as planned. | the registry key, the signed index, `doze.lock` |

Completions are dynamic: service names, module names and their arguments, actions, from the cached manifests.

### `--dry-run`, command by command

Every command that changes something takes `--dry-run`: it does the reading and the planning, prints what it would
do, changes nothing, never writes `doze.lock` (as Go's dry runs already guarantee), and exits 0 when it could make the
plan (otherwise the class of what stopped it: 5 a config error, 4 an unknown name, …).

| Command | `--dry-run` prints |
|---|---|
| `up` | what it would converge (object by object, as `sync`), then the services it would wake, in order |
| `down` | the services it would put to sleep, and that the daemon would stop |
| `sync` | each object it would create, change or drop, per block |
| `wake`, `restart` | the services, with the dependencies it would wake first, in order |
| `sleep`, `off`, `on`, `pin` | each service and the state it would move to (`pin` and `--off`/`--toggle` alike) |
| `reset` | each service and the data it would delete: folder and size |
| `migrate` | the version from and to, the folder it would set aside, the steps (dump, fresh cluster, restore) |
| `task` | the task and what it would wake first; the task does not run |
| `add` | the block (as Go) |
| `init` | the `doze.hcl` it would write, with the reason for each block |
| `fmt` | a diff per file; `--check` stays (the same, and exit 1 if any file would change, for CI) |
| `modules upgrade` | each module's pin from and to; `--check` stays (exit 1 if any would move) |
| `trust`, `dns-setup`, `doctor --fix` | the privileged script; `--print` (and doctor's `--print-fix`) stay as hidden aliases so the published docs keep working |
| `update` | the version it would install and how (brew, installer, app) |

No `--dry-run`: the read commands (`status`, `env`, `url`, `logs`, `lint`, `doctor`, `why`, `version`, `modules`,
`modules docs`, `modules verify`, `completion`, `help`), the ones that only open something (`open`, `connect`), `bug`
(it writes a report file and nothing else), and `run` (it runs your command; there is nothing to preview).

### Exit codes of the new commands

| Command | Exits |
|---|---|
| `connect` | the client's own code (as `doze run`); 4 no service by that name; 1 the module declares no client ("valkey has none; `doze url cache` prints its address"); 10 the client binary is not on this machine (the message names how to get it) |
| `open --console` | 0 opened; 4 no service by that name; 1 the module has no web console (the message offers `doze connect`) |
| `why` | 0 (it works from the journal on disk with the daemon down, and says so); 4 no service by that name |
| `update` | 0 updated or already newest; 10 the network; 1 this install cannot update itself (the message gives the command, such as `brew upgrade doze`) |
| `bug` | 0, with the report's path; 1 it could not write it |
| `modules` | 0; 6 no config; 11 partial: the index is not cached, so "newest that fits" is unknown (said in the table) |
| `modules docs` | 0; 4 no module by that name; 10 not cached and the registry cannot be reached |
| `modules verify` | 0 all verified; 1 a mismatch (it names the module and the fix); 10 the registry cannot be reached; 11 partial: some checked against the cached index only |

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

`open --console`:

```
$ doze open --console flows
→ temporal Web UI on flows (http://flows.shop.doze:8233) · opened in your browser
```

`modules`:

```
$ doze modules
module     pinned   newest that fits   used by
postgres   0.2.5    0.2.5              db
valkey     0.2.1    0.2.2              cache
  1 upgrade waiting · doze modules upgrade valkey
```

`modules docs`:

```
$ doze modules docs valkey
valkey 0.2.1 (pinned) · Valkey, the Redis-compatible key-value store
  module valkey "cache" { version = 8 }

  version            number   required             8, 7.2
  port               number   default 6379
  maxmemory_policy   string   default "noeviction"  one of noeviction, allkeys-lru, …
  …
  next: doze add valkey · the whole page: doze modules docs valkey --web
```

`modules verify`:

```
$ doze modules verify
registry key ed25519 3f9a…c2 (built into this doze)
✓ index signature valid · signed 2026-11-10
✓ postgres 0.2.5  darwin/arm64  sha256 9b1e…04  matches the index and the cached copy
✓ valkey   0.2.1  darwin/arm64  sha256 51c7…aa  matches the index and the cached copy
  2 of 2 verified
```

`sync --dry-run`:

```
$ doze sync --dry-run
db (postgres)
  ~ role app        password changed
  + extension pgvector
cache (valkey)      nothing to change
  dry run: nothing was changed · doze sync applies this
```

## Errors and exit codes

The codes Go ships are published (website CLI reference, "Codes are added, never renumbered") and scripts branch on
them, so they stay as they are; the two classes this proposal needs are added after them. (The first draft of this
table renumbered them; the coverage check caught it.)

| Exit | Kind | Meaning |
|---|---|---|
| 0 | | done |
| 1 | `failed` | it failed (a service did not start, an action was refused), for a reason with no code of its own |
| 2 | `usage` | unknown command or flag, a missing argument |
| 4 | `not_found` | the stack declares no service or task by that name |
| 5 | `config_invalid` | the config has a mistake (the message points at it) |
| 6 | `no_config` | no `doze.hcl` here or in any parent |
| 7 | `daemon_down` | the command needs a running stack |
| 8 | `timeout` | it took longer than allowed |
| 9 | `needs_confirmation` | it needs a yes and there is no terminal: pass `-y` |
| 10 | `machine` | **new**: network, port taken, permission (the message names it and the fix); Go says 1 |
| 11 | `partial` | **new**: done, but not all of it could be checked or reached (rule 3); Go says 0 |
| 130 | `interrupted` | Ctrl-C or SIGTERM |

`doze run` and `doze connect` exit with their command's own code. JSON errors keep the `doze/v1` envelope:
`error.code` stays the kind word above and `error.exit` the number; the stable message code (`DZ0301` …), the span
when there is one, and the fix are added as new fields (`error.id`, `error.span`, `error.fix`), so no script that
reads today's envelope breaks.

## Command coverage

Asked 2026-11-13: "Check all the commands we need based on the doze-website." Every `doze` command, subcommand, alias
and flag found in any of three places, one row each:

- **the website**: the doze docs (doze repo `website/src/content/docs/` at origin/main 23b73d6, served under
  `/docs/doze/`; cited `docs/…`), the site itself (doze-website `src/` at origin/main d3d3538: landing, product
  pages, registry, changelog; cited `site/…`), and the doze AWS and doze Kafka docs (no `doze` CLI commands in them:
  they document their own `doze-aws` / `doze-kafka` binaries). `cli` is `docs/reference/cli.md`.
- **Go**: `cmd/doze` at origin/main 23b73d6, built and walked with `--help` (hidden commands and flags read from the
  source); "y (hidden)" is in the binary but not in help.
- **proposal**: this document before → after this check.

Decisions: **keep** (as Go, or as already proposed), **add** (specified in [The command surface](#the-command-surface)),
**website is wrong** (with what it should say), **drop** (with the reason).

| Command | Website (file:line) | Go | Proposal | Decision |
|---|---|---|---|---|
| **Global** | | | | |
| `-c, --config <path>` (a directory merges its `*.hcl`) | cli:28; docs/hcl/splitting-files.md:59 | y | y | keep |
| `--var name=value` | cli:29; docs/hcl/names.mdx:146 | y | y | keep |
| `--json` / `DOZE_JSON=1` | cli:30, cli:434; site/pages/index.astro:277 | y | y | keep: `doze/v1` envelope unchanged, fields only added |
| NDJSON for `logs -f`, `status -w` | cli:462 | y | n → y | keep |
| `-q, --quiet` | cli:31 | y | y | keep |
| `-y, --yes` / `DOZE_YES=1` | cli:32, cli:472 | y | y | keep |
| `--timeout <duration>` | cli:33; docs/hcl/start-order.md:114 | y | y | keep |
| `--no-color` / `NO_COLOR=1` | cli:34 | y | y | keep |
| `-h, --help` | cli:10 | y | n → y | keep |
| exit codes 0 1 2 4–9 130 | cli:475–493; docs/guides/upgrading-from-0-1.mdx:262 | y | n (renumbered) → y | keep the published codes; the proposal's renumbering is undone, 10 and 11 added |
| **The app** | | | | |
| `doze` (app / dash over SSH / status when piped) | cli:38–51 | y | y | keep |
| `open [dir]` | cli:38; docs/app/library.mdx:61 | y | y | keep |
| `open doze://stack[/service]` | cli:53–57 | y | n → y | keep |
| `open --show <service>` | cli:47; docs/app/inside-a-service.mdx:37 | y | n → y | keep |
| `open` with no screen prints the status table | cli:49 | y | n → y | keep |
| `open --serve --fixture --measure --window --close-after --quit-after` | — | y (hidden) | n | keep hidden: the app's dev server and bench, owned by the app port, not the CLI surface |
| `open <name>` (a module's web console) | — | n | y → changed | becomes `open --console <name>`: the positional is already `[dir]` |
| `dash` | cli:426; docs/guides/upgrading-from-0-1.mdx:254 | y (hidden, deprecated) | n | drop: the app replaced it; the dash stays behind bare `doze` over SSH |
| **The stack** | | | | |
| `up [service…]` | cli:62; docs/start/first-stack.mdx:82 | y | y | keep |
| `up -f, --follow` | cli:72; docs/guides/faq.md:123 | y | n → y | keep |
| `up --last` | cli:73; docs/app/library.mdx:90 | y | n → y | keep |
| `up --all` | cli:74; docs/app/library.mdx:91 | y | n → y | keep |
| `up --detach` | — | y (hidden, no-op) | n | drop: `up` always detaches |
| `down` | cli:82; docs/app/library.mdx:93 | y | y | keep (verifies, F7) |
| `sync [service]` | cli:89; docs/guides/workflows.mdx:226 | y | y | keep |
| `sync --dry-run` | cli:98; docs/guides/workflows.mdx:226 | y | y | keep, object by object |
| `sync --auto-approve` | docs/guides/upgrading-from-0-1.mdx:258 (as gone) | n | n | drop: 0.1 spelling; the website already says use `-y` |
| `status` | cli:117; site/pages/examples.astro:310 | y | y | keep |
| `tree`, `ls`, `ps` (aliases of `status`) | cli:119 | y | n → y | keep |
| `status -w, --watch` | cli:126; docs/guides/workflows.mdx:82 | y | y | keep |
| `status --graph` | cli:127; docs/guides/microservices.mdx:214 | y | n → y | keep |
| `env [service…]` | cli:143; docs/services/aws.md:69 | y | y | keep |
| `env --dotenv` | cli:151; docs/hcl/references-and-env.md:120 | y | n → y | keep |
| `run -- <command>` | cli:160; docs/hcl/teams-and-ci.md:95 | y | y | keep |
| `run --env` | cli:168; docs/guides/faq.md:34 | y | n → y | keep |
| **One service** | | | | |
| `wake [name…]` | cli:179; docs/hcl/sleep.mdx:110 | y | y | keep |
| `boot` (alias of `wake`) | cli:181 | y | n → y | keep |
| `sleep [name…]` | cli:187; docs/hcl/sleep.mdx:108 | y | y | keep |
| `reap` (alias of `sleep`) | cli:189 | y | n → y | keep |
| `restart <name…>` | cli:195; docs/guides/troubleshooting.mdx:68 | y | y | keep |
| `reset <name…>` | cli:199; docs/services/postgres.md:220 | y | y | keep |
| `reset --yes`, `init --yes` | docs/guides/upgrading-from-0-1.mdx:258; site/data/module-extras.ts:190 | y (global `--yes`) | y | keep: it is the global `-y` |
| `migrate <name>` | cli:205; docs/guides/postgres-upgrades.mdx:58 | y | y | keep |
| `task [name]` | cli:214; docs/hcl/tasks.mdx:115 | y | y | keep |
| `pin <name…>` | cli:224; docs/hcl/sleep.mdx:112 | y | y | keep |
| `keepawake` (alias of `pin`) | cli:226 | y | n → y | keep |
| `pin --off` | cli:234; docs/hcl/sleep.mdx:113 | y | n → y | keep |
| `pin --toggle` | cli:235; docs/guides/upgrading-from-0-1.mdx:259 | y | n → y | keep |
| `off <name…>` | cli:237; docs/hcl/sleep.mdx:144 | y | y | keep |
| `on <name…>` | cli:243; docs/hcl/sleep.mdx:145 | y | y | keep |
| `logs [name…]` | cli:248; docs/app/inside-a-service.mdx:39 | y | y | keep |
| `logs -f, --follow` | cli:257; docs/start/your-repo.mdx:137 | y | y | keep |
| `logs --run <n>` | cli:258; docs/guides/troubleshooting.mdx:38 | y | y | keep |
| `logs --since <duration>` | cli:259; docs/guides/workflows.mdx:147 | y | y | keep |
| `logs -n, --tail <n>` | cli:260 | y | n → y | keep |
| `url <name>` | cli:270; docs/app/domains.mdx:73 | y | y | keep |
| `copy` (alias of `url`) | cli:272 | y | n → y | keep |
| `connect <name>` | — (docs/guides/upgrading-from-0-1.mdx:255 sends people to `psql "$(doze url db)"`) | n | y | keep (owner decided the name) |
| `why <name>` | — (the app's "why it is awake", docs/guides/faq.md:120) | n | y | keep |
| `shell <name>`, `psql` | docs/guides/upgrading-from-0-1.mdx:255 (as gone) | n | n | drop: `connect` replaces them |
| **Set up and edit** | | | | |
| `init` | cli:285; docs/start/your-repo.mdx:52 | y | y | keep (says why, never invents: F6) |
| `init --wizard` | cli:297; docs/start/your-repo.mdx:101 | y | n → y | keep |
| `init --force` | cli:298; docs/start/your-repo.mdx:52 | y | n → y | keep |
| `add <module\|service> [name]` | cli:300; site/pages/aws.astro:209 | y | y | keep |
| `add --version <v>` | cli:307 | y | n → y | keep |
| `add --port <n>` | cli:308; site/scripts/registry-search.ts:550 | y | n → y | keep |
| `add --as installed\|container\|download` | cli:309; docs/hcl/your-own-services.md:74–76 | y | n → y | keep |
| `add --dry-run` | cli:310; docs/hcl/shared-services.md:37 | y | y | keep |
| `lint` | cli:329; docs/hcl/teams-and-ci.md:107 | y | y | keep; partial now exits 11 (F4), see website fixes |
| `lint --offline` | cli:343; site/pages/registry/[ns]/[module]/index.astro:236 | y | n → y | keep |
| `fmt` | cli:348; docs/hcl/splitting-files.md:52 | y | y | keep |
| `fmt --check` | cli:356; docs/hcl/teams-and-ci.md:103 | y | n → y | keep |
| `doctor` | cli:358; docs/app/domains.mdx:125 | y | y | keep (checks the registry: F5) |
| `doctor --fix` | cli:367; docs/hcl/names.mdx:100 | y | y | keep |
| `doctor --print-fix` | cli:368; docs/app/settings.mdx:85 | y | n → y | keep as a hidden alias of `doctor --fix --dry-run` |
| `dns-setup` | cli:370; docs/guides/linux.mdx:75 | y | y | keep |
| `dns-setup --check` | cli:380; docs/start/install.mdx:75 | y | n → y | keep |
| `dns-setup --print` | cli:381; docs/start/install.mdx:79 | y | n → y | keep as a hidden alias of `--dry-run` |
| `dns-setup --uninstall` | cli:382; site/pages/download.astro:162 | y | n → y | keep |
| `trust` | cli:384; site/pages/doze/domains.astro:370 | y | y | keep |
| `trust --check` | cli:395; docs/app/domains.mdx:62 | y | n → y | keep |
| `trust --print` | cli:396; docs/app/domains.mdx:63 | y | n → y | keep as a hidden alias of `--dry-run` |
| `trust --remove` | cli:397; docs/app/domains.mdx:159 | y | n → y | keep |
| `trust --rotate` | cli:398; docs/why/trust.md:86 | y | n → y | keep |
| **Modules and the lockfile** | | | | |
| `modules` (bare) | docs/why/trust.md:72 ("`doze modules` output") | y (prints help) | n → y | add: lists the pins, newest that fits, users |
| `mod` (alias of `modules`) | cli:404 | y | n → y | keep |
| `modules upgrade [type…]` | cli:402; docs/hcl/teams-and-ci.md:46; site/components/module/TooOld.astro:89 | y | y (as `upgrade`) | keep; the proposal's bare `upgrade` corrected to `modules upgrade` (`update` is doze itself) |
| `modules upgrade --check` | cli:415; docs/hcl/teams-and-ci.md:45; site/pages/registry/trust.astro:200 | y | n → y | keep |
| `modules docs <module>` | site/pages/registry/index.astro:228 (as new); site/pages/changelog.astro:243 (0.1.1); docs/guides/upgrading-from-0-1.mdx:257 (as gone) | n | n → y | add; the website contradicts itself (fix 1) |
| `modules docs --web` | site/pages/registry/index.astro:228 | n | n → y | add |
| `modules verify` | site/pages/registry/trust.astro:187, site/pages/registry/[ns]/[module]/[version].astro:248 (both "planned") | n | n → y | add |
| `modules search` | docs/guides/upgrading-from-0-1.mdx:257 (as gone) | n | n | drop: the registry's search page does it |
| `binaries …` | docs/guides/upgrading-from-0-1.mdx:256 (as gone) | n | n | drop: `doze.lock` pins binaries |
| **Other** | | | | |
| `version` | cli:421; site/components/RegistryBar.astro:45, site/scripts/doze-check.ts:62 | y | y | keep, and keep the first line `doze X.Y.Z (os/arch…)`: the site's version picker parses what people paste |
| `completion bash\|zsh\|fish\|powershell` | cli:422 | y | y | keep (dynamic completions) |
| `help [command]` | cli:423 | y | n → y | keep |
| `update` | — (site/pages/registry/index.astro:232 says `brew upgrade doze`) | n | y | keep; under brew it runs (or prints) `brew upgrade doze` |
| `bug` | — | n | y | keep |
| `--dry-run` on `up`, `down`, `wake`, `sleep`, `restart`, `reset`, `migrate`, `task`, `pin`, `on`, `off`, `init`, `fmt`, `modules upgrade`, `trust`, `dns-setup`, `doctor --fix`, `update` | — (only `sync` and `add` have it: cli:98, cli:310) | n (only `sync`, `add`) | partial → y | add (owner decision): [`--dry-run`, command by command](#--dry-run-command-by-command) |
| `__daemon`, `__fetch` | — | y (hidden) | n | keep as internal entry points, undocumented (or a separate binary in the core port) |

Counts (bare `doze` counts as a command; aliases and `completion`'s four shells are not counted; a flag counts once
per command that takes it, the eight global flags once): the website names 38 commands (32 that exist, 4 it lists
as gone in 0.2: `shell`, `psql`, `binaries`, `modules search`, and 2 it advertises that do not exist: `modules
docs`, `modules verify`) and 43 flags; Go has 34 commands (3 hidden: `dash`, `__daemon`, `__fetch`) and 48 flags
(7 hidden); the proposal named 35 commands and 14 flags before this check, and 37 commands and 43 flags after, plus
`--dry-run` on 18 more commands.

### Website fixes

Documentation that names a command or flag that does not exist, or says it behaves otherwise. Not edited here (the
docs live in the doze repo and doze-website).

1. **`doze modules docs postgres --web`** (site/pages/registry/index.astro:228) is shown as a working terminal
   command, tagged "New: --web (docs returns)". Go has no `modules docs` (exit 2, `unknown command "docs"`), and
   docs/guides/upgrading-from-0-1.mdx:257 says it is gone. Until the Rust CLI ships it, mark it planned as
   site/pages/registry/trust.astro:187 does for `modules verify` ("[not a command yet]"), or drop the sentence.
2. **"You meet it only in `doze modules` output"** (docs/why/trust.md:72): bare `doze modules` prints help, no
   versions. Today it should say "in `doze modules upgrade` output and in `doze.lock`"; the Rust `doze modules`
   (added above) makes the sentence true.
3. **`doze migrate postgres`** (site/data/module-extras.ts:218, site/pages/changelog.astro:55): `migrate` takes the
   service's name, not the module's, and `doze add postgres` names the service `db`. Say `doze migrate db` (or
   `doze migrate <name>`). docs/guides/postgres-upgrades.mdx:58 is right: its service is named `postgres` (line 32).

What changes on the website when the Rust CLI ships (correct for Go today, so not fixes yet): the exit-code table
gains 10 and 11 (cli:480–491); `lint` with a module it could not check exits 11 instead of passing (cli:337–339,
cli:345); `doctor` checks the registry (cli:360); the 0.1 → 0.2 table's `doze shell db` row points at
`doze connect db` (docs/guides/upgrading-from-0-1.mdx:255); `--dry-run` joins each mutating command's flag table,
and `--print`/`--print-fix` are described as its aliases; `open --console`, `why`, `update`, `bug`, `modules`,
`modules docs`, `modules verify` get sections.

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
| `connect`, `open --console`, `why`, `update`, `bug`, dynamic completions | core port, after the daemon journal | 5 |
| `--dry-run` on the 18 mutating commands that lack it (the planning half exists for most; `up`, `reset`, `migrate` and `init` need theirs written) | core port, with each command | 3 |
| `modules`, `modules docs [--web]`, `modules verify` | core port, with the module manager | 3 |

About 31 days of CLI work, most of it inside the core port (25 before the coverage check).

## For the owner

1. **F1, now**: the registry hostname has no DNS record; fresh installs of doze cannot fetch modules. Point
   `registry.doze-dev.com` at the registry (or revert the Go constant to a host that serves it).
2. Approve the rules and the command surface (or change them).
3. Decided 2026-11-13: `doze connect` is the name; `--dry-run` is on every mutating command.
4. Fix F2 to F7 in the Go CLI too, since it ships today? Recommended for F3, F4, F5 and F7 (they mislead); F2 and F6
   can wait for Rust.
5. From the coverage check: approve `doze modules`, `modules docs [--web]` and `modules verify` (the website already
   promises the last two); `open --console <name>` instead of `open <name>`; keeping Go's published exit codes and
   adding 10 and 11 (the first draft renumbered them).
6. The three website fixes under [Website fixes](#website-fixes), in the doze repo's docs and doze-website.
