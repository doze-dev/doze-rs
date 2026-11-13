# The console in React + shadcn/ui, embedded in the binary

Owner decision, 2026-11-13: rebuild the console's page layer in **React + shadcn/ui**. Hard constraint: **one process, embedded in the
binary** ("I do not want it to be a separate process"). This plan replaces the client decisions D2, D4, D4a and D4b of `phase9.md` and the
htmx/idiomorph/Alpine stack of `client-stack.md`. The broker, the recorder, `ConsoleView`, and the `gather/` + `model/` layer (what every
page shows, computed in Rust) stay; they become the API.

## Why the old stack failed (so we do not repeat it)
1. We hand-built every interactive control (select, combobox, multi-select, menu, popover, switch, refresh picker) and tested them in
   Chrome only. The owner's browser (Zen) is Firefox's engine; the Playwright matrix reproduced "clicking an option does not choose it".
2. Five layers mutated the same DOM (htmx swaps, idiomorph morphs, Alpine re-inits, injected overlays, polling/tails) and broke each other's state.
3. Our gates checked HTML structure and a11y, never behaviour in a real browser. Nothing clicked a toggle in Firefox before the owner did.

## Architecture
```
doze-kafka (one process)
├─ Kafka listener (unchanged)
└─ console listener (axum, same as today: host guard, origin proof, headers, limits)
   ├─ GET /assets/*      the built bundle (JS, CSS, fonts), embedded with include_bytes!, served gzip, immutable cache
   ├─ GET /api/v1/...    JSON: today's view-models (gather/ + model/), serde-serialised
   ├─ POST /api/v1/...   writes (produce, create, config, offsets, ACLs, Lab...), same-origin proof, Kafka's own refusal words
   ├─ GET /api/v1/tail/{live,messages}   SSE, server-paced (Realtime 1 s / Relaxed 3 s / Slow 10 s), cursor resume
   └─ GET /*             index.html (client routing; deep links work), CSP default-src 'self'
```
- **Build:** a `web/` directory at the repo root (TypeScript, React 19, Vite run by bun, Tailwind v4, shadcn/ui components copied
  into `web/src/components/ui` as the shadcn CLI does). `bun run build` writes `crates/kafka-console/web-dist/`, which is
  **committed** (like the vendored htmx was), so `cargo build` needs no bun or Node. A Rust test fails if `web-dist/` is stale (a hash of
  `web/` sources recorded by the build). CI runs `bun install --frozen-lockfile && bun run build` and checks the tree is clean.
