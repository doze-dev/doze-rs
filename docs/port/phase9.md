# Phase 9: the console

Status: **slice 9a BUILT (2026-11-12, branch p9a, audits pending; see its row below). The rest is PLAN. The owner has settled D1 (the console is in the default build; the size gate rises by about 1 MB with headroom) and asked for the canvas boards to be corrected first (in progress).** Evidence: `research.md` beside this file (every claim about the boards, the broker and the
weight is there, with files and numbers). The design is the canvas "doze-kafka console direction" (94 screens), its decisions are in
`docs/port/console/{DIRECTION,GROUNDING,REALITY}.md`, and the measured weight is the spike in `weight/`. Phase 8 is built and its audits are pending; Phase 9
starts after they close (the console reads Phase 8's `Caller` and `Authorizer` and mounts its guard).

Goal: a developer opens one URL and sees their Kafka: where to point a client, what the apps are sending right now, what is in a topic, whether a group is
keeping up, why something was refused, what a config does. It is the 94 boards of the canvas (less the 12 Lab boards, which are a later feature),
built as server-rendered pages from typed view-models, reading and acting on the broker through the same handlers and the same authorization as a Kafka
client, in the same process, behind a cargo feature whose weight is measured and gated.

Done when: (1) every board of the map below except Lab passes the board-diff gate and has been looked at, rendered next to its board, by someone other
than its author; (2) the console's reads and writes all go through `Broker::handle` with the console principal, and a test proves the set of write APIs
the pages can issue is exactly the set of `data-op` names on the boards; (3) the recorder, group event ring, connection table and log facts exist, are
bounded, cost nothing idle, and are redacted (the extended planted-secret test passes); (4) every non-GET is refused without a same-origin proof, every
request is refused for a `Host` the guard does not allow, a record value of `<script>` appears escaped everywhere it is shown; (5) the weight gates of both
builds hold (the lean build at today's numbers, the console build at its own, recorded with their reason in `weight.md`); (6) the replay is 58 of 58
(slice 9i), interop 23 of 23 including the SASL and TLS cells with the console on; (7) two independent audits say done and their fixes are re-audited.

## What the research settled

- **The console can be a Kafka client in the same process with no new protocol code.** `Broker::handle(listener, caller, head, request)` is the one entry
  point the connection uses and it already takes Phase 8's `Caller`. 11 APIs are issued by the boards' buttons and about 12 more are read.
- **The protocol does not say everything the boards show.** Many of the 63 screens the canvas tags `now` need a read accessor on private broker or storage types
  (partition files and sizes, group timeouts and "has rejoined", the planner, uptime and the last stop: the "small" rows of research section 1), and 19 need a capture
  that does not exist: the request recorder (wire, request detail, refusals, clients, first request: 14 screens, plus the docks on the topic and group pages), the group event
  ring (3 screens), the cleaner's last round (2). The recorder is the largest single piece (research 2.3).
- **The weight is real but bounded.** On darwin the console stack is +309 KB of binary and 25 packages (32 lines in the CI's own count) for axum and
  maud, the pages' own code is an estimate of 100 to 200 KB, assets about 200 KB for the new design, idle +0.3 MB. The Linux binary is already 0.25% under
  its limit, so **the console cannot live inside today's gates**; it needs a gate of its own, like `tls` has. cargo-deny passes unchanged.
- **maud is free at runtime, axum is not, and hand-rolling HTTP saves 0.3 MB and 22 packages at the price of about 700 lines of parser to fuzz and own.**
  The Phase 10 registry needs the same stack.
- **The canvas is stale in four places** (research 5): it carries doze names (`kafka.larder.doze`, `via doze`) that the owner removed, it says "no SASL yet"
  and its Access page describes a broker that refuses SASL flags, its Live pages promise a 3-hour window a 2,000-row ring cannot promise, and its kit
  scaffolding (`Reached from`, `Other moments`) is not product.
- **A console with no login is open to cross-site form posts even with the Host guard.** Origin checks on every non-GET are part of the phase (research 6).
- **Two items the docs hand to Phase 9 are not console work:** DescribeCluster and the KIP-714 telemetry pair (the 11 replay failures that remain).

## Decisions taken (each can be changed by the owner before its slice starts)

| # | Question | Recommended default | Why, and what changes if it is overturned |
|---|---|---|---|
| D1 | Is the console in the default build? | **DECIDED BY THE OWNER (2026-11-12): yes. The `console` cargo feature is on by default, and the default weight gate rises by about 1 MB with headroom** (the owner: "add the extra 1 MB and give ourselves a little more headroom"). Proposed numbers, to be set from the slice 9a measurement on the Linux CI job: size **6.5 MB** (Phase 8 closed at about 5.17 MB on Linux by the darwin steps, which was already over today's 5.15 MB; the console is 0.8 to 0.95 MB; about 6.1 MB, plus about 6% headroom), packages **165** (the measured count plus 4, as at 8a; the console adds about 32 lines by the CI count), idle RSS **8 MB** unchanged. The reason is written in `weight.md` when the gate moves, not before. A lean build (`--no-default-features`) stays as one cheap CI job at the Phase 8 closing gates so that the broker without a console cannot grow unnoticed | The first Linux measurement may move the numbers; the rule stays: the gate is set from a measurement plus headroom, with the reason written. If the console proves heavier than the estimate, split or simplify a slice before raising the gate again |
| D2 | HTTP stack | **axum 0.8 (`default-features = false`; `tokio`, `http1`, and `form` unless a 20-line decoder is preferred) and maud 0.27**; `tower-http` not used | Measured: +309 KB, +25 packages, cargo-deny clean. Hand-rolled saves about 0.3 MB and 22 packages and adds about 700 lines of HTTP to fuzz; Phase 10 needs HTTP too. If hand-rolled is chosen: slice 9a grows by 1 session and a `fuzz:parser` target (request head, form body, query); everything else is unchanged because pages are `String`s |
| D3 | Embedding assets | **A manifest module of `include_bytes!` lines (`assets.rs`, about 40 lines) and a test that walks `assets/` and fails on a file not in the manifest**; no crate | rust-embed adds 8 packages for nothing here and include_dir adds 2 and 16 KB; the PLAN named rust-embed. If either crate is preferred it is a one-file swap behind `assets::get(path)` |
| D4 | How pages update | **One server-sent-events stream per page that is on screen (the owner's 2026-11-09 decision), carrying only "this fragment changed" ticks; the page then fetches the fragment (an htmx request with an ETag so an unchanged fragment is a 304). The source of ticks is one shared signal that runs only while someone is connected; from 9e it is the recorder and the group ring, before that a 2 s floor timer.** Own about 40 lines of JS over `EventSource` (no htmx SSE extension) | The PLAN's text says htmx polling every 3 s. Polling is stateless and simplest to test; the stream means one connection and no per-fragment timers, and it is what the boards ("live sentence", "N held back") are drawn for. Until 9e the stream is polling on one timer, said honestly. If polling is preferred: `hx-trigger="every 3s"` on each fragment, `stream.rs` and `tick.rs` are not written, the Pause and "held back" logic stays client-side either way. Shutdown rule for the stream: it selects on the server's cancellation token, or graceful shutdown never ends |
| D4a | The client layer (added 2026-11-12, after the owner asked whether htmx is the right tool in Rust) | **DECIDED 2026-11-12 by the spike (`phase9/client-spike.md`): our own script (`assets/console.js`, about 100 lines over `EventSource` and `fetch`, no third-party JavaScript).** All three candidates passed the same ten behaviour tests in a real Chrome (focus, caret and typed text kept, scroll kept, Pause, reconnection, forms with and without JS, dialogs, shutdown with ten open streams under 1 s, an unchanged fragment costing no swap, a stale slow answer not overwriting a newer one); weight and the security headers decided: the own script is 4 KB against htmx 55 KB (18 KB gzipped) and Datastar 36 KB, adds 34 KB to the binary against 100 KB, and runs under `default-src 'self'` with no `unsafe-eval` and no inline script, which Datastar does not (it evaluates attributes as code, and the console shows hostile Kafka data). htmx is not a Go thing: it is browser JavaScript that works with any server; it simply was not needed. Traps the spike found, now tests: a browser turns a 304 into a 200 for `fetch`, so the script sends `If-None-Match` itself; `EventSource` leaks through the back/forward cache unless closed on `pagehide`; reconnect forever, not ten times; a slow stale answer must not overwrite a newer one. The seam is `ui/client.rs` plus `console.js`; the flip conditions (the client logic passing about 200 lines, or htmx 4 going stable with its own morph) are in the spike report | The WASM routes are not candidates (a wasm toolchain and 0.3 MB or more against a strained size gate; the board-diff gate compares server-rendered HTML). The page code is the same whatever the client layer: only `console.js`, `ui/client.rs` and the manifest change |
| D5 | The console's identity | **Principal `User:__console`, reserved (refused as a `--sasl-user`, as a credential name over the API, and by name in `--acl-super-user`), host `127.0.0.1`; a super user by default** (the Go edition's `__console` login, and what SEC-188's text already tells the operator). Every console call carries it into `Broker::handle` | "Same handlers and same authorization as a client with a console principal" is exactly this. Alternative: not a super user, the operator grants it ACLs (`--acl-super-user User:__console` is then the one flag that restores the default); a refusal then shows on the page as a client would see it. The cost is a first-run console that shows 'not authorized' on an ACL-on broker until granted. Recommend the default because the console cannot be exposed beyond loopback unprotected anyway |
| D6 | Who may use the console | **No login in Phase 9** (loopback by default, the guard on `Host`, the start-up rule SEC-188 for exposure), **plus a same-origin proof on every non-GET** (new, `origin.rs`) and security headers | Phase 8 already decided "no login, refuse exposure". A login (HTTP basic against the SASL users, a session cookie) is a phase of its own and invites the "console as a way in" problem. The origin check is not optional: a cross-site form post needs no preflight and the Host is right |
| D7 | Read path | **Everything the protocol can say is a protocol request through `Broker::handle`; `ConsoleView` (feature `inspect` of `kafka-broker`) holds only what the protocol hides, one small method per need, each taking the console `Caller` and checking Describe on the resource** | One code path means one set of refusals and one authorization. `ConsoleView` has `&self` methods returning owned structs, no writer in reach, so it cannot write by construction. If the owner prefers typed methods for everything (no protocol requests): about 1 session more for the accessors, and the "same handlers" claim becomes "same checks, re-implemented" |
| D8 | The recorder | **`kafka_server::Observer`, two-phase (`begin` before the handler with the request, `end` after with the response), implemented in `kafka-console`; 2,000 rows and a 1 MiB budget, grown on demand; folded repeats; `Summarize` per API with a typed exhaustive match; an error table with a completeness test; redaction joins the Phase 8 planted-secret gate** | The only place both the request and the answer are in scope is `exchange.rs`. The honest window is computed, not promised (3 h is gone from the copy). If a bigger ring is wanted it is one constant; a persisted history is not planned (nothing in the boards needs it) |
| D9 | The gate that replaces the Playwright suite | **The board-diff gate** (Testing, below): structural, in CI, no browser: the real page rendered from fixtures and from a real broker compared with its canvas board by a signature; plus an **optional pixel check with Chrome run one page at a time by hand** | The owner retired the 34 specs (2026-11-09); what the suite proved (routes, design fidelity, a11y) is kept by the route table, the signature and an accessibility lint. PLAN.md's Phase 9 gate cell must be rewritten to match (listed below) |
| D10 | The protocol leftovers (DescribeCluster, telemetry) | **Stay in Phase 9 as slice 9i**, independent of the UI, doable first or in parallel | They are what the 11 replay failures are (`oracle.md`, debt I17). If the owner moves them to their own phase, Phase 9 loses about 1.25 sessions and the replay stays 47/11 until then |
| D11 | The canvas corrections | **Before 9a: regenerate the chrome without doze names and with a data-driven security pill; redraw `access-open` for Phase 8 (tag `new`); reword the 3-hour copy on `live-wire` and `live-quiet`; mark the kit scaffolding `data-scaffold`** | Boards that lie cannot be a gate. The owner (or a canvas pass) does this; I did not touch the canvas. If not done, the allow-list carries 94 entries for the top bar and `access-open` is built from this plan's text, not its board |
| D12 | Fonts | **Embed Inter and JetBrains Mono, Latin subset, woff2, OFL (about 90 to 100 KB); no Alpine, no CodeMirror** | The boards are drawn in them; a system stack would make every pixel check differ. 100 KB is half of the asset budget. If fonts are dropped: binary -100 KB and the pixel check is only approximate |
| D13 | Default address | **`127.0.0.1:9093` (the Kafka port plus one) when `--console` is not `false`; busy default port: warn and run without a console (the broker must not fail for its optional UI); an explicit `--console-addr` that cannot bind: exit 2.** `--console-prefix` (default `/`) from 9a: every link is built from a `Base`, so a prefix costs nothing now and a retrofit later | SEC-D63 left the default open. The Go edition's default was a doze name. A prefix is in the PLAN's kept list |
| D14 | Lab | **Not in this phase**; its 12 boards are the map's last row (owner decision, REALITY) | Fault rules in the request path are a feature of their own (4 to 6 sessions) |

## Design

### Crates and layering

```
kafka-storage  + log/segments.rs (LogReader::segments), compact re-export of the planner        read-only additions
kafka-broker   + view/ (feature `inspect`: ConsoleView), groups/events.rs (the ring, 9f),
                 cleaner.rs / retention.rs notes (9g)
kafka-server   + observer.rs (the trait and the Pending token), connection table (9e)
kafka-console  axum + maud; depends on kafka-broker (feature inspect), kafka-server (Observer), kafka-wire, kafka-auth (the guard)
doze-kafka     feature `console` (default), starts the console beside the server, the flags go live
```

Arrows are unchanged: `kafka-console` knows `kafka-broker` and the observer trait, never sockets of the Kafka listener; `kafka-broker` does not know HTTP.
`kafka-console` is generic-free: it holds `Arc<Broker<RealFs>>` (the binary only ever uses the real file system; view-model and page tests need no broker
because pages take view-models).

### The in-process handle and the `ConsoleView` boundary

`Backend { broker: Arc<Broker<RealFs>>, caller: Caller, listener: Listener }` with one method, `call(request) -> Response`, which builds a `RequestHead`
(`client_id: "console"`, a fixed correlation id, the highest version of the API) and awaits `Broker::handle`. `backend/requests/*.rs` has one builder per API
(typed, bounded: Fetch `max_wait 0`, `max_bytes` 256 KiB for a table and the record size cap for one record; Produce through the same validation a client
meets). A `Gather` layer (`gather/*.rs`) turns responses into **view-models** (plain structs: no HTML, no broker types, `Serialize`-free) that pages render.
`ConsoleView` is the second source, only for what the protocol hides (research 2.2). The write set is a single file, `actions.rs`, with an enum of the 11
APIs; the test `actions_match_boards` compares it with the `data-op` names in the committed board snapshot, both ways.

### HTTP, templating, pages

axum 0.8 `Router` built from the route table (`routes.rs`: path, method, mutates, op, screen ids). Layers in order: security headers, host guard
(`kafka_auth::hostguard::allowed`, 403 on refusal), origin proof on non-GET, body limit 1 MiB. maud components are functions returning `Markup`, one file per
family (research section 4); pages are `fn(&Model) -> Markup`; htmx target ids and event names are `const`s shared by markup and tests; an enum for every
class variant so a misspelt `st warn` does not compile. Dialogs and drawers are routes returning their body; with htmx they open into a `<dialog>`, without
JavaScript they render as a full page (a direct URL works). Forms are plain `<form method=post>` with htmx enhancement, so every write works without JS. A
write answers with the result state page (the boards' `-saved`, `-created`, `-refused` states), a refusal being Kafka's own words with the code, never a
generic error page. Function size is capped by clippy (70 lines): each card of a page is its own function.

### Updates: one stream per page

`GET /events?watch=wire,group:fulfilment` (a closed vocabulary of fragment ids) returns `text/event-stream`; each event is `tick` with the ids that changed.
The client script (about 40 lines) re-requests those fragments with htmx, sending the ETag so an unchanged fragment costs a 304 and no swap. The ticker is one
`tokio::sync::watch` fed by a task that exists only while a stream is connected (idle cost zero); before 9e a 2 s timer feeds it, from 9e the recorder
bumps it per row, groups per event, and `topics_changed` (which exists) per topic change, with a 5 s floor for time-based text (ages). Pause is client-side
(the stream continues; the page holds the new rows back and shows the count). Losing the stream shows the banner of the boards and retries with backoff; the
page stays correct because every fragment is also a plain GET. Shutdown cancels every stream.

### Assets and the design system

`design/kit/{paper.css,kafka.css}` are copied from the canvas kit as the source (about 30 KB with the fluid shell and `@font-face`), `assets/` holds htmx,
the own script (`console.js`), the fonts and a favicon, `assets.rs` is the manifest. Cache headers: `max-age` with a content hash in the file name; the
manifest computes it at build time by a `build.rs`-free route (the hash is a `const` written by the test that checks the manifest). CSP: `default-src 'self';
style-src 'self' 'unsafe-inline'; frame-ancestors 'none'` (inline `style="--at:62%"` custom properties are how the ruler and lag bars are placed, as drawn).

### Authentication, TLS and the host guard

No login (D6). The guard is mounted on every route including `/events`; its answer is 403 with a body naming the `Host` it saw and the flag that allows it
(`--console-allow-host`), so the operator is not left guessing. The start-up rule exists (SEC-188) and now has something to act on; `--console-unprotected`
turns into a visible banner on every page ("served beyond loopback, no login"). The console is plain HTTP; TLS for it is not planned. The Access page shows
what the broker enforces and what the console is: its principal, that it is a super user, and that anyone who can reach the console acts as it.

### Reads and actions, and what the console never does

Every read of records is bounded and decoded for display only (codecs through `kafka-wire`, inflate caps as the broker's); a value is shown in full up to
64 KiB, then clipped with its true size. Every write is a form that names its API (`data-op`) and shows the call beside it (the boards' `apicall`); the
console issues nothing the boards do not draw. A write that Kafka refuses is shown with Kafka's code and the broker's own sentence (the handlers' text), plus
the table's "what would pass" when there is one. The console never writes a record, a config or an offset except by the user's explicit submit, and never from
a GET.

### The recorder and its neighbours

Research 2.3 to 2.5 give the contract: the observer trait in `kafka-server`, the ring, `Summarize`, the error table, folding, redaction, the connection
table, the group ring and the log facts. The Live page, the docks, `request-detail` and the refusal drawers read the recorder through `Recorder::since(cursor)`
and `Recorder::get(id)`; the page never sees the ring's types.

## Files

Targets are under 200 lines, none over 300 (soft) or 450 (hard); tests in sibling `tests.rs` or under `tests/`.

| Area | Files |
|---|---|
| `kafka-console` core | `lib.rs`, `serve.rs`, `app.rs`, `routes.rs`, `state.rs`, `guard.rs`, `origin.rs`, `headers.rs`, `assets.rs`, `stream.rs`, `tick.rs`, `base.rs` (links), `format.rs` (sizes, ages, numbers), `words.rs` (the state sentences), `actions.rs` |
| `backend/` | `backend.rs`, `backend/{head,limits}.rs`, `backend/requests/{metadata,offsets,fetch,produce,topics,groups,configs,txn,acl}.rs` |
| `gather/` (broker to view-models) | `gather/{home,topics,topic,record,partitions,groups,group,lag,config,producers,txn,settings,access,log}.rs` |
| `model/` (view-models) | `model/{home,topics,record,groups,config,live,log,settings,access}.rs` and a `fixtures` module (the "larder" world of `kit/world.md`, behind `cfg(test)` and an `examples` feature) |
| `ui/` | `ui/{shell,layout,button,table,form,notice,code,filter}.rs`, `ui/kafka/{ruler,lagbar,story,record,rules}.rs`, `ui/wire/{dock,row}.rs` |
| `pages/` | `pages/{home,connect}.rs`, `pages/topics/{list,detail,messages,record,produce,create,partitions,delete}.rs`, `pages/groups/{list,detail,members,lag,reset,story}.rs`, `pages/config/{view,edit}.rs`, `pages/live/{wire,detail,refusal,clients}.rs`, `pages/log/{ruler,planner,rounds,segments}.rs`, `pages/producers.rs`, `pages/settings/{sections,edit}.rs`, `pages/access.rs`, `pages/refusal.rs` |
| `recorder/` (9e) | `recorder.rs`, `recorder/{ring,row,fold,clients,cursor}.rs`, `recorder/summary/{produce,fetch,topics,offsets,groups,txn,configs,security}.rs`, `recorder/errors/{table_a,table_b,classes}.rs` |
| `kafka-server` | `observer.rs` (trait, `Pending`, `Observed`), `connection.rs` and `connection/exchange.rs` (the two calls), `config.rs` (the optional observer), `connections.rs` (table) |
| `kafka-broker` | `view.rs`, `view/{instance,partition_files,group_detail,producers,log_facts,events}.rs` (feature `inspect`), `groups/events.rs`, `groups/machine/rebalance.rs` and `join.rs`, `leave.rs`, `migrate.rs`, `consumer/*` (the cause argument, 9f), `cleaner.rs`, `retention.rs` (the notes, 9g); in 9i: `handlers/describe_cluster.rs`, `handlers/{get_telemetry_subscriptions,push_telemetry}.rs`, `telemetry.rs` |
| `kafka-storage` | `log/segments.rs`, `compact.rs` (re-export of `judge` and `Plan`) |
| `doze-kafka` | `Cargo.toml` (feature), `run.rs` (start the console), `cli/console.rs` (live; `--console-prefix`), `facts.rs` (the flag table: name, value, default, set) |
| Design | `crates/kafka-console/design/boards/<section>/<id>.html` (94 copies of the canvas sources), `design/kit/*.css`, `design/allow.json`, `design/boards.json` (generated signatures), `design/routes.json` (generated) |
| Tooling | `tools/boards-snapshot.ts`, `tools/board-diff.ts`, `tools/a11y-lint.ts`, `tools/pixels/{board-pixels.ts,package.json,bun.lock}` (never in CI), `xtask`: `console-boards` (renders then runs the diff) |
| Tests | `kafka-console/tests/{routes,boards,live,security,redaction,assets}.rs`, golden files `tests/golden/*.html` for components only |
| CI | `ci.yml`: the lean `weight` job keeps its gates with `--no-default-features`; new `weight-console`; a step `bun tools/board-diff.ts`; `deny` already covers it |
| Docs | `behaviours/console.md` (the catalogue, CON-n, each with its test, in the shape of `security.md`), `weight.md`, `PLAN.md` (the Phase 9 row and ledger), `debt.md` (I17, I20, I21 closed), `oracle.md` (58 of 58) |

## Slices

Each slice is a piece the owner can open in a browser. The gate of every slice includes the common ones: fmt, clippy (also with `--no-default-features`), `lint-size`,
the route tests, the board-diff for the slice's screens, the redaction test, both weight jobs (numbers recorded), replay unchanged (47/11 until 9i, then 58/58),
interop 23/23 with the console on, and **an independent look**: someone other than the author renders each page of the slice next to its board and lists the
differences (the lesson of the SQS pass and of the 21/21 that was 3/19: self-judged fidelity is too generous). That look is done in Chrome one page at a time.

| | Slice | Gate (beyond the common ones) |
|---|---|---|
| 9a | **BUILT 2026-11-12 (branch `p9a`, no audit yet).** What shipped: `kafka-console` (axum 0.8 with `tokio` and `http1` only, maud 0.27, no serde, no tower-http), the route table and `Base` (`--console-prefix`), the layers (security headers, the host guard, the same-origin proof as a pure function with a fuzz target `origin`, a 1 MiB limit), the assets manifest with content-hashed names, Inter and JetBrains Mono embedded, the kit's CSS unchanged plus a fluid override, the `ui/` components with golden files, `Backend::call` with the Metadata, ListTransactions, DescribeTransactions and ListOffsets builders, `ConsoleView::instance()` (feature `inspect`), the reserved principal `User:__console` (SEC-240 to SEC-249), the advertised-listener probe with real dials, the stream skeleton (a task only while somebody listens, shutdown under a second with ten streams open), the feature `console` on by default in `doze-kafka` with `--console-addr` and `--console-prefix` live, and five pages: home (and its quiet and attention states), connect, connect-advertised. **The client layer is our own script (D4a decided by the spike: `client-spike.md`), no htmx**: 4 KB, behind `ui/client.rs`. Not in 9a: the board-diff tool, `boards-snapshot` and the a11y lint (another agent's), `connect-first-request` (9e). Gate numbers and what the boards show that the broker cannot supply: `behaviours/console.md`, `debt.md` section J. Original row: **The crate, the shell, Home and Connect.** `kafka-console` with axum, maud, the route table and `Base`, the guard and origin layers, security headers, assets manifest and the CSS ported from the kit, the `ui/` component library (shell, layout, buttons, table, kv, notice), `Backend::call` with Metadata, `ConsoleView::instance()`, the advertised-listener probe, the stream skeleton with the 2 s ticker, the feature in `doze-kafka` with `weight-console` (the first real weight number), `--console-addr` live, `--console-prefix`, tooling: `boards-snapshot`, `board-diff` v1 and the a11y lint. **First, the client-layer spike (D4a).** Pages: home, home-quiet, home-attention, connect, connect-advertised (`connect-first-request` needs the recorder and is slice 9e; the canvas folded `connect-standalone` into `connect` on 2026-11-12 and added `access-secured`, `access-acl-create` and `access-acl-delete`: the map is 96 boards, not 94; slice 9h takes the three new Access ones) | the 6 boards pass; the guard 403s a foreign `Host` and a cross-origin POST; a record of `<script>` cannot be shown yet but the component test exists; `weight-console` numbers written; the console starts and stops cleanly with an open stream (shutdown test); start-up rule tests from 8g still pass with a console listening; `--console=false` leaves the lean numbers |
| 9b | **Topics.** List, the topic page with Messages (records decoded, read_committed against read_uncommitted, filters by partition, offset, time, key), one record complete, produce with the call beside it, create (three steps), partitions, add partitions, delete records, delete, offline state. `ConsoleView::partition_files()`, `LogReader::segments()` | the 25 boards pass; every write is a protocol request with the console principal (the `actions_match_boards` test); a produce that Kafka refuses shows Kafka's words (INVALID_RECORD on a compacted topic; MESSAGE_TOO_LARGE); the XSS fixture renders escaped in key, value, headers, topic names; a 249-byte topic name fits; an ACL-on broker shows a refusal as a client would see it p9b-write: BUILT 2026-11-12 (branch p9b-write, audits pending): the 14 write boards (create in three steps, produce and its two results, add partitions, delete records, delete), CON-150 to CON-165, debt J41 to J56; fixtures and live renders pass the board-diff. |
| 9b | **Topics.** List, the topic page with Messages (records decoded, read_committed against read_uncommitted, filters by partition, offset, time, key), one record complete, produce with the call beside it, create (three steps), partitions, add partitions, delete records, delete, offline state. `ConsoleView::partition_files()`, `LogReader::segments()`. **p9b-read: BUILT 2026-11-12 (branch `p9b-read`, audits pending):** the eleven reading boards (`topics-list`, `topics-none`, `topics-filtered`, `topic-detail`, `topic-messages-empty`, `topic-messages-filtered`, `topic-readcommitted`, `topic-record`, `topic-record-binary`, `topic-partitions`, `topic-offline`) pass the board-diff in the fixture render and (ten of them) the live render over a real broker; `Batch::each_record_full` in `kafka-wire`, `LogReader::segments`, `LogReader::offline_cause` and `segments_on_disk` in `kafka-storage`, `ConsoleView::partition_files` in `kafka-broker`; five routes, the Fetch/ListOffsets/DescribeConfigs/ListGroups/OffsetFetch/DescribeProducers builders, the record walker with the consumer's aborted-transaction bookkeeping, the value decoder, filters by partition, offset, time and key, the stream and fragments for the list and the Messages page. Rules CON-100 and up in `behaviours/console.md`, debt J21 to J40. | the 25 boards pass; every write is a protocol request with the console principal (the `actions_match_boards` test); a produce that Kafka refuses shows Kafka's words (INVALID_RECORD on a compacted topic; MESSAGE_TOO_LARGE); the XSS fixture renders escaped in key, value, headers, topic names; a 249-byte topic name fits; an ACL-on broker shows a refusal as a client would see it |
| 9a | **BUILT 2026-11-12 (branch `p9a`, no audit yet).** What shipped: `kafka-console` (axum 0.8 with `tokio` and `http1` only, maud 0.27, no serde, no tower-http), the route table and `Base` (`--console-prefix`), the layers (security headers, the host guard, the same-origin proof as a pure function with a fuzz target `origin`, a 1 MiB limit), the assets manifest with content-hashed names, Inter and JetBrains Mono embedded, the kit's CSS unchanged plus a fluid override, the `ui/` components with golden files, `Backend::call` with the Metadata, ListTransactions, DescribeTransactions and ListOffsets builders, `ConsoleView::instance()` (feature `inspect`), the reserved principal `User:__console` (SEC-240 to SEC-249), the advertised-listener probe with real dials, the stream skeleton (a task only while somebody listens, shutdown under a second with ten streams open), the feature `console` on by default in `doze-kafka` with `--console-addr` and `--console-prefix` live, and five pages: home (and its quiet and attention states), connect, connect-advertised. **The client layer is our own script (D4a decided by the spike: `client-spike.md`), no htmx**: 4 KB, behind `ui/client.rs`. Not in 9a: the board-diff tool, `boards-snapshot` and the a11y lint (another agent's), `connect-first-request` (9e). Gate numbers and what the boards show that the broker cannot supply: `behaviours/console.md`, `debt.md` section J. Original row: **The crate, the shell, Home and Connect.** `kafka-console` with axum, maud, the route table and `Base`, the guard and origin layers, security headers, assets manifest and the CSS ported from the kit, the `ui/` component library (shell, layout, buttons, table, kv, notice), `Backend::call` with Metadata, `ConsoleView::instance()`, the advertised-listener probe, the stream skeleton with the 2 s ticker, the feature in `doze-kafka` with `weight-console` (the first real weight number), `--console-addr` live, `--console-prefix`, tooling: `boards-snapshot`, `board-diff` v1 and the a11y lint. **First, the client-layer spike (D4a).** Pages: home, home-quiet, home-attention, connect, connect-advertised (`connect-first-request` needs the recorder and is slice 9e; the canvas folded `connect-standalone` into `connect` on 2026-11-12 and added `access-secured`, `access-acl-create` and `access-acl-delete`: the map is 96 boards, not 94; slice 9h takes the three new Access ones). **Audit fixes (branch `p9a-fix`, 2026-11-12; audit `phase9/audit-9a.md`: DONE WITH MINORS, one MAJOR and 28 smaller findings):** Home describes every open transaction in chunks (the MAJOR, A1) and says "N were not checked" past 10,000; Home is one live fragment that follows the data (A13) and the live fragments never dial (A2, single-flight check, CON-11 corrected); the console listener has a header-read and idle timeout, a request timeout, 256 connections, 64 streams, 503 with Retry-After, counted in the descriptor budget (A12); `--console` is a strict boolean and says when it is off (A10); the console address is bound before the data directory exists (A11); RSS from `VmRSS` (A14); the client script is tested in a DOM (`tools/dom`, A8) and "Copy as" applies to the snippets only (A9), the banner has a way back (A18), Copy works over plain http (A19); the five headers, the body limit, the tick task, the banner through the binary, the reserved name from the file and the env, the default build's tree and the probe's deadlines each have a test that fails when the code is mutated (A3 to A7, A20, the probe mutants); CI runs the console catalogue, the snapshot, board-diff, the a11y lint, the DOM tests, the lean build's clippy and tests, and the `origin` and `form` fuzz targets (A15); words that overclaimed say what was checked; static files answer 304. Every finding is a row of `debt.md` J61 to J90. | the 6 boards pass; the guard 403s a foreign `Host` and a cross-origin POST; a record of `<script>` cannot be shown yet but the component test exists; `weight-console` numbers written; the console starts and stops cleanly with an open stream (shutdown test); start-up rule tests from 8g still pass with a console listening; `--console=false` leaves the lean numbers |
| 9b | **Topics.** List, the topic page with Messages (records decoded, read_committed against read_uncommitted, filters by partition, offset, time, key), one record complete, produce with the call beside it, create (three steps), partitions, add partitions, delete records, delete, offline state. `ConsoleView::partition_files()`, `LogReader::segments()` | the 25 boards pass; every write is a protocol request with the console principal (the `actions_match_boards` test); a produce that Kafka refuses shows Kafka's words (INVALID_RECORD on a compacted topic; MESSAGE_TOO_LARGE); the XSS fixture renders escaped in key, value, headers, topic names; a 249-byte topic name fits; an ACL-on broker shows a refusal as a client would see it |
| 9c | **Groups and lag.** List, group page (classic and KIP-848), members and member drawer, rebalancing state, lag with its words, no commits, ahead of the end, expiring offsets, reset offsets with preview, delete offsets, delete group. `ConsoleView::group_detail()` | the 16 boards pass; reset is refused when the group is not empty with Kafka's words; lag arithmetic is tested against ListOffsets and OffsetFetch fixtures including a topic recreated (committed beyond the end); offsets expiry date matches `GroupLife` |
| 9c | **Groups and lag.** List, group page (classic and KIP-848), members and member drawer, rebalancing state, lag with its words, no commits, ahead of the end, expiring offsets, reset offsets with preview, delete offsets, delete group. `ConsoleView::group_detail()`. **p9c: BUILT 2026-11-12 (branch `p9c`, audits pending):** the 16 boards pass the board-diff in the fixture render and the live render; `ConsoleView::group_detail` (leader, timeouts, last heard, who a rebalance waits for, epochs, the date the sweep takes a commit: `groups::first_expiry` held to `groups::expired`), pure lag arithmetic, reset (preview and write as an administrator commit), delete group, delete offsets, Kafka's own refusals; rules CON-200 to CON-215, debt J91 to J103. | the 16 boards pass; reset is refused when the group is not empty with Kafka's words; lag arithmetic is tested against ListOffsets and OffsetFetch fixtures including a topic recreated (committed beyond the end); offsets expiry date matches `GroupLife` |
| 9d | **Config.** The topic config page (value, source, default, Kafka's doc string, when it takes effect, synonyms), the edit dialogs (value, list append and subtract, the two compaction lags together, reset), saved and both refusals; the effects table. (Broker configs on Settings come with 9h through the same code) | the 8 boards pass; every config key the broker lists has an effect sentence (a completeness test over `topic_config::table`); the "Before, After, what it affects" preview is the real validator's answer, not a copy |
| 9d | **Config.** The topic config page (value, source, default, Kafka's doc string, when it takes effect, synonyms), the edit dialogs (value, list append and subtract, the two compaction lags together, reset), saved and both refusals; the effects table. (Broker configs on Settings come with 9h through the same code) | the 8 boards pass; every config key the broker lists has an effect sentence (a completeness test over `topic_config::table`); the "Before, After, what it affects" preview is the real validator's answer, not a copy p9d: BUILT (8 boards pass both renders; routes `/topics/{topic}/config`, `/config/edit`, `/config/reset`; CON-250 to CON-264; J121 to J130). |
| 9e | **The recorder, then Live.** `Observer` and its two calls in `exchange.rs`, the ring, folding, `Summarize` for every served API, the error table and its completeness test, redaction, the connection table; then Live as a table with filters, Pause, clients, request detail, the three refusal drawers (including the amber Denied class Phase 8 made real), the docks on topic and group pages, the first-request state, Home's "apps spoke" strip, the topics list's "written by" | the 12 boards pass (live-wire, live-paused, live-quiet, live-load, request-detail, request-refused, request-fenced, request-too-large, live-clients, connect-first-request, topic-wire, group-wire); with no observer installed a Produce benchmark is within noise of before (the hot-path claim); a heartbeat-only hour folds to a few rows; no planted secret appears anywhere; the ring never exceeds its byte budget under a 900 KiB `reason` (the Go TRF-1 case); the error-table test fails on a deliberately added error with no row |
| 9f | **The group event ring and the Story tab.** `Effect::Note` with a cause on the five `begin_rebalance` call sites and the 848 epoch bumps, the ring (200 per group, 100 groups), the client's `reason`, the Story page, the rebalance drawer, the flapping state and the calm card | the 3 boards pass; every Phase 4 and 5 oracle scenario, property test and interop group cell is unchanged (a note is an extra effect, never a different transition); cause phrases match Kafka's own reasons where a recording has one |
| 9g | **The Log page and Producers.** `ConsoleView::log_facts()`, `producers()`, `CleanerState` and the retention note, the ruler with gaps, the planner verdict and its rules, last round, deletions, durability, segments table; then topic-producers, the open transaction and what it holds back | the 7 boards pass; the ruler is drawn from a real compacted partition in a test (gaps where records were removed); the planner verdict equals what the cleaner then does (a test runs `judge` and then a round); an idle broker keeps no log facts |
| 9h | **Settings and Access.** The one Settings page in sections (the five dynamic configs with the edit dialogs, version profile, intervals, weight, data, flags), Access from Phase 8's state (mechanisms, ACL default and super users, TLS, users by name, the console's principal, exposure), `facts.rs`, the guard banner | the 5 boards pass (access-open is the redrawn board, D11); no secret on Access (a planted SCRAM password and salt never appear); the flags table equals clap's own list (a test over `Cli::command()`) |
| 9i | **Protocol leftovers.** DescribeCluster v0 to v2, GetTelemetrySubscriptions and PushTelemetry with a bounded store and `--client-telemetry`, their authorization rows (debt I17). **p9i: BUILT 2026-11-12 (branch `p9i`, audits pending):** the three APIs, `--client-telemetry` (default on), the store bounded at 128 instances and 4 MiB; replay 58 of 58; `wire.md` W24 to W35, `security.md` SEC-290 to SEC-293 and SEC-D100, SEC-D101; debt I17 closed for the three | the 11 replay failures pass: replay 58 of 58; `version-matrix` whole; the wire catalogue gains rows |
| close | Two independent audits of the phase against `behaviours/console.md` and the boards, fixes, re-audit, `weight.md` and `PLAN.md` updated | the done-when list |

Order: 9a, 9b, 9c, 9d, 9e, 9f, 9g, 9h; 9i is independent and can run first or beside any slice; 9f and 9g need 9e only for their docks (a dock-less
build of each is possible). Lab is later and separate.

### Screen-to-slice map (all 94 ids)

| Slice | Screens (`src/<section>/<id>.html`) | n |
|---|---|---|
| 9a | home/home, home/home-quiet, home/home-attention, home/connect, home/connect-advertised | 5 |
| 9b | topics/topics-list, topics-none, topics-filtered, topic-create, topic-create-config, topic-create-review, topic-create-refused, topic-created, topic-detail, topic-messages-empty, topic-messages-filtered, topic-record, topic-record-binary, topic-produce, topic-produce-keyless, topic-produced, topic-readcommitted, topic-partitions, topic-add-partitions, topic-add-partitions-refused, topic-delete-records, topic-delete-records-refused, topic-delete, topic-deleted, topic-offline | 25 |
| 9c | groups/groups-list, groups-none, group-detail, group-detail-848, group-members, member-detail, group-rebalancing, group-lag, lag-no-commits, lag-ahead, lag-expiring, group-reset-offsets, group-reset-refused, group-delete, group-delete-refused, group-offset-delete | 16 |
| 9d | config/topic-config, config-edit, config-edit-list, config-edit-lag, config-saved, config-refused, config-refused-value, config-reset | 8 |
| 9e | live/live-wire, live-paused, live-quiet, live-load, request-detail, request-refused, request-fenced, request-too-large, live-clients; home/connect-first-request; topics/topic-wire; groups/group-wire | 12 |
| 9f | groups/group-story, story-rebalance, story-flapping | 3 |
| 9g | log/topic-log, log-planner, log-retention, log-empty; producers/topic-producers, txn-open, txn-holding | 7 |
| 9h | settings/settings-home, broker-config-edit, broker-config-refused, broker-config-saved; access/access-open | 5 |
| Lab, later | lab/lab-home, lab-rule-create, lab-slow, lab-fail, lab-drop, lab-expire, lab-rebalance, lab-active, lab-expired, lab-empty; live/live-lab-rule; log/log-compact-now | 12 |
| | | **94** |

(6 + 25 + 16 + 8 + 12 + 3 + 7 + 5 + 12 = 94; checked against the front matter of the 94 files by script.) `group-story-calm` and `home-instance` are folded
into their parents (owner decisions), so there is no board of their own.

## Testing

### The board-diff gate, concretely

Inputs. The 94 canvas sources are copied into `design/boards/<section>/<id>.html` (each a front-matter JSON line plus main-content markup in the kit's
classes); that is the contract. They are not rendered in a browser by the gate.

1. **Signature extraction** (`tools/boards-snapshot.ts`, bun's `HTMLRewriter`, run when a board changes, output committed as `design/boards.json`). Glyph tokens `[[g:..]]`
   are dropped, `href="@id"` stays as an id. The signature of a page is: (a) the title (last crumb) and the `h1` text; (b) `h2`/`h3` texts in order; (c) for each
   `table.t`: the `th` texts in order and whether it has body rows; (d) tab labels in order without counts, and which is `.on`; (e) every `.btn` and `button`: label,
   variant class (`pri`, `danger`, `fill`, `sm`), and its action: `data-op` name, or `data-ui` kind, or the target screen id; (f) the set of screen ids linked; (g) per card,
   the `dt` labels of `.kv`, the `label` texts of `.field`, the first bold phrase of each `.callout` and `.banner` with its variant; (h) the set of `data-extra` values;
   (i) the multiset of `data-op`; (j) the set of state classes in use (`st ok/warn/bad/refused/info/idle`, `rw ...`); (k) landmarks: `nav`, `main`, the rail.
2. **Route resolution.** A board's link `@topic-detail` is satisfied by a link on the real page whose path matches the route pattern that serves `topic-detail`
   (`design/routes.json`, generated from `routes.rs`: screen id to pattern); this is the "route table is the one list" rule of the Go gate.
3. **Two renders per board, both in CI without a browser.**
   - **Fixture render** (`tests/boards.rs`): the page function is called with the "larder" fixture view-model (the world of `kit/world.md`, the same numbers) and written
     to `target/board-diff/<id>.html`. Strict equality on (a) to (k) except cell text.
   - **Live render** (`tests/live.rs`): a real broker is started in process with a seeded world (a few topics, a compacted one, a group with members, an open transaction,
     a refusal), each route is requested through `Router::oneshot` as the console principal, and the response is compared with the board on structure only: same columns, same
     buttons and ops, same links, same cards; cell text and numbers are not compared. Boards whose data comes from the recorder or ring are seeded by driving real requests
     through the in-memory connection.
4. **Compare** (`tools/board-diff.ts`) prints missing and extra elements per board and exits non-zero on a diff not in `design/allow.json`. The allow-list is a JSON array of
   `{board, selector or field, reason}`; an entry with no reason is an error; entries expire with the slice that created them (listed in `behaviours/console.md`). Standing
   rules, not entries: kit scaffolding (`.from`, `[data-scaffold]`, "Other moments"), glyph SVGs, and anything under `[data-vary]` (times, ages, ids).
5. **What the gate does not say.** It says the page has the board's parts; it cannot say it looks right. That is the independent look in every slice gate and the optional pixel check.

**Pixel check (optional, manual, one page at a time).** `bun tools/pixels/board-pixels.ts <id>` serves the fixture render from an example binary
(`cargo run -p kafka-console --example boards`, fixtures only, no broker), takes one screenshot with Chrome's own CLI (`--headless --screenshot`, width 1440, height read from
the board's PNG, a 20 s timeout, `nice`), masks `[data-vary]`, and compares with the canvas render `shots/<section>/<section>__<id>.png` (per-pixel threshold 0.1, fail above
1.5% of pixels changed, writes a side-by-side to `target/board-diff/`). It refuses `--all` unless `PIXELS_ALL=1`, kills Chrome after each page, and is never in CI. `pngjs` and
`pixelmatch` live in `tools/pixels/` with their own lockfile so the rest of `tools/` stays dependency-free.

### Everything else

- **Route tests** (`tests/routes.rs`, `Router::oneshot`, no sockets): the route table is the router (a route without a row, or a row without a handler, fails); every non-GET
  is 403 without a same-origin proof and 200/303 with one; a foreign `Host` is 403 on every route; every `href` and `src` of every rendered page resolves (no dangling link, no
  missing asset: the "console errors" spec, kept); unknown route is a 404 page in the shell; the stream is tested with a real listener (first event, a tick, shutdown while open).
- **Pure parts get unit, property and mutation tests** (the Phase 7 and 8 method): `origin.rs` (table test and a fuzz target `fuzz:parser origin`), `format.rs`, `words.rs`, the
  ring, folding, cursors, every `Summarize` (golden files, one per API), the error table. Mutation runs on these modules at each slice close; every mutation is checked as
  applied.
- **Hot path**: a Produce benchmark with no observer, with an observer and with the console off, in the slice 9e gate.
- **Redaction** (`tests/redaction.rs`): the Phase 8 planted-secret test extended to the recorder, the stream, the HTML and every `Debug`.
- **XSS and escaping** (`tests/security.rs`): hostile keys, values, headers, topic and group names, client ids and error texts rendered through every page that shows them,
  searched for unescaped angle brackets and attribute breaks.
- **Accessibility, no browser** (`tools/a11y-lint.ts` over the rendered fixtures): `lang`, one `h1`, no skipped heading level, every control has an accessible name, every
  `th` has a scope, no positive `tabindex`, dialogs name themselves (`aria-labelledby`), live regions only where the boards declare a live sentence, colour tokens meet WCAG AA
  (computed from the CSS variables), `prefers-reduced-motion` honoured, status never by colour alone (a word beside it, as drawn). Not automated: screen-reader reading and
  focus order beyond native controls; one manual pass per slice in the browser.
- **Assets**: the manifest test (every file listed, none missing) and a size report (the total is a number in `weight.md`).

## Weight

Measured (research section 7, darwin/arm64, release): axum 0.8 + maud over a tokio baseline is **+309 KB, +25 packages (+32 lines in the CI's count), idle RSS +0.3 MB**;
maud adds about nothing at runtime; rust-embed would add 16.5 KB and 8 packages, include_dir 16.5 KB and 2; a hand-rolled server instead of axum saves about 0.3 MB and 22
packages. cargo-deny (the repo's file, unchanged): advisories, bans, sources and licenses pass.
Estimated, not measured: the pages' own code (100 to 200 KB), recorder and views (about 80 KB), assets (about 200 KB for the new design).
**So the console build is about 0.7 to 0.8 MB larger (Linux 0.8 to 0.95 MB), about 6.0 MB; about 25 more packages (about 156 lines); idle about 5.3 MB, under the 8 MB limit; and the
lean build is untouched.** Today's Linux binary is estimated 0.25% under its limit, so the console is a feature with its own job. Whether it is on by default is D1; the
measurements say it can be (the idle memory limit holds and the size is a gate to set, not a wall). What the recorder adds idle is zero (grown on demand); at 2,000 rows it is
0.4 to 0.6 MB, capped at 1 MiB. Slice 9a measures the first real figure and sets the `weight-console` limits to it plus 5%, written in `weight.md` with the reason; every later
slice records its step.

## Risks

- **Fidelity at scale.** 94 boards, a structural gate and a human look. The gate cannot see colour, spacing or overflow; the optional pixel check can, one page at a time. Mitigation:
  the independent look is in every slice gate, the first slice (9a) proves the signature on 6 boards before 87 more are built on it, and the CSS comes from the kit unchanged.
- **The canvas is not final.** D11: if the doze names and the stale Access board stay, the gate carries allow-list entries that hide real differences. Decide before 9a.
- **The recorder on the hot path.** Two function calls and an allocation per request when installed, one `Option` test when not; the gate measures it. A slow `Summarize` would slow
  every client, so summaries are bounded (strings clipped, results capped at 16) and never copy record bytes.
- **Touching the group machines (9f).** The only slice that changes the state machines; the oracle scenarios, property tests and interop group cells are its gate, and the note is
  an effect, not a transition.
- **Weight creep.** 94 screens of maud code is the unmeasured part. The `weight-console` job fails a slice that crosses its limit; the answer is to split or simplify, not to raise it,
  unless the reason is written.
- **Security of a no-login console** (research 6): cross-site posts, record values as hostile HTML, exposure beyond loopback. Each has a test and a header; the exposure rule exists
  from Phase 8.
- **Stream and shutdown.** An open SSE connection keeps a graceful shutdown waiting for ever unless each stream selects on the cancellation token; tested in 9a.
- **Platform facts.** Idle memory and thread count are not readable on macOS without `unsafe`; the page says so rather than guessing.
- **Fixture drift.** The fixture world, the seeded live world and the boards' `world.md` can disagree. One module owns the fixtures and a test checks its headline numbers against `world.md`.
- **Sizing.** This is the largest phase so far in screens; the estimate below is bottom-up and the plan's old number covered a third of it.

## Not in this phase

Lab and its fault rules (12 boards), the schema registry pages (Phase 10), the transit map, record trace, lag history and the scrubbed window (dropped by the owner), a console login,
TLS for the console, dark and high-contrast themes, a narrow-width layout, per-app credentials, ACL and user editing in the console (Access is read-only), "send again" and raw-bytes
download (Go features with no board), client metrics display (KIP-714 payloads are stored but not shown), the palette's "do something" catalogue, server-side pins, internationalisation,
a Playwright suite (retired), and the Go console's `/cluster` pages as such (their facts are on Home and Settings).

## PLAN.md and the other docs change (not edited here)

- Phase 9 row: replace the gate "Playwright suite (34 specs) passes unchanged in external mode" by "board-diff gate green on 82 boards, independent look, weights, redaction", and the
  estimate "8 to 12" by the estimate below; the design table's "assets via `rust-embed`" by the manifest (D3) and "htmx polling" by D4 if accepted.
- Testing strategy and Phase 9 verification lines: the Playwright references.
- Crate map: `kafka-console` gains the recorder note (it is there); `kafka-server` gains `observer.rs`.
- `weight.md` and `ci.yml`: the console job, the lean job's `--no-default-features`.
- `debt.md`: I17 (9i), I20 (9h), I21 (9a).

## Honest sizing, in sessions (an agent session is about 2 hours)

The plan said 8 to 12 for the whole phase. Phase 8 was planned at 3 to 4 and took about 10 sessions for 7 slices (about 2.5 times, because the plan did not count the 45-API matrix, the
vectors and the audit loop). The same miss is the risk here, so this is built bottom-up from the pieces, and then compared with the 2.5 factor (8 to 12 times 2.5 is 20 to 30).

| Slice | What is in the number | Sessions |
|---|---|---|
| 9a | crate, stack, guard and origin, CSS port, about 25 components, `Backend`, stream skeleton, the feature and `weight-console`, board tooling v1, 6 pages | 2.5 to 3.5 |
| 9b | 25 screens, record decoding, produce, a 3-step create, five write dialogs, `partition_files` | 3 to 4.5 p9o: BUILT 2026-11-13: the overlay system (`ui/overlay.rs`, `docs/port/console/overlays.md`): a record is a drawer over the Messages page and the four write dialogs are dialogs over their parents, by direct visit as well; gentle motion and cross-document view transitions; the jump palette (⌘K), j and k, a copy that confirms. The 9b read builder's choice that a record is a page is reversed by the owner. |
| 9c | 16 screens, KIP-848 and classic views, lag words, reset with preview, `group_detail` | 2 to 3 |
| 9d | 8 screens, the effects table, the teaching text, the real-validator preview | 1.5 to 2 |
| 9e | recorder, two hook calls, 40 summaries, the error table, redaction, connection table, Live, three drawers, two docks, first-request, folding, hot-path benchmark | 4 to 6 |
| 9f | cause on five call sites and 848, the ring, three screens; the risk is behaviour, not volume | 1 to 2 |
| 9g | `log_facts`, `CleanerState`, retention note, the ruler and the planner page (the biggest single screen), producers and transactions | 2.5 to 3.5 |
| 9h | Settings sections and flags table, Access redrawn, guard banner | 1 to 2 |
| 9i | DescribeCluster, telemetry pair, rows in the matrix, 11 replay scenarios | 1 to 2 |
| close | two audits, fixes, re-audit (Phase 7's two audits took about as long as the build of their slice) | 3 to 5 |
| **Total** | | **21.5 to 33.5, most likely about 27** |

The part the old number was about, the console that a developer uses on day one (9a to 9d: Home, Connect, Topics, Groups, Config; 55 of the 94 boards), is **9 to 13 sessions**, which agrees with the
plan's 8 to 12. Everything after it (the recorder, the story, the log, settings, the protocol leftovers and the audits) is the rest. If the owner wants to cut: drop 9g's Log page to the ruler and planner
verdict only (saves about 1 session), defer 9f (the story, saves 1.5), or move 9i out (D10, saves 1.25).
