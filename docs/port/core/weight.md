# doze core: weight baselines and the C0 probe

Measured 2026-11-14 on the owner's Mac (darwin/arm64). Linux numbers are not taken yet (the Lima VM run is a follow-up).

## Go baselines (doze `origin/main 23b73d6`)

| What | Go | Rust budget (PLAN 6.3) |
|---|---|---|
| `doze` binary, `go build -trimpath -ldflags "-s -w"` | **24.8 MB** (35.5 MB unstripped) | 9 MB line, 10 MB ceiling |
| daemon idle RSS, three process services all asleep | **27.3 MB** | ≤ 8 MB |
| daemon threads, same | **17** | (current-thread tokio plus blocking pool) |
| `doze status` warm, median of 15, daemon running | **6.6 ms** | ≤ 15 ms (already met by Go on this small stack) |
| `doze version`, median of 15 | **5.6 ms** | |
| cold wake of a sleeping process service (connect → first response byte; service is a 7 MB Go HTTP server) | **14–16 ms**, one 38 ms outlier in 5 | at or under Go |
| connect to an awake service, median of 10 | **0.34 ms** | |

How: `bun baseline.ts <doze> <hello-server>` (`weight-probe/baseline.ts`; the probe crate is `weight-probe/`, path deps pointing at a local checkout): a throwaway `DOZE_HOME` under
`/tmp` (the control socket path limit), a stack of three `service` blocks with `sleep = "2s"` and `names = false`,
`doze -y up`, wait for sleep, then measure; `doze -y down` and the home removed. No process was left behind.

The plan's "Go: 27–54 ms" for `status` came from the CLI proposal's measurements on a larger stack with names on; on a
three-service stack Go answers in under 7 ms, so the Rust gate for `status`/`env`/`url` should be "at or under Go on the
same stack", not only "≤ 15 ms".

## The weight probe (dependencies only)

A throwaway binary linking every dependency the plan picks for `doze` and calling into each one so fat LTO keeps what a
real build keeps: tokio (current-thread, net, process, signal, fs), clap derive + clap_complete, serde_json, yaml-rust2,
ed25519-dalek, sha2, ureq 3 + rustls (ring) + rustls-native-certs, hcl-edit 0.10, astro-float 0.9, regex (full), tun-rs
2.8, and from the workspace doze-os, doze-names (resolver, front door, CA: hyper, tokio-rustls, p256, x509-cert) and
doze-module-host / doze-module-proto. Built with the workspace release profile (fat LTO, one codegen unit, strip,
panic = abort).

| Profile | Size |
|---|---|
| release (opt-level 3) | **4.68 MB** |
| release, opt-level "s" | **3.77 MB** |

So the dependencies take about half the 9 MB line, and doze's own code (engine, daemon, config, HCL, CLI) has about
4.3 MB at opt-level 3, or 5.2 MB at "s". Whether that is enough is not yet measured (doze's own code is mostly unwritten); it is not slack to spend: the levers in PLAN 6.3 stay ready
(opt "s"/"z" for cold crates, `regex-lite` where full regex is not needed, one HTTP stack). The CI line for `doze` goes in
with the first `doze` binary that links the engine.

Under O1 the app binary links the same CLI library on top of the GUI, so its line (app plan Q5, ≤ 16 MB) must absorb
roughly this plus doze's own code: about 9 MB on top of GPUI's 12 to 16 MB. **That would break the app's 16 MB line**: the app plan's Q5 line needs restating under O1 (owner item).

## E2: the engine links doze-journal (2026-11-14, Builder B)

The engine now writes the journal and the log store, so it links `doze-journal` (serde, serde_json, `time` with
formatting and parsing) and `doze-logstore` (tokio `sync` and `rt` only), and the module host for the module pool
(`doze-module-host`, already in the probe). `time` is the only new third-party crate. Measured as the probe above plus
`doze-journal` called for real (parse a line, encode it, format its time), same profile, darwin/arm64, same day:

| Build | Size |
|---|---|
| the probe (dependencies only) | 4,681,408 B |
| the probe + doze-journal and `time` | 4,714,512 B (**+33,104 B, +0.7%**) |

Judgement call (the owner leaves small rises to the builder): accepted. 33 KB is under 0.4% of the 9 MB line, and the
journal's Go-exact RFC 3339 with nanoseconds and offsets is what `time` buys; hand-rolling it would save little and
risk the byte-for-byte format the app reads.

## H4: doze-config links psl, the public suffix list (2026-11-14, Builder A)

`doze-config` refuses an app domain that is a public suffix (`co.uk`, `github.io`), as Go's
`golang.org/x/net/publicsuffix` does. The owner chose `psl` (the list compiled in, no runtime file). Measured
alone, the release profile above (fat LTO, one codegen unit, strip, panic = abort), darwin/arm64: one binary that
reads an argument and prints a bool, with and without one `psl::domain` call.

| Build | Size |
|---|---|
| without psl | 302,576 B |
| with `psl` 2.1.254 | 1,179,088 B (**+876,512 B**) |
| with `psl`, `opt-level = "z"` for the psl package | 1,179,088 B (no change) |

So psl costs **0.84 MB**, about 9% of the 9 MB line. The list is compiled into match code, so opt-level does not
shrink it. That is not the "small rise" the owner left to the builder, so the number is reported for an owner call:

- **Keep psl** (simplest; the list updates with the crate).
- **Embed the list as text** (about 230 KB raw, less compressed) with a small matcher of Go's rules. This saves
  roughly 0.6 MB, and is a port to own and test against `publicsuffix` on the corpus.

The engine wiring (`doze-config` → `doze-engine`) adds no new third-party crate.

## H4 follow-up: the public suffix list owned, psl dropped (2026-11-14, Builder A)

The app binary is near its 16 MB line and links doze-config, so the list is now doze-config's own
(`src/psl.rs`, `data/public_suffix_list.fc`): the publicsuffix.org list (VERSION
`2026-11-13_14-44-35_UTC`, commit `71c235dd`) front-coded to 78,230 B and decoded into a table on the
first `domains` check. Same probe as above (one argument in, one bool out; release profile, darwin/arm64):

| Build | Size | Added |
|---|---|---|
| no lookup | 302,560 B | |
| `psl` 2.1.254 | 1,179,088 B | +876,528 B |
| **`doze_config::psl`** | **385,168 B** | **+82,608 B (0.08 MB)** |

The first lookup costs about 0.9 MB of resident memory for the table (2.9 MB peak RSS against 2.0 MB for
the probe without a lookup) and no measurable time at process start; a stack with no app domains never
decodes it. Matching is Go's `x/net/publicsuffix` loop (wildcards, exceptions, ICANN and private
sections, parent-only nodes, IP addresses). Evidence: `psl` as a dev-dependency oracle on every rule and
300,000 random names (one reviewed difference, where psl and Go part, pinned to Go); and, built from
Go's own list revision, 131,275/131,275 names identical to Go. Refresh: `bun
crates/doze-config/data/refresh-psl.ts`.
