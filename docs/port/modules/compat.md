# Compatibility: doze, modules and what people are told

Agreed 2026-11-13. A person installs doze once; modules, the SDK and doze itself then move on at their own pace. This
says how a stack keeps working through that, and how people learn there is something newer without being nagged.
doze is asking people to leave a tool they know; the bar is that an update never surprises them.

## What protocol 1 (Go) already did, and kept

- A module release records the oldest doze it runs under (`doze:` in the registry index). An older doze skips the
  releases it cannot run and takes the newest it can; only when none is left does it say "valkey needs doze 0.5 or
  newer; this doze is 0.4" (`modindex.SelectFor`, the same idea as Cargo's MSRV-aware resolver and the VS Code
  marketplace's `engines.vscode`).
- `doze.lock` records the doze that wrote it and pins each module by version and hashes; the project's `doze.hcl` can
  say which doze it needs. A teammate's newer lock gets a clear message.
- The app checks GitHub for a newer doze every six hours (Settings › About).

## What it lacked, and the rules now

1. **Nothing breaks a working stack.** Published module releases and engine builds are never deleted (a bad one is
   marked withdrawn); modules are cached locally; the lock pins them. An old doze keeps working offline.
2. **The minimum doze is computed, never typed.** The release tool derives it from the protocol minor the module uses
   (`doze-module-release` today has a typed placeholder, `MIN_DOZE`: it goes). It is part of the signed index
   payload (protocol 1 left `Release.doze` outside the signature). Before a release is published, CI runs it against
   the oldest doze it claims and the newest.
3. **Newer parts degrade, they do not fail.** Hosts and modules agree on the older minor (`version::negotiate`); a
   module meeting an older doze leaves out what that doze cannot use (a newer console block draws as a placeholder
   card, `Node.kind` is a string for this) and the rest works. A new major is rare; when there is one, both release
   lines stay in the index, and doze supports the previous major for **12 months**.
4. **People are told at three levels, never nagged.**
   - Ambient: a dot on the menu bar icon with two lines of what is new. The CLI prints at most one line a day, only
     on an interactive terminal (never in CI or a pipe), and it can be turned off.
   - When it matters: only when something they reach for needs an update, saying what they would gain and the exact
     command for how they installed doze: "valkey 0.4 adds snapshots on sleep; it needs doze 0.5 (you have 0.4). You
     are on valkey 0.3, which works. `brew upgrade doze`".
   - Blocking: only when the project itself requires it (a newer lock, `doze.hcl`), naming who needs what.
   - In the editor: a setting the installed module does not know yet says when it arrives ("`x` arrives in valkey
     0.4, which needs doze 0.5"), not "unknown argument".
5. **Updating is easy and never stops services.** The app installs updates itself after asking once; never forced,
   never behind a login. After an upgrade the new CLI notices the older daemon (the hello carries the doze version,
   not only the control protocol number) and hands over; engines are separate processes and keep running. Protocol 1
   answered a mismatch with "run `doze down`", which stops everything.
6. **Module updates are their own thing.** The app shows "valkey 0.4 available" with what changed and whether it is a
   security fix; a pinned module never changes by itself: `doze modules upgrade`, or one click.
7. **What is new comes from doze.** The update check reads a small signed file from the registry (latest version,
   two lines of news, security flags) instead of GitHub's API: no rate limit, nothing sent but the request.

## Engine builds and the operating system

An engine build may need a newer OS than the one doze runs on: doze-binaries' macOS builds needed macOS 14 until
2026-11, and need 15 from then (the stated floor, `MACOSX_DEPLOYMENT_TARGET`, gated in `smoke.sh`). The index should
carry each build's floor so doze picks the newest build that runs here, the way it picks module releases by doze
version, and says why when none does. Until then, an older macOS gets the error the engine gives.

## Where each part is built

| Part | Where | When |
|---|---|---|
| Computed, signed minimum doze; compat CI before publish | `doze-module-release`, the release workflow | before the first protocol-2 release |
| `negotiate`, placeholder cards, 12-month window | `doze-module-proto` (done), the core | core port |
| Engine and per-field history in manifests (`since` per setting by module version) | registry `meta.yaml` across releases | with the release workflow |
| The three levels of telling, the editor message, the signed news file | core and app | core port |
| Daemon hand-over on upgrade | core (`doze-os` detach + control hello) | core port |
| OS floor per engine build in the index | doze-binaries `dzb manifest`, core downloads | core port |
