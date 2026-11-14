# C2b: the CLI's commands that need no Rust daemon (behaviour catalogue)

Distilled from Go `cmd/doze/{add,init,modules,open}.go` and `internal/detect` at doze 23b73d6, and the CLI proposal
(`cli-design`: rules 1 to 8, "Beyond Go"). Each item names the Rust test or oracle that holds it.

**Oracles.** doze-detect: Go's `detect.Scan` + `Scaffold` over 121 projects (120 generated, and `examples/shop`),
recorded through `go build -overlay` so the doze checkout is never touched: 121/121 identical
(`crates/doze-detect/tests/go_oracle.rs`). `add` and `init`: Go's own `doze` binary, offline with its built-in starters,
against the Rust one with a registry whose modules declare the same facts: `add` 6/6 scenarios (23 commands, files and
stdout identical), `init -y` 101/101 files (`crates/doze-cli/tests/go_add_init.rs`). There are no Go goldens for these
commands in `cmd/doze/testdata`, so these recordings are the oracle.

| # | Behaviour | Held by |
|---|---|---|
| 1 | Module knowledge comes only from what modules declare (`meta.yaml` through doze-fetch: label, port, default version, category, `detect`); the signed index's newest engine stands in for a missing default version. No engine table anywhere in the CLI or doze-detect | `go_add_init.rs` (the registry declares Go's table), `nothing_declared_nothing_guessed` |
| 2 | `add <module>` writes Go's block beside what is there (comments, order kept: doze-hcl's editor), a free name (`db2`) when taken, `stack = "<folder>"` and a blank line first in a new file; the result must read as a config or nothing is written (exit 1) | `go_add_init.rs`, transcripts `add`, `add-fresh`, `doze-hcl/tests/edit_sequence.rs` |
| 3 | `add <service>`: a binary on the PATH runs it, a known one runs as a container, anything else is a download to fill in; `--as installed\|container\|download`; `--as` on a module is refused (exit 1) | `go_add_init.rs` (services), transcript `add` |
| 4 | `add`'s output is Go's: the "not a module" preamble, the block as built, the wiring line, `✓ added to doze.hcl — doze lint · doze up` or `(dry run: nothing written)` | `go_add_init.rs` |
| 5 | Better than Go: a name one letter from a module or service doze knows is "did you mean" (exit 4), not a silent service; a taken service name's block takes the free name (Go labelled it with the taken one); the registry unreachable with no copy is exit 10 with the way out (`--as`) | transcripts `add`, `add-offline` |
| 6 | `init` reads go.mod (block and one-line `require`: F6), Go main packages (imports, variables, a port literal), package.json, a compose file (aliases never expanded), a Makefile, `.env`; each block says why | `go_oracle.rs`, transcript `init` |
| 7 | `init` never invents: nothing found writes a commented starter (Go wrote postgres); `--wizard` has no default pick; no module's declaration to be had (offline, nothing cached) is exit 11 with the reason | transcripts `init-empty`, `init-offline` |
| 8 | `init` asks before writing on a terminal (Enter is yes), not under `-y` or a pipe; `--dry-run` prints the file; an existing doze.hcl needs `--force` (exit 1) | transcript `init` |
| 9 | `modules upgrade [module…]`: the modules doze.hcl declares (its shape, offline) or the ones named, resolved again, the build downloaded and verified, pinned through doze-fetch's locked writer keyed by doze.lock's canonical path; Go's table and closing lines | transcripts `modules-upgrade`, `modules-upgrade-offline`, doze-fetch `one_file_reached_two_ways_keeps_both_pins` |
| 10 | `modules upgrade --check` exits 1 when a pin would move; `--dry-run` is the plan (pin from and to); neither writes doze.lock; an undeclared name is exit 4 with did-you-mean; a failure carries Go's words (`no release of postgres supports postgres 17; …`) | transcript `modules-upgrade` |
| 11 | `bug` writes a report in the doze home (version, platform, install; the stack's shape with no values; doze.lock's pins; the daemon log's last 40 lines) and says where; nothing is sent | `tests/bug.rs`, transcript `bug` |
| 12 | `open [dir\|doze://…] [--show]` hands the command to the app (`$DOZE_APP_BUNDLE`, Doze.app in the Applications folders, `doze-app` on the PATH), the folder made absolute; no app is exit 10; over SSH (or Linux without a display) it prints the status table, as Go; `--show` names a declared service or says the nearest | transcripts `open`, `open-no-app`, `open-ssh` |
| 13 | Bare `doze`: the app on a terminal with a display, the status table over SSH or when piped (O2) | transcript `open-ssh` |
| 14 | `update`: the newest doze from the signed release file through doze's one HTTP client; Homebrew runs `brew upgrade doze`; the app and a download get their exact way (exit 1); with a stack running the daemon hand-over is said as pending (D1) and exits 11, and `--dry-run` shows it as pending | transcripts `update`, `update-dry-run`, `update-handover` |
| 15 | Every error ends with a `fix:` line (a group of config diagnostics: one, after the group) | `tests/fixes.rs` over every transcript |
| 16 | Every new command's help ends with examples, each a line doze accepts | `tests/help_examples.rs` |

**Pending until the Rust daemon (core plan D1):** `up`, `down`, `env`, `url`, `run`, `connect`, `sync`, `migrate`,
`lint`'s module pass, `open --console`, and `update`'s hand-over. Each says what it waits for (DZ0001, exit 1, or 11 for
the hand-over).

**Reviewed differences from Go** (each in the test that holds it): `add`'s wiring line comes from the variables a module
declares (url first) rather than Go's per-engine table (kvrocks declares none, so it has none); init's module blocks
come in the modules' name order, and a module found in go.mod says "go.mod requires"; several migration targets make one
`after` list (Go wrote one `after` line per target, which does not read); `${` and `%{` in a written run line are escaped.
