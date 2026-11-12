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
escape above. The gate's results per browser are below (filled in by the last run).
