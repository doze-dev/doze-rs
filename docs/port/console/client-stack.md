# The console's client stack: htmx 2 + idiomorph + Alpine (CSP build)

**Decided 2026-11-12 (UX foundation, CON-650 to CON-699). Supersedes D4a of `phase9.md`** ("our own script, no third-party JavaScript"), which was right about bytes and CSP and wrong about what the console is: the owner opened the built pages and every interaction was a full-document GET (a scroll jump, a flash, a native `<select>`, a drawer that nudged 28 px). The Go console used htmx and Alpine; the Rust console now does too, with the two constraints D4a was right about kept: **`default-src 'self'`, no `unsafe-eval`, no inline script**, and every page working with no script.

## What is adopted, and why each piece

| Piece | Version (pinned in `src/assets.rs`) | Does | Licence | Bytes (minified, raw) |
|---|---|---|---|---|
| htmx | 2.0.11 `dist/htmx.min.js` | boosted links and forms (a navigation swaps `main` only), `hx-push-url`, `hx-swap` with `show:` and `transition:true`, history restore, response headers (`HX-Retarget`, `HX-Replace-Url`), out-of-band swaps for the rail and the overlay root | 0BSD | 52,182 |
| idiomorph (htmx extension build) | 0.8.0 `dist/idiomorph-ext.min.js` | `morph:outerHTML` swaps: the DOM is edited into the new one, so focus, the caret, typed text and scroll survive; also used by the stream's fragment refresh | 0BSD | 11,037 |
| Alpine.js, CSP build | `@alpinejs/csp` 3.17.5 | the components (`assets/components.js`): `Alpine.data(...)` only, so no expression is ever compiled from a string | MIT | 72,242 |
| Alpine anchor | `@alpinejs/anchor` 3.17.5 (Floating UI inside) | positions a popup against its trigger and flips it near the window's edge | MIT | 15,664 |
| **Vendored total** | | | | **151,125** |
| `console.js` + `components.js` (ours) | | product logic (Live, the palette, copy, overlays) and the six components | | ~40,000 |

Licence texts sit beside the files (`htmx.LICENSE.txt`, `idiomorph.LICENSE.txt`, `alpine.LICENSE.txt`); `src/assets.rs` names each file, its npm package and version and its sha256; a test fails if a file of `assets/` is not in the manifest or has no licence.

**Weight.** The brief's budget was "vendored JS under 150 KB"; the stack is **151,125 B** (1 KB over, after leaving out the focus plugin, 26 KB, and the preload extension, 4 KB: see below). The whole console asset set is 330 KB with the fonts (it was 140 KB). The binary step is in `docs/port/weight.md`; the gate is 9 MB and the owner's ceiling is 10 MB, so the stack is not the constraint: bugs were.

## What was tried and not kept (evidence)

- **`@alpinejs/focus` (26 KB, `x-trap`).** Not shipped. The overlay's page is `inert` (the browser's own focus containment) and a 10-line Tab wrap keeps focus in the panel; the DOM tests (`tools/dom/overlay.test.ts`, "focus") and `drive.ts` (`drawer-closes-three-ways-and-keeps-the-page`) assert both. 26 KB for behaviour that was already tested was the one place the budget decided.
- **`htmx-ext-preload` 2.1.2 (4 KB).** Tried, dropped. It throws `TypeError: Cannot read properties of null (reading 'forEach')` on any form that holds a hidden input (it reads `element.labels`, which is `null` for hidden inputs) the moment `preload` is inherited from `body`, and it has no focus trigger. Replaced by 20 lines in `console.js` (section 8): a boosted link the pointer rests on for 65 ms, or that takes focus, is fetched ahead with the headers a navigation sends (`HX-Request`, `HX-Boosted`, `HX-Target: main`) plus `HX-Preloaded`; the server lets the browser keep **only that** answer for 5 s (`Cache-Control: private, max-age=5`, `src/region.rs`), so the click is answered from the cache (`nav-preload` in `drive.ts` asserts the cached response).
- **`hx-ext="alpine-morph"` / Alpine's morph plugin.** Not needed: idiomorph is the one morph.
- **htmx `historyCacheSize`.** Set to 0: the snapshot cache writes whole pages (record values!) to `localStorage` and restores stale Live pages. Back fetches the page again (`HX-History-Restore-Request`, answered with the whole document) and the scroll is restored from `sessionStorage` per address.

## CSP: what was checked

The header is unchanged: `default-src 'self'; style-src 'self' 'unsafe-inline'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'`.

