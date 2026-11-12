# Protocol 2: how doze talks to a module

Status: built 2026-11-12 (step 1 of PLAN.md): `crates/doze-module-proto`, 16 tests and a fuzzed frame decoder (1.8 M runs, no crash). Owner: "if the golang one had limitations, you can make it better".
Evidence: the three audits of 2026-11-12 (what the Go core calls, the SDK, the modules), S0 and W0 (transport numbers on
five targets). Encoding (minicbor, numbered fields) and transport (`doze-os::channel`) are settled; this note is about
what goes over them.

## What was wrong with protocol 1, and what protocol 2 does instead

| # | protocol 1 (go-plugin + gRPC + gob) | protocol 2 |
|---|---|---|
| 1 | **Lockstep versions.** The core selects only modules whose `protocol` equals its own; any change to the contract is a flag day for every published module | **Ranges.** `hello` carries the major the side speaks and the minor it knows. Same major is required; within it, fields and calls are added, never changed or removed, and old builds ignore what they do not know (minicbor rules proven in S0). The registry index records the major and the minimum minor a module needs |
| 2 | **Static facts asked per call.** `Type`, `Capabilities`, `DescribeConsole`, `Actions` are RPCs; `BackendSocket` is called on every status and boot | **One manifest in `hello`**: type, category, versions and the exactness rule, default port, boot budget, backend address template, supervised or lazy, versionless, schema, example, console layout, admin actions, capabilities. Cached per module version |
| 3 | **Capabilities are strings** re-mapped to Go interfaces; an accident of the adapter makes Console unreachable without Admin; Inventory and Prune ride on Converger | **A typed capability set** derived from the traits the module implements, each independent |
| 4 | **Config is shipped as source.** The whole file goes to the module, which re-parses it with its own HCL and cty, re-evaluates variables (no functions), and returns an opaque gob blob the core stores and sends back on every call | **Schema-declared, core-evaluated config** (PLAN.md D1): the core sends a typed `Value` tree with source spans; modules never link HCL. Nothing opaque crosses the wire |
| 5 | **Errors are strings**, except decode diagnostics smuggled as a gRPC status detail | **Structured errors on every call**: kind (invalid, unsupported, not found, unavailable, timed out, cancelled, internal), message, diagnostics pointing at value paths, retryable |
| 6 | **Callbacks need a second server.** The host's `Fetcher` is served over go-plugin's broker on a separate connection, valid only during `Resolve`; an old host gets a special "upgrade doze" stub | **Callbacks ride the same connection**, scoped to the call that may make them: a module call to `fetch.*` names the host call it serves, and the host refuses it once that call has ended |
| 7 | **Client hand-off is a side channel**: a second unix socket, a JSON header, then a line protocol (`boot`, `handled`, `splice`, `backend`, `bootfail`) | **The handle travels on the control channel** (SCM_RIGHTS / WSADuplicateSocketW, `doze-os`), and boot, acquire and release are ordinary calls |
| 8 | **Logs are scraped**: hclog JSON on stderr, and the host must drain go-plugin's stdio stream or the module blocks (spike: 4,000 lines) | **Log frames**: level, message, fields, the instance they concern; rate-limited by the module SDK. stderr is only for a crash |
| 9 | **No deadlines.** A hung `Resolve` or `Converge` hangs the wake | **Every call carries a deadline**; the host enforces it and sends `cancel`; the module sees cancellation as a future that resolves |
| 10 | Streams (`Tail`) rely on gRPC's window; the host gets rows as fast as the module makes them | **Credit-based streams**: the host grants N items, the module waits for more credit. A slow console never grows memory |
| 11 | **The module cannot speak first.** It can only answer | **Module events** (opt-in): an instance's health changed, it needs a restart, a long call's progress (e.g. "downloading postgres 17, 40 %") for the CLI and the app |
| 12 | **Shutdown is a kill** (go-plugin `Kill`), and a killed host leaves modules running (spike) | **`shutdown{deadline}`**, then the channel closes; a module also exits on its own when the channel ends (host death: 1 to 3 ms, S0/W0) |
| 13 | **One warm process per type for the daemon's life**, 8 to 15 MB each in Go | Rust modules idle at 1 to 6 MB and start in 0.1 to 4 ms (S0, W0), so the host may stop an idle module and start it on demand |
| 14 | AutoMTLS ceremony available, never used, P-521 only (plugin spike) | None: an OS channel only the two processes hold; on Windows the host verifies the client's pid |
| 15 | `Object.Hash` is SHA-256 of a Go struct's JSON: language- and field-order-dependent | Hash over canonical CBOR of the object's declared fields, defined in the protocol, so a Rust module and a future one agree |

## Frame

`[len u32][id u32][kind u8][flags u8][method u16][body][handle trailer]`, little-endian, body ≤ 16 MiB (a bigger
one is refused before it is read). Kinds: `call`, `reply`, `error`, `item`, `end`, `cancel`, `credit`, `event`, `log`.
Flag bit 0: a handle rides with the frame. The decoder is sans-IO (bytes in, frames out) and fuzzed.

## Calls

| call | direction | notes |
|---|---|---|
| `hello` | module → host, first frame | versions, manifest; the host answers with its versions and the module's limits (max concurrent calls, log rate) |
| `decode`, `network`, `connection`, `attributes`, `backend_url` | host → module | pure functions of the instance; cacheable by the host |
| `resolve` | host → module | may call `fetch.resolve_major` and `fetch.ensure` while it runs |
| `provision`, `plan`, `template.ensure`, `template.clone` | host → module | |
| `converge`, `objects`, `prune` | host → module | separate capabilities |
| `restart_policy`, `health` | host → module | supervised modules |
| `resources`, `run_action` | host → module | admin |
| `tail` | host → module, stream | credit-based |
| `wire.accept` | host → module, with a handle | the module may call `wire.boot`, `wire.acquire`, `wire.release` while it runs |
| `shutdown` | host → module | |
| `event`, `log` | module → host, one-way | |

## Done when (step 1)

The `doze-module-proto` crate: frame codec (fuzzed), all messages with their field numbers frozen in one place, version
negotiation, credit accounting, and the async peer on `doze-os::channel`; tests: a compatibility test in which a
"2.0" and a "2.1" build talk in both directions, deadlines and cancellation, credit back-pressure, callbacks scoped to
their call; green on five targets.

## Found while building it (keep)

- **`#[cbor(index_only)]` enums break evolution.** An unknown variant of an `index_only` enum fails to decode even inside
  `Option` (EndOfInput); only the default one-item-array form reads as `None`. S0 had proven the rule for the default
  form only. `tests/compat.rs` caught it; no enum in the protocol uses `index_only`.
- **A handler that keeps only `call.cancel` drops the responder at once** (edition 2021+ closures and async blocks
  capture fields precisely), and the peer then answers "dropped without an answer" for it. That safeguard is the
  point, but the SDK should hand handlers a context that keeps the call alive.
- An unknown capability bit from a newer module is kept, not refused; an unknown callback is refused with a structured
  `Unsupported` and the channel stays usable (`tests/compat.rs`).
