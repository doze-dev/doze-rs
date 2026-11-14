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
4.3 MB at opt-level 3, or 5.2 MB at "s". Whether that is enough is not yet measured (doze.s own code is mostly unwritten); it is not slack to spend: the levers in PLAN 6.3 stay ready
(opt "s"/"z" for cold crates, `regex-lite` where full regex is not needed, one HTTP stack). The CI line for `doze` goes in
with the first `doze` binary that links the engine.

Under O1 the app binary links the same CLI library on top of the GUI, so its line (app plan Q5, ≤ 16 MB) must absorb
roughly this plus doze's own code: about 9 MB on top of GPUI's 12 to 16 MB. **That would break the app.s 16 MB line**: the app plan.s Q5 line needs restating under O1 (owner item).
