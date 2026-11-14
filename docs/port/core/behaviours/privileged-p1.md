# P1: privileged and DNS (behaviour catalogue)

Distilled from Go `cmd/doze/{dnssetup,trust,doctor}.go`, `internal/adminprompt` and doze-names `setup.go`/`trust.go`
at doze 23b73d6, and from the app's command source (`doze-desk/src/settings/{cli,repair,hands,machine}.rs` on
`app-finish`), which runs these commands. Each item names what holds it.

The machine's parts and the repair are data in the new crate `doze-doctor` (checks, the domains' source, leftovers,
the one-prompt repair); the scripts are doze-names' (N5); `doze-cli` runs them through its `Host`, so every test runs
the commands in-process with a recorder in place of root (`crates/doze-cli/tests/privileged.rs`).

| # | Behaviour | Held by |
|---|---|---|
| 1 | `dns-setup` installs the zone's setup in **one** script under one prompt; `--dry-run` (aliases `--print`) prints Go's `# <what>` and a pasteable `sudo sh <<'DOZE_SETUP'` and hands nothing over; `--json` gives `{dry_run, applies_with, nothing_to_do, what, shell, script}` | `privileged.rs` `dry_run_shows_…`, `a_run_hands_over_one_script…` |
| 2 | `dns-setup --check` reports each part (`names: network`, `names: resolver route` on macOS; doze0, the route, the unit, hosts, sysctl on Linux; the NRPT rule on Windows) and exits 1 with `doze dns-setup (one sudo)` when one is missing | `privileged.rs` `check_reports_…`, Lima run |
| 3 | `dns-setup --uninstall` removes the setup, every recorded route and every doze authority's trust in one script, then the authority's files and the routes record | `privileged.rs` `uninstall_…`, Lima run |
| 4 | `DOZE_ZONE=off`: `dns-setup` refuses (DZ0407, exit 1), `--check` passes, the repair has nothing to do | transcript `setup-zone-off.txt` |
| 5 | From a terminal the script runs under `sudo` ("doze needs sudo once: …" on stderr); from the app (`DOZE_ADMIN_PROMPT=1`) behind the system's prompt (osascript on macOS, pkexec on a Linux desktop, UAC on Windows); already root, by its own shell | `privileged.rs` `the_app_gets_the_system_prompt…`; `machine::run_as_root`; owner run 4 (app) |
| 6 | A cancelled prompt says `the administrator prompt was cancelled` (the words the app's `doze_outcome` looks for), exit 1; a failed script says what it was for and why, exit 10 | `privileged.rs`, `doze-os` `cancel_and_failure_are_told` |
| 7 | After the zone's script, the run waits up to 6 s for the network daemon launchd starts a beat later, then reports each part; a script that ran but left a part missing is an error (exit 10), never a tick | `privileged.rs` `a_run_hands_over_one_script_then_checks` |
| 8 | `trust` routes this stack's domains and trusts the doze authority when there is one; stale routes go in the same prompt; `--check` (exit 1 with the fix), `--dry-run`, `--remove` (this stack's routes unless another stack declares them; with the last stack, every doze authority's trust and the authority's files), `--rotate` (a new authority, trusted, whatever state the old one is in); `--rotate --remove` is a usage error | `privileged.rs` `trust_routes_…`, transcript `setup-zone-off.txt`, Lima run |
| 9 | `doctor` reports `names: …` (each setup part, who answers the resolver and the front door), `domains: …` (each route, the authority, stale routes, who serves https), `leftovers: …` (names of dead processes, an authority no stack needs), the daemon (running or stopped), and keeps F5's registry check; every check carries `ok` (Go's machine contract, which the app reads) and a failing one its `fix` | `privileged.rs` `check_reports_…`, transcripts `doctor*.txt`, `doze-doctor` tests |
| 10 | `doctor --fix` (and `--fix --dry-run`, `--print-fix`) repairs the zone **and** this stack's domains in **one** script under one prompt (Go asked once per part) | `privileged.rs` `the_repair_is_one_prompt…`, `doze-doctor` `parts_run_in_order…` |
| 11 | The network daemon the setup installs is this program run as `doze __doze-netd <registry> [<uid> <gid>]`, dispatched before anything else in the process; `doze-app __doze-netd` is the CLI word too, so the app's copy can be the daemon | `privileged.rs` `the_daemon_word_is_a_command`, owner run 3 |

## Differences from Go, by design or until a later phase

- **One prompt for the repair** (better than Go): `doctor --fix` combines the zone's setup and the domains' trust; each
  part runs in its own subshell so one part's `set -e` cannot leak into the next, and a failing part stops the run.
- **Where a stack's domains come from (until H3)**: Go's `trust` reads `domains` from doze.hcl. The Rust core does not
  read doze.hcl yet, so `trust` and doctor use the routes record (`routes.json`, shared with Go), which the stack's
  daemon writes when it comes up (Go `daemon/ingress.go`). A stack that never came up has none yet, and `trust` says
  so. For the same reason `trust` never makes the authority (the daemon does, when a domain is served from it) and
  doctor marks a missing authority "not checked" rather than failing, and the per-domain public-suffix and
  own-certificate checks wait for the config.
- **Exit codes**: Go exits 1 for every failure here; Rust gives a failed privileged script or an unusable setup exit
  10 (the machine's problem) and a cancelled prompt exit 1.
- **Not yet in doctor**: the leftovers sweep of processes, sockets and data folders (the app's Settings › Machine has
  it, `doze-desk/src/settings/leftovers`); the port-holder lines and the system-resolver probe of a stack's names
  (both need the stack's ports and names from doze.hcl, H3).

## After the independent audit (2026-11-14)

Fixed: the script always goes through the CLI's `Host` (a root process no longer bypassed a test's recorder);
`doctor --fix` exits 10 when a part, names or domains, is still missing after the run; `trust --rotate` is refused with
`--check` or `--dry-run` (it used to make the new authority before looking); a plain `trust` with a broken authority
says so (exit 10, fix `doze trust --rotate`); the authority is trusted only when a domain is served from it (the daemon
registers each domain with the person's own certificate, if any: Go `usesDozeCA`), so a left-over root is never added to
the system's store; a not-checked line is `ok: false`, so the app never shows it as a tick; `DOZE_ADMIN_PROMPT=1` with no
system prompt (a Linux machine without a desktop) falls back to sudo, as Go's `Requested()` does; what the script prints
goes to stderr, so `--json` stays one envelope; a UAC "no" reads as a cancel; `trust --remove` forgets the stack in the
routes record only after its routes are gone; a part that cannot be planned (no setup on this system, a broken
authority) is said and the rest is still repaired; `doctor --dry-run` without `--fix` is a usage error; `data.ok` is
back in `doctor --json`; the owner script keeps the System keychain's doze roots and trusts them again afterwards.

More differences from Go, by design:
- **`doctor` outside a project** reports a missing `.doze` setup as a warning, not a failure: a machine that never ran
  `dns-setup` is not broken. Inside a project the parts fail, until doze.hcl is read (H3) and they can be limited to a
  stack that uses names, as Go did.
- **`--rotate` with `--check` or `--dry-run`** is a usage error (Go rotated before looking).
