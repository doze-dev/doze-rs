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