- **Libraries (all MIT, cross-browser proven):** React 19, shadcn/ui (Radix primitives), Tailwind v4, TanStack Router (routes, typed
  search params so filters live in the URL), TanStack Query (fetching, caching, refetch intervals, pause on hidden tab, keep previous data:
  no flicker), TanStack Table (+ Virtual for long tables), cmdk (the ⌘K palette, shadcn's Command), react-hook-form + zod (shadcn Form),
  sonner (toasts), lucide-react (icons). Versions pinned in `web/bun.lock` after checking current releases in R0.
- **Contract:** the view-model structs get `serde::Serialize` (and `Deserialize` for inputs); TypeScript types are generated from them
  with `ts-rs` (dev-dependency only: a test writes `web/src/api/types/*.ts`; a stale file fails the test), so Rust and React cannot drift.
- **Live updates (the design in `live-design.md` carries over):** every page is a snapshot (`staleTime` infinite) with the RefreshControl
  in the header: Off (default) / 5 s / 10 s / 30 s / 1 min, persisted, implemented as TanStack Query `refetchInterval` with
  `placeholderData: keepPreviousData` and structural sharing (unchanged rows keep their React keys: nothing re-renders, nothing flickers).
  Only Live and Messages have a tail (SSE, batched, held while scrolled, 200-row window, stable keys). No live numbers in prose: stat tiles.
- **Tables:** one `DataTable` (TanStack Table): fixed column sizes, ellipsis + tooltip, sticky header, stable row ids, Virtual past 500 rows.
- **Overlays:** shadcn Dialog and Sheet (drawer from the right, full height), routed (a drawer has its own URL: deep link opens the
  page with the drawer), focus trap and scroll lock from Radix.
- **Look:** the kit's tokens (paper/kafka CSS variables: colours, radius, type: Inter + JetBrains Mono, already licensed and embedded)
  mapped onto shadcn's CSS variables; the boards stay the floor, improvements allowed (owner rule).
- **Security:** CSP `default-src 'self'; style-src 'self' 'unsafe-inline'` (Radix positions with inline styles), no eval; the
  same-origin proof on every POST via a header the fetch client sets; host guard unchanged; output is JSON (React escapes text: no HTML
  from the server is ever injected; no `dangerouslySetInnerHTML`).
- **Weight:** estimated JS ≈ 150-250 KB gzip (embedded gzip), serde + serde_json ≈ 0.1-0.2 MB; removing the maud page layer saves an
  estimated 1+ MB of Rust code. Net: likely smaller than today's 8.06 MB (darwin). Measured at R0 and every slice.

## Verification (the gate that was missing)
- **Playwright matrix (Firefox, WebKit, Chromium)** in `web/e2e/`, run against the real binary with a real broker and traffic (a
  seeded fixture world for deterministic runs too). Every control type has a scenario that uses real mouse and keyboard input and asserts
  the outcome (value chosen, URL updated, data changed). Every page: no console error, no horizontal overflow at 1024/1280/1440/1920, no
  layout shift on refresh or tail, overlays open/close/Back. This runs before anything is shown to the owner; the lead also clicks
  through in Firefox (the owner's engine) before saying "done".
- **Board fidelity:** the board-signature tool runs on the DOM Playwright captures (not on server HTML), with the allow-list.
- **API:** Rust contract tests per endpoint (golden JSON for fixtures, live tests against a real broker), the planted-secret test over
  every JSON response, hostile strings (React escapes, but the test still checks).
- **Unit:** `bun test` for pure TS (formatters, query-string state).

## Slices (owner sees each one in Firefox before the next starts)
| Slice | Content | Agent sessions |
|---|---|---|
| R0 Foundation | `web/` scaffold, build + embed + SPA fallback + CSP, serde + ts-rs contract, shell (sidebar, top bar, ⌘K palette, RefreshControl, theme from the kit), component inventory (Button, Input, Select, Combobox, MultiSelect, Switch, ToggleGroup, Tabs, Dialog, Sheet, DropdownMenu, Popover, Tooltip, Card, Badge, Table/DataTable, Toast) each with a Playwright scenario in 3 browsers, a `/gallery` route, Home + Connect | 2 |
| R1 Topics | list, topic (Messages + tail, record Sheet, Partitions, Consumers, Log, Producers, Config + dialogs), create, produce, delete, add partitions, delete records | 3 |
| R2 Groups | list, group, members + member Sheet, lag, reset/delete dialogs, Story + rebalance Sheet | 1.5 |
| R3 Live | snapshot + tail, filters (MultiSelect), request/refusal Sheets, clients | 1.5 |
| R4 Settings, Access, Lab | broker configs + dialog, Access + ACL dialogs, Lab (engine already built) | 2 |
| R5 Retire + close | remove the maud page layer, htmx/Alpine/idiomorph, old tools; docs; two independent audits (security + UX in 3 browsers); fixes | 1.5 |

Total about 11 sessions. R1 to R4 run in parallel after R0 (separate directories under `web/src/routes/` and `crates/kafka-console/src/api/`).
The current maud console keeps serving until R5, so nothing is broken in between.

## Decisions taken (owner may change)
- Vite (via bun) rather than bun's own bundler: shadcn and Tailwind v4 are documented on Vite; the output is static either way.
- TanStack Router rather than React Router: typed search params make filters-in-the-URL safe.
- Commit the built bundle so `cargo build` stays bun-free.
- No server-side rendering of React (not needed for a local console; the first paint is the shell + skeleton-free cached data).
- No no-JS fallback (the console needs JS; the owner did not ask for it).

## R0 done (2026-11-13, branch `react-r0`)

**Versions** (npm registry, 2026-11-13; pinned in `web/package.json`, locked in `web/bun.lock`): react and react-dom 19.3.1, vite 8.3.8
(run by bun 1.4.0), @vitejs/plugin-react 6.3.0, tailwindcss and @tailwindcss/vite 4.3.5, tw-animate-css 1.4.0, radix-ui 1.8.1 (shadcn's
components through the shadcn CLI 4.23.0, style new-york), @tanstack/react-router 1.170.46, @tanstack/react-query 5.107.1,
@tanstack/react-table **8.21.3**, @tanstack/react-virtual 3.14.26, cmdk 1.1.1, react-hook-form 7.92.0, @hookform/resolvers 7.0.6,
zod 4.6.7, sonner 2.0.8, lucide-react 1.72.0, class-variance-authority 0.7.1, clsx 2.1.1, tailwind-merge 3.7.0; typescript **6.0.3**,
eslint 10.14.0, typescript-eslint 8.72.2, eslint-plugin-react-hooks 7.1.1; @playwright/test 1.65.0 (`web/e2e`, own lock). Rust: serde
1, serde_json 1 (both already in the build), ts-rs 12.0.1 (dev-dependency). Two deliberate steps back from "latest": TypeScript 7.0
(typescript-eslint supports < 6.1) and TanStack Table 9 (a rewrite of the API; shadcn's data-table and every example are v8).

**Structure.** `web/src/router.tsx` (code routes, the shell: shadcn Sidebar collapsible to icons, the top bar, the ⌘K palette),
`src/pages/` (Home, Connect, the gallery, the planned pages that open the classic console, Not here), `src/components/ui/` (shadcn's,
as the CLI writes them; three edits: Sheet focuses its panel and moves in 200 ms, the overlays make the page `inert`, the palette's
dialog passes `shouldFilter`/`onCloseAutoFocus`), `src/components/app/` (ours: PageHeader, RefreshControl, StatTile/StatStrip,
DataTable, CodeValue, CopyButton, Combobox, MultiSelect, StatusDot, EmptyState, Spans, the sidebar, top bar, palette), `src/api/`
(the fetch client, the Query hooks, `types/` generated), `src/lib/` (format, refresh interval store, recents, search-param parsers,
with `bun test` unit tests). Rust: `crates/kafka-console/src/api.rs` (+ `api/types.rs`, the ts-rs test), `src/web.rs` (+ tests),
`app::console` (the whole console: API, bundle, SPA fallback, `/classic/`), `State::console`.

**The contract.** `GET /api/v1/home` (`HomeResponse`: the HomeView and the words Rust decides), `GET /api/v1/connect`
(`ConnectResponse`), `GET /api/v1/shell` (`ShellView`: address, profile, security chip, locked, unprotected, topic and group counts,
the classic console's address), `GET /api/v1/jump?q=` (`JumpView`: topics and groups containing the text, any case, 200 of each at
most), `POST /api/v1/probe` (`ProbeView`). Errors are `{code, message}` (`ApiError`). Every request needs `X-Console: 1` and no foreign
`Sec-Fetch-Site`; a POST also passes `origin::check`. No memory figure is ever serialised. Catalogue: CON-1000 to CON-1014.

**Decisions.** The gallery is always there (`/gallery`, in the palette, not in the sidebar) as its own lazily loaded chunk: it is the
living inventory the e2e gate drives, and it costs nothing until opened. Search params are parsed by hand (`src/lib/search.ts`), so the
main chunk carries no zod; zod runs jitless (no `new Function` under the CSP). Pages not yet rebuilt say "Being rebuilt" and open the
same place in the classic console. Times on screen are UTC (`Updated 14:32:07 UTC`), as the rest of the console. The record Sheet of
the gallery is the pattern for a routed overlay (a search param; Back closes; a deep link opens it). Modals make `#root` `inert`
(Safari's Tab skips buttons and walked out of Radix's trap into the page behind: found by the WebKit run).

**Sizes.** Main chunk 183,580 B gzipped (595,914 raw), gallery chunk 77,483 B (271,601), CSS 13,289 B, fonts 91,256 B: 366,084 B
embedded. Binary (darwin, fat LTO) 6,161,072 B against 5,797,328 B after the weight pass; 160 packages (limit 161). `weight.md` has it.

**What the browser gate found and fixed before it went green**: the top bar ran 41 px past the window at 1024 (chips did not shrink);
zod's `new Function` probe was refused by the CSP and logged in Firefox; the palette lost Enter while an older answer was on screen,
and gave the focus to nothing on close in Firefox and Safari (a clicked button is not focused there); Copy as replaced the history
entry, so Back did not restore it; the drawer put the focus on a tooltip trigger, so the first Esc closed the tooltip; the WebKit focus
escape above.

**The gate, last run** (Playwright 1.65.0, `workers: 1`, release binary): 29 scenarios per browser (see `web/e2e/tests/`), **Firefox 29/29, WebKit 29/29, Chromium 29/29** against the seeded world, and the same 87/87
with `E2E_URL` against a broker with live traffic (the demo workload). Rust: `cargo test -p kafka-console -p doze-kafka` 888 passed;
fmt, clippy `-D warnings` (default and `--no-default-features`), lint-size (0 over hard), the console catalogue and the assets check
green; `bun run typecheck`, `bun run lint` (0 warnings), `bun test` green. Two allowances in the gate, both written in the specs: a
font download Firefox reports as aborted because the test navigated away first, and Safari's Tab (which skips buttons) is held to
"never the page behind" rather than "always inside the dialog".

**Left for later.** The gallery's TanStack Table and zod make its chunk 77 KB; a page that needs a table in R1 will pull Table into
its own chunk the same way. The main chunk (184 KB gzipped) has room to shrink (lucide icons by path, Router's devtools-free build
is already used). Screenshots for the review: scratchpad `r0/shots/` (Home, Connect, gallery in Firefox, WebKit, Chromium at 1440
and 1024). The fonts are embedded twice until R5 removes the classic console.

## R3 done (2026-11-14, branch `react-r3`): Live

**Pages.** `/live` (`web/src/pages/live/live.tsx`): a snapshot by default with the RefreshControl, a `Snapshot | Live tail` ToggleGroup
(`tail=1` in the address), the speed (Realtime 1 s, Relaxed 3 s the default, Slow 10 s; `doze:tail-speed`), the connection
indicator (grey Snapshot, green Live, amber Connecting/Reconnecting, red Disconnected with a countdown and Retry now), a StatStrip
(requests in the last minute, rows kept of 2,000, reaches back to HH:MM UTC, clients), the refusals folded to one line per (code,
client) in a one-line strip that opens in place, the filters in the address (search, Apps/Topics/APIs MultiSelects with counts,
Refused only, Idle polls hidden), the tape as a DataTable in a box of fixed height (600 px) with columns chosen by the table's width,
the request Sheet (`?request=`: detail, refused, fenced, too large, the Lab tag) and the clients Sheet (`?clients=1`). The tail
(`pages/live/tail.ts`): fetch-read SSE (the proof header), batches applied at most once a pace, merged by request id (a folded repeat
changes in place), 200 rows kept, 500 held, held while scrolled, hovered, selected, focused or under an overlay behind a floating
`↑ N new · Show` pill, a 1.5 s paint-only highlight on new rows, reconnect from `Last-Event-ID`, and a new tape when the broker
restarted (the cursor carries the process's epoch). `/connect/first-request` and `WireDock` (`pages/live/wire-dock.tsx`, `topic` or
`group`, shown on `/gallery`) for R1a/R2 to mount. Both Live routes are their own chunk.

**Endpoints** (`crates/kafka-console/src/api/live.rs`): `GET /api/v1/live`, `/api/v1/live/request/{id}` (`found: null` when gone, a
200: a 404 is a console error), `/api/v1/live/clients`, `/api/v1/live/first`, `/api/v1/live/topic-wire?topic=`,
`/api/v1/live/group-wire?group=`, `GET /api/v1/tail/live` (JSON SSE, pace clamped 1 to 30 s by the server). The refusal rules are the
classic drawers' own functions (`refused_rules`, `fenced_rules`, `too_large_rules`). Catalogue CON-1250 to CON-1262.

**Shared files touched** (small): `router.tsx` (two lazy routes), `api.rs` (routes, `GET_PATHS`), `api/types.rs`, `index.css` (the
highlight keyframes), `gallery.tsx` (the WireDock section), `components/app/data-table.tsx` (two fixes every slice gets: a column
without a size now takes the rest, where TanStack had given it 150 px; `aria-rowindex` only on a virtual body, so a prepended row does
not rewrite every row's index).

**Gate.** Playwright `web/e2e/tests/live.spec.ts` (10 scenarios) with R0's: 117 passed in Firefox, WebKit and Chromium on the seeded
world with the spec's own traffic; the Live spec also green in Firefox against a broker running the demo workload. Measured: no request
after load in a snapshot; batch gaps at least the pace; no existing row replaced or rewritten over 20 s; header, strip, toolbar, tail
bar and table head boxes identical; scroll unchanged while held; no sideways scroll at 1024 to 1920. Weight: 6,177,952 B, 160
packages, 7,008 KB idle (limit 7.8 MB).

**Not supplied.** The `live-lab-rule` board's "A rehearsal is active" card (the Live view-model does not carry the active rule; the
rows carry the `lab` tag and the Sheet names the rule); the boards' two-line rows (one line a row, the second line in the tooltip);
"Pause" (pausing is Snapshot).

## R2 done (2026-11-13, branch `react-r2`): Groups

**Pages** (`web/src/pages/groups/`, each its own chunk): `/groups` (StatStrip of the counts, the rebalancing line in a region of
reserved height, the name filter `q` and the states `state` as a MultiSelect with the count of each, both in the address; a DataTable;
the empty state with its command); `/groups/$group` as a layout (h1, state and pills, Reset offsets / Delete offsets / Delete group,
the group's StatStrip, switch-group chips or a Combobox past eight, tabs as routes) with **Members** (the data chooses: classic
stable or empty, KIP-848 with target and assigned, rebalancing with the rejoin deadline counted down in the browser; who holds which
partition; the group's facts; R3's `WireDock group` as "What just happened"; a member as a Sheet, `?member=`), **Lag** (one
DataTable per topic with lag bars, the states no-commits, past-the-end and expiring, `?expiring=1` a segmented control), **Story**
(the generations timeline, the Now card with its countdown in its own slot, the causes, the flapping verdict, the calm contrast or a
quiet story; a rebalance as a Sheet `?generation=`, the flapping rule as a Sheet `?rule=1`), **Wire** (R3's `WireDock group`). The
dialogs, routed by `?dialog=`: Reset offsets (topic Select, mode RadioGroup of six, an offset or shift field, a UTC date-time picker
of our own, a GET Preview, then Reset of exactly what was previewed), Delete group, Delete offsets (a Checkbox per topic). A refusal
stays in the dialog in Kafka's name, number and words with the rule, what would pass and the call refused. Overlays return the
focus to their opener (Firefox and Safari do not focus a clicked button), even when a refresh drew the row again.

**Endpoints** (`crates/kafka-console/src/api/groups.rs`): `GET /api/v1/groups`, `/api/v1/groups/{group}?expiring=1`, `/story`,
`GET`+`POST /reset`, `GET`+`POST /delete`, `GET`+`POST /offsets/delete`. A write answers 200 with an `outcome` (`done`, `refused`,
`invalid`): a refusal is an answer, so no browser logs a failed request. The view-models are the classic ones (`model/groups.rs`,
`model/story.rs`, `lag.rs` gained `Serialize` + ts-rs). Catalogue CON-1200 to CON-1209.

**Shared files touched**: `router.tsx` (the routes), `api.rs` (`pub mod groups`, the merge of its routes, two `GET_PATHS`),
`api/types.rs` (one function), `api/queries.ts` (`useInterval` exported). React-r3 is merged into this branch (for `WireDock`).

**Gate.** `web/e2e/tests/groups.spec.ts` (14 scenarios, each browser making its own groups through Kafka: one empty with commits,
one with a member that stays) with R0's and R3's specs: **53/53 in Firefox, WebKit and Chromium** on the seeded world, and the
groups spec **14/14 in all three** against a broker running the demo workload. WebKit and Chromium log any non-2xx fetch as a console
error; that is why a write's refusal is a 200 outcome and a deleted group's queries are dropped only after its page is gone. Rust:
`cargo test -p kafka-console -p doze-kafka` 898 passed (`tests/console_groups_api.rs` 6), fmt, clippy `-D warnings` default and
lean, lint-size 0 over hard, the catalogue checker green. Weight: 6,261,104 B, 160 packages, 7,200 KB idle (limit 7.8 MB).
Screenshots: scratchpad `r2/shots/` (demo broker) and `r2/fx/` (the fixtures' states: rebalancing, KIP-848, past the end, the
refusals, the rebalance Sheet), Firefox and WebKit at 1440 and 1024.

**Not supplied.** The boards' "Reached from" lines; the group wire's filters are R3's dock state, not the address; below 1280 the
members table leaves Host and Session to the member's Sheet; the planted-secret test reads the list and not one group's endpoints (its
live world has no group: `GROUP_GET_PATHS` are read over the fixtures instead).
## R1a done (2026-11-13, branch `react-r1a`): Topics, the read side

**API** (`crates/kafka-console/src/api/topics.rs`, `topics/{log,tail,types}.rs`): `GET /api/v1/topics`, `/topics/{t}` (the frame),
`/messages`, `/records/{p}/{o}`, `/consumers`, `/log?partition=`, `/producers?partition=`, `/transactions/{id}`,
`POST /topics/{t}/log/compact` (the Lab's round), `GET /api/v1/tail/messages` (SSE over fetch: `ready`, then JSON `rows` events at most
one per pace, `Last-Event-ID` resume, one stream permit each). A lookup that finds nothing answers `{missing}` with 200 (a 404 is logged
as an error by Chromium and Safari). Catalogue CON-1100 to CON-1108.

**Pages** (`web/src/pages/topics/`, one lazy chunk): the list (filter, Show internal Switch, none/filtered states), the frame
(`frame.tsx`: head, actions opening `?dialog=` for R1b, stats, reserved transaction region, five tabs as routes, `TopicOverlays` mount
point), Messages (toolbar of MultiSelect, the one find box, the From time Popover with quick choices, the isolation ToggleGroup; a filter
line of removable chips; DataTable with stable ids; Older; the routed record Sheet; Snapshot | Live tail with Realtime/Relaxed/Slow,
the held pill and the connection indicator), Partitions, Consumers, Log (ruler, planner or retention rules, last round or pass,
deletions, durability, segments that open in place, other logs, Compact now Dialog), Producers (rows that open to their batches, the tail
ruler, aborted transactions), a transaction and who it holds back. Shared files touched: `router.tsx` (the routes), `data-table.tsx`
(a column without a width now takes the rest: TanStack's default 150 px size had made every "fill" column 150 px; a row height prop;
a row class for the tail's highlight), `index.css` (the new-row keyframes).

**Gate**: Firefox, WebKit, Chromium 45/45 each on the seeded world (R0's and R1a's specs), and R1a's specs against a broker with
traffic. Weight: doze-kafka 6,244,400 B darwin (R0 6,161,072), 160 packages.

## R1b done (2026-11-14, branch `react-r1b`): Topics, the write side and Config

**Pages and overlays.** `/topics/new` (`web/src/pages/topics/create.tsx`, its own chunk with zod): Create topic as a page of three
steps (shadcn Tabs as a stepper, `?step=` in the address, Back returns to the step with what was typed, `?name=` to start from; the
list's `?dialog=create&name=` lands here), react-hook-form + a zod schema for what can be said at once, then the server's checks as
you type: the form view (taken, colliding, the limits, the call, the terminal command) and, once nothing is wrong, the same
CreateTopics with `validate_only`, its answer under the name ("Kafka would take this" or Kafka's code and sentence). Check only,
Create, the created state (banner, next steps with the kcat commands, Produce a record), the refused state (Kafka's words, Open the
existing topic, Choose another name, the other refusals, the answer). The write overlays (`pages/topics/writes/`) fill R1a's
`TopicOverlays` (`?dialog=produce|add-partitions|delete-records|delete`, `&partition=` for Delete records): Produce (partition
Select with auto, key, value with a JSON hint, header rows added and removed, acks ToggleGroup, the teaching folded, the call the
server draws for what is typed; in place: the partition and offset the log chose, whether read_committed sees it, the answer; or
the refusal), Add partitions (the field starts at one more, the unit says what it adds, the button counts it), Delete records
(partition Select, the offset, the server's preview of what goes and which groups are below it), Delete topic (type the name, who
uses it; after, the list and a toast). Buttons stay in reach (a sticky footer). The Config tab (`pages/topics/config.tsx`, R1a's
route, `?q=` and `?show=changed|compaction|segments` in the address, Back restores them): changed configs first, then the groups
(replication folded), each row with value and gloss, source badge, Kafka's first sentence, when a change counts, "Where from" and
"Kafka's text" as Collapsibles (the scroll and the address stay), Edit/Reset in a DropdownMenu on a value the topic sets; the edit
Dialog (`?edit=`: one value, a list with append/subtract/set, the two lags together; Check and Save; checked, saved with before and
after and when the broker reads it, or refused with what would pass and Edit again) and Reset (`?reset=`). After any write every
page query is marked stale (the form previews excepted), so the page under the overlay shows the new state without a reload.

**Endpoints** (`crates/kafka-console/src/api/topic_writes.rs`, `topic_admin.rs`, `config.rs`, `config_edit.rs`): `GET`/`POST
/api/v1/create-topic`, `/api/v1/topics/{topic}/produce`, `/add-partitions`, `/delete-records`, `/delete`, `GET
/api/v1/topics/{topic}/config`, `GET`/`POST .../config/edit`, `.../config/reset`. POSTs are form-encoded, as the classic forms, through
the same `Source` calls; every answer to a write is a 200 with `outcome` (`done`, `refused` with Kafka's name and the broker's
sentence and `detail`, `invalid`), as R2; a missing topic or config is `{missing}`, as R1a. The dialogs' words that are made from
data (teaching, button labels, the Delete records preview, who uses a topic) moved from the page layer to `write_words.rs`, so R5 can
remove the maud pages. Catalogue CON-1150 to CON-1160.

**Shared files touched** (small): `router.tsx` (`/topics/new`, `configSearch` on R1a's Config route), `api.rs` (four merges,
`GET_PATHS`), `api/types.rs` (two functions), `lib/search.ts` (three parsers), `api/client.ts` (`ApiFailure.detail`), R1a's
`pages/topics/overlays.tsx` (filled) and `config.tsx` (replaced), `e2e/tests/topic-tabs.spec.ts` (the Delete records dialog now
opens, so the test closes it), `components/ui/collapsible.tsx` (shadcn CLI), model derives (`model/write.rs`, `model/config.rs`,
`Cleanup`).

**Gate.** Playwright `topic-writes.spec.ts` (12 scenarios) and `topic-config.spec.ts` (7) with everything on main; numbers in the
report of the slice. Rust: `tests/live_api_writes.rs`, `live_api_config.rs` over a real broker.

**Not supplied / differences.** The Config rows are a fixed-column list, not the DataTable (rows wrap and open panels under them);
the Produced state is in the dialog, not the board's page with the topic's table (the Messages page under the dialog refetches);
the created state replaces the form on `/topics/new` rather than a page of its own address; the board's cleaner verdict ("61 % of
the log is dirty") the broker cannot supply (debt J121, as the classic page).
## R4 done (2026-11-13, branch `react-r4`): Settings, Access, the Lab

**API** (`crates/kafka-console/src/api/{settings,access,lab}.rs`, CON-1300 to CON-1331): the view-models of `model/{settings,access,lab}.rs`
serialised (memory and thread counts never; `Refusal` and `ConfigSource`, shared with other pages, through `serialize_with`, so their files
are untouched); every write through the classic console's own path, answering a tagged outcome (`saved | refused | checked | invalid |
switched`, `created | invalid | refused`, `made | invalid | checked`, ...) in Kafka's or the engine's words. The Lab's rule form now takes
several APIs (the engine's failable list), topics, partitions and a group, and sends its codes, shares and lifetimes (`lab/form.rs`,
`lab/act.rs`). Contract tests: `crates/kafka-console/tests/console_r4.rs` (fixtures, and a real broker with the fault table for the Lab).

**Pages** (`web/src/pages/{settings,access,lab}`, each its own lazily loaded chunk): Settings with its Edit dialog (`?edit=&scope=`);
Access with Create ACL (`?acl=new`) and Delete ACL (`?delete=`) and ACL filters in the address; the Lab with its states by data, the New
rehearsal dialog (`?new=slow|fail|drop|expire|rebalance|compact`), the rule's sheet (`?rule=`) and Remove (`?remove=`). For the other
slices: `LabRuleBadge` (`pages/lab/badge.tsx`, R3's Live drawer) and `CompactNowDialog` (`pages/lab/compact-now.tsx`, R1a's Log tab).

**What the browser gate and the screenshots found**: the router wrote a numeric string id as JSON (`?rule=%221%22`), so rule numbers are
numbers; the focus could not go back to the opener while the page was still `inert`, and a table that re-made its columns on each render
drew its buttons again (the opener was gone): columns are made once with stable callbacks and the focus is restored after the overlay's
clean-up; a centred dialog moved as its content grew (now anchored at the top); WebKit reported a ResizeObserver loop when the shared
MultiSelect's trigger shrank under its open popup as the Clear button appeared (the button is now always there, hidden while empty: a
one-line change to `components/app/multi-select.tsx`); at 1024 the three switch cards of Access squeezed the listener address into a
column of characters (they stack below 1280) and the Lab's rule table ran past its box (Share folded into Applies to).

**The gate, last run** (Playwright 1.65.0, `workers: 1`, release binary): 43 scenarios per browser (R0's 29 and R4's 14), **129/129**
against the seeded world and **129/129** with `E2E_URL` against a broker with the demo workload (Firefox, WebKit, Chromium each 43/43);
after a last text-only change the Settings spec again 15/15 in the three browsers. Rust: `cargo test -p kafka-console -p doze-kafka`
893 passed; fmt, clippy `-D warnings` (default and `--no-default-features`), lint-size (0 over hard), the console catalogue green.
Weight (darwin): 6,311,072 B, 160 packages (limit 161), 7.2 MB idle: ok. Screenshots: scratchpad `r4shots/` (Settings, the Edit dialog,
Access, Create ACL, the Lab, a rule's sheet, New rehearsal on Slow and Expire; Firefox and WebKit at 1440 and 1024).

**Left for later.** The weight's package count is a build-time figure the binary cannot read, so Settings shows the binary's size and
the open partitions but not the packages. The client and group fields of a rule choose from the apps and groups there are; a glob
(`orders-*`) is taken by the engine but the form has no free-text entry for it yet.

## R5 done (2026-11-13, branch `react-r5`): the classic console removed

Owner: "We can get rid of the classic mode." The server-rendered console is gone from the crate and the app.

**Removed.** The maud page layer (`pages/`, `ui/`, `handlers/`, `routes`, `region`/`regions`, `trail`, `actions`, the `boards*` renders,
`examples/boards.rs`), the classic files (`assets/`: htmx, idiomorph, Alpine, `console.js`, `components.js`, `multiselect.js`, `live.js`, the
kit CSS, a second copy of the fonts), `design/` (the board sources and signatures), the `/classic/` mount and the router's fallback into it,
the classic HTML tail (`stream.rs` keeps only the pace rules the API's two tails use), and every word function only a page called (found by
compiling the crate with private modules; their unit tests went with them). Dependencies: `maud` (and `maud_macros`,
`proc-macro2-diagnostics`); `tower` is no longer a direct dependency. Tools: `board-signature`, `board-diff`, `boards-refresh`,
`boards-snapshot`, `a11y-lint`, `tools/{lib,test,dom,ux,pixels}`, `assets-compress.ts` (its `pack` moved into `web/scripts/build.ts`), and
their CI steps. In the app: the sidebar's Classic console item, the `classic` field of `/api/v1/shell`, `PlannedPage` (`pages/planned.tsx`
is `pages/not-found.tsx`). `git diff --stat main`: 626 files, +1,563, -69,231.

**Kept and moved.** The view-models, `gather/`, `backend/`, the recorder, the words the API uses, the fixtures. From the page layer: the
request rules and copy commands of Live (`live_words.rs`, the API's `RequestKind`, `RequestLimits`, `RequestRule` unchanged), `Chrome`
(into `model`), the file answer (`files.rs`: gzip negotiation, ETag, 304), the guard's refusal page (now a `format!` with an `escape`).

**Tests.** The classic page tests are gone (boards, components, regions, routes, overlays, ux, and the live_* tests that read HTML pages:
112 tests). Ported to the API, not dropped: the guard and same-origin proof over every API write (`tests/guard.rs`), the headers, 304 and
gzip over the bundle's files (`tests/headers.rs`), the limits and the stream cap and shutdown over `/api/v1/tail/*` (`limits.rs`,
`stream.rs`; the API's 503 now carries `Retry-After`), Home and its dials (`home_live.rs`, `live.rs`, `txns.rs`), a GET never writes over
every API read (`live_write_acl.rs`), the planted-secret gates (`redaction.rs`, `live_access_secrets.rs`), the process tests of
`doze-kafka` (Home, the proof, the tail, the prefix, the unprotected flag through `/api/v1/shell`). `/classic/...` is now asserted to be the
app's Not here (Rust and Playwright). New: `web::tests::no_source_of_the_page_layer_injects_html`.

**CSP tightened**: `img-src 'self'` (no `data:`) and `form-action 'none'` (was `'self'`): both were for the maud pages only.

**Catalogues.** `security.md`: SEC-243 to SEC-249 rewritten for the React console. `console.md`: 51 classic-only rules marked "retired in
R5" with the reason, 25 data rules whose only test was a page test marked "Untested since R5" (the behaviour holds in the API, no API test
names it yet: CON-113, 114, 201, 202, 204, 205, 208 to 211, 530 to 537, 539, 583, 606, 612, 615 to 617), CON-1010 rewritten; both checkers
green.

**Found.** The React Home reads `/api/v1/home`, which runs the advertised-listener check when the cached result is older than 30 s: a Home
left open with a refresh interval dials at most once in 30 s, and against an address that does not answer such a read waits up to the
check's deadline (1.5 s). The classic page's fragment never dialled. Not changed here (it needs a Home page change; the polish pass owns
`web/src/pages`). The classic test `crates/kafka-console/tests/live_tail.rs` flaked once under load (a timeout); it went with the classic
tail.

**Gate.** fmt; clippy `-D warnings` default, lean (`-p doze-kafka -p kafka-console --no-default-features`) and `--features tls`;
`cargo test -p kafka-console -p doze-kafka` 521 passed; lint-size 0 over hard (the generated `web-dist/manifest.rs` now says
`@generated`, as the codegen tables, and is skipped); both catalogue checkers green; `bun run typecheck`, `lint`, `bun test` (31), `build`;
Playwright with main's theme spec merged: **Firefox 104/104, WebKit 104/104, Chromium 104/104** on the seeded world. Weight (darwin):
**5,945,008 B** (main 6,593,728 B), **155 packages** (limit 161, not moved), 6,800 KB idle.