- htmx is configured by `<meta name="htmx-config">` (`ui::client::HTMX_CONFIG`, a constant with each line explained): `allowEval:false` (no `hx-on`, no `js:` values), `allowScriptTags:false`, `selfRequestsOnly:true`, `includeIndicatorStyles:false` (it would add a `<style>`), `historyCacheSize:0`.
- Alpine is the **CSP build**: it parses directive expressions itself and evaluates no string. Components are `Alpine.data` objects; markup binds only method names and property paths (`x-on:keydown="key"`, `x-show="open"`, `:aria-expanded="open"`).
- `drive.ts` fails any scenario that logs a console error, warning, exception or CSP violation: every scenario in the suite ran with the real header and none logged one.
- Static tests: `assets::tests::no_script_of_ours_evaluates_a_string` (no `eval(`, `new Function`, string timers, `document.write` in our two scripts), `region.rs` tests (no inline `<script>`, the meta config carries `allowEval:false`).
- The vendored files themselves contain a `new Function` and an `eval(` (htmx, for `hx-on` and `js:` values, **guarded by `allowEval`**); they are never reached because the markup never has such an attribute and `allowEval` is false.

## How it fits together

- **Server contract.** Every route returns a whole document. When the request carries `HX-Request` (and is not a history restore), `ui::shell::document_with` returns the **region** instead: `<title>`, `<main id="main" data-stream=...>` (crumbs, the live banner, the page) and, out of band, the rail (`#rail`) and the overlay root (`#ovl`): the shell around `main` is not sent. `src/region.rs` is a layer plus a task-local; `tests/region.rs` pins it. `Vary: HX-Request, HX-History-Restore-Request, HX-Target` is on every answer. Overlay routes answer the overlay element alone to `HX-Target: ovl` (or `X-Fragment`), unchanged.
- **Navigation.** `<body hx-boost hx-target="#main" hx-swap="outerHTML transition:true show:window:top" hx-ext="morph">`: every link and form of the console is a client-side navigation that cross-fades `main` in a same-document View Transition (180 ms in, 120 ms out, a 5 px rise), starts at the top, pushes the history, updates the title, the breadcrumb (inside `main`) and the rail's active item (out of band), and re-opens the stream for the new page (`data-stream` is on `main`). Back restores the scroll. A progress hint (a 2 px bar) appears only if a request takes over 250 ms.
- **In place.** A control that changes only the data it owns (a filter, a chip, a sort, a pager) is `ui::client::in_place_link` / `in_place_form` / `components::seg(.., Some(id))`: `hx-get`, `hx-target="#id"`, `hx-select="#id"` (the part is picked out of the page's region), `hx-swap="morph:outerHTML show:none"`, `hx-push-url`. Scroll, focus, typed text and the toolbar stay; only what changed fades (<= 120 ms, opacity); an unchanged answer changes nothing.
- **Overlays.** Opened by `htmx.ajax('GET', url, {target: '#ovl'})` (no history of htmx's: the overlay's entry is ours, so Back closes it in place; a capture-phase `popstate` listener takes the entries that are ours before htmx can restore a page for them). A form in an overlay posts through `htmx.ajax`; an overlay that comes back (a refusal) replaces this one; a page that comes back is told by the layer to go into `main` (`HX-Retarget`, `HX-Reswap`) and to replace the overlay's address with its own (`HX-Replace-Url: <path>`; htmx 2.0 has no `true` for that header, found by a test), and the out-of-band `#ovl` clears the overlay.
- **Components** (`docs/port/console/components.md`): maud functions in `src/ui/components/` that render complete no-script markup (a real `<select>`, links in a list, a `title`) and carry `x-data`; Alpine enhances them.

## What stays ours, and why

The copy buttons (with "Copied"), the Live tape (pause with a held-back count, the tape held while a drawer is open, new rows marked), the stream (`EventSource`, the ETag and `304` handling, the banner), `j`/`k`, the palette's keys and data, the scroll memory, the progress hint, the focus after a swap. These are the product; htmx and Alpine have no opinion on them. The DOM tests in `tools/dom` run **both** the libraries and our two scripts in happy-dom with a scripted `XMLHttpRequest`, `fetch` and `EventSource` (66 tests, rewritten where the interception moved from our code to htmx's).

## Numbers measured

See `docs/port/console/ux-tests.md` for how to run `tools/ux/drive.ts`, and the final report of the UX foundation pass for the per-scenario numbers (drawer rect 0/800/1280, slide start 480 px, 240 ms, layout shift 0, scrollY unchanged by a filter, one preload, cached click).
