# D4a client-layer spike: htmx vs Datastar vs own script

Run 2026-11-12. Everything is in `/private/tmp/claude-501/-Users-srini-Code-doze-dev/014d8981-fa59-41b3-a494-fc4a4035c0bb/scratchpad/spike/`.
No repo was edited. Chrome 152 headless, macOS, one browser only (no Safari or Firefox, no screen reader).

## What was built

One axum 0.8 + maud 0.27 server, one page (Home-like: live sentence, request tape of the newest 20 rows arriving every 100 to 300 ms from one
shared tokio task over a `watch`, filter box, Pause with "N held back", produce form, delete-topic `<dialog>`, scrollable table), with the page
written once (`app/src/view.rs`) and the elements that carry client behaviour supplied per variant (`v_htmx.rs`, `v_own.rs`, `v_datastar.rs`; `v_base.rs`
is the no-client-layer baseline). A cargo feature picks the variant: `base`, `htmx`, `own`, `datastar` (hand-written SSE frames), `datastar-sdk`
(the `datastar` 0.4.1 crate). `./build.sh` builds all five into `bin/`.

Two layouts are tested, because the interesting failures only show when a stateful element sits inside what gets swapped:

- **stable** (default): filter, Pause and the scroller sit outside the swapped fragment (`#tape` table, `#sentence` span). This is how a careful htmx page is built.
- **hostile** (`/?layout=hostile`): the whole card (filter input, Pause, scroller, table) is the swapped fragment `#tape-card`. For htmx this is run twice: with the idiomorph extension (`morph:outerHTML`) and as a plain `outerHTML` swap (`&swap=plain`).

The filter is server-side (`?client=` on the fragment, or the `client` signal for Datastar). Pause is client-side for htmx and own (the stream keeps
ticking, the page counts rows held back) and server-side for Datastar (a patch stream cannot be held back in the browser, so Pause re-opens the stream with
`paused` and `pseq` signals and the server sends only the "N held back" patch). All CSP runs use the console's policy
`default-src 'self'; style-src 'self' 'unsafe-inline'; frame-ancestors 'none'` unless stated.

## Results

### Weight

| | htmx 2.0.11 (A) | own script (C) | Datastar 1.0.5 (B) |
|---|---|---|---|
| Library bytes raw / gzip / brotli | 52,182 / 16,885 / 15,259 | none | 35,673 / 14,131 / 12,821 |
| Our script, raw / gzip / brotli (as written) | 3,021 / 1,389 / 1,155 | 4,191 / 1,829 / 1,516 | 823 / 463 / 359 |
| Our script minified (`bun build --minify`), raw / gzip | 1,837 / 946 | 2,523 / 1,276 | 423 / 287 |
| **Client layer total, raw / gzip / brotli** | **55,203 / 18,274 / 16,414** | **4,191 / 1,829 / 1,516** | **36,496 / 14,594 / 13,180** |
| Extra if the page needs morph (hostile layout) | + idiomorph ext 11,037 / 3,945 / 3,517 | in the 4,191 | in the library |
| Non-blank, non-comment client lines we own | 52 | 77 | 11 |
| Server lines the variant needs (non-blank) | 55 + `ticks.rs` 68 + ETag helper about 20 = about 143 | 44 + 68 + 20 = about 132 | 147 (of which about 14 are SDK `cfg` and about 10 hand frames) + 2 header lines |
| Release binary (thin LTO, stripped), baseline `base` = 908,080 B | 1,007,712 (**+99,632**; +88,595 without idiomorph) | 941,664 (**+33,584**) | hand frames 991,936 (**+83,856**); with the SDK crate 1,043,248 (**+135,168**) |
| Packages, `cargo tree -e normal` unique (baseline 43) | 43 (+0) | 43 (+0) | hand 45 (+2: serde_json, zmij); SDK 52 (+9: also datastar, serde, serde_derive, serde_path_to_error, serde_urlencoded, form_urlencoded, ryu) |

Notes: the binary deltas include the SSE and fragment server code that htmx and own share (about 29 KB), so compare the variants with each other, not with 0.
The SDK adds 51 KB and 7 packages over hand-written frames because it turns on axum's `json` and `query` features; the frames it writes are byte-identical
to the hand-written ones (checked). htmx 4.0.1 (npm tag `next`, not `latest`) is 37,448 / 13,403 / 12,000 with morph in core and fetch instead of XHR; not tested.

### Behaviour tests

PASS in the layout the variant is designed for; the hostile-layout column is where the libraries differ. Scripts: browser tests `tests/browser.ts`
(`bun tests/browser.ts <variant> [--csp=strict|eval|nonce] [--only=T1,...]`), server tests `tests/server.ts`, shared helpers `tests/lib.ts`. Logs and JSON in `results/`.
Datastar rows are with `CSP=nonce` (it does not run at all under the console's policy, see the CSP section).

| # | Test | htmx | own | Datastar |
|---|---|---|---|---|
| 1 | filter keeps focus, caret, typed text across 50 updates; then caret mid-text across 20 more; then 34 characters typed at 20 ms while updating | PASS stable. Hostile: **FAIL** with idiomorph in 5 of 5 runs, with plain swap in 3 of 5 (a keystroke typed while the response is in flight is overwritten by the server-rendered `value=`). With `Idiomorph.defaults.ignoreActiveValue = true` (one line in the glue) hostile PASSES 4 of 4 | PASS stable; hostile PASS 5 of 5 (+2 more runs) | PASS stable; hostile PASS 5 of 5 (+2 more runs) |
| 2 | scroll position kept (scrollTop 120 across 30 updates) | PASS stable and hostile+morph; hostile+plain swap **FAIL** (scrollTop 0), as expected | PASS both | PASS both |
| 3 | Pause holds rows, counts them, resume catches up (button label, `aria-pressed`) | PASS | PASS | PASS (server-side pause; needed `aria-pressed` written as `$paused ? 'true' : 'false'`, a boolean true gives `""`) |
| 4 | stream reconnects after a server restart, no page reload: SIGKILL, then SIGTERM | PASS, 0.5 s after restart each | PASS, 0.5 s | PASS, 0.5 s with `retry:'always', retryMaxCount: Infinity`. See traps |
| 5 | form post with JS (fragment + flash, no reload, `<b>` escaped, row appears) and without JS (page renders 20 rows, post is a full reload) | PASS / PASS | PASS / PASS | PASS / PASS |
| 6 | dialog: modal, focus moves in, Esc closes and focus returns to the opener, Cancel, confirm posts and closes; without JS the route renders as a page and confirm posts | PASS / PASS | PASS / PASS | PASS / PASS |
| 7 | graceful shutdown with 10 open streams | PASS, exit 3 ms after SIGTERM | PASS, 2 ms | PASS, 2 ms (hand and SDK) |
| 8 | unchanged fragment on a frozen tape, 10 forced ticks | PASS only after a fix, see traps: 20 requests, all 304, 0 body bytes, 0 content mutations (60 `class` attribute mutations from `htmx-request`) | PASS: 20 requests, all 304, 0 bytes, 0 mutations | PASS: nothing sent (the server remembers what each stream last sent), 0 mutations |
| 9 | 100 open streams, ticks every 300 ms: server RSS delta (idle server 3.3 MB), CPU | +8,064 KB; 3.3% of one core | +7,888 KB; 3.3% | hand +4,944 KB, 1.5%; SDK +5,264 KB, 1.45% |
| 10 | overlapping responses: a slow stale answer must not overwrite the newest filter | PASS, because the glue calls `htmx:abort` first (the htmx default for an element with a request in flight is `drop`, which would drop the newer one) | PASS (`AbortController` per fragment) | PASS (a new `@get` to the same URL cancels the old one) |

Test 9 detail: for htmx and own the 100 simulated clients act like the page script (on each tick they fetch `/frag/sentence` and `/frag/tape` with `If-None-Match`),
14,800 fragment requests in the 20 s window. Datastar needs none of those requests. Payload per update is about the same (2.6 KB, roughly 860 to 895 KB/s over 100 clients);
htmx and own add the HTTP request and response headers of two extra requests per update (estimated at about 1 KB, not measured). CPU resolution of `ps` is 10 ms over a 20 s window.
Chrome's peak RSS (sum over all its processes, shared pages counted repeatedly) was 1.2 to 1.3 GB per run, under the 2 GB abort line; 0 Chrome processes after every run.

### Traps found (each cost real debugging, all apply to the plan's D4 as written)

1. **The ETag plan does not work as written.** The browser validates `Cache-Control: no-cache` + `ETag` fragments itself and hands the page a 200 with the cached body, never a 304
   (`tests/probe304.ts`: plain `fetch`/XHR give 200, a manually set `If-None-Match` gives a real 304). So with plain htmx every tick still swaps (40 content mutations per 10 ticks
   in the first run). The script must keep the ETag and send `If-None-Match` itself, and for htmx also cancel the swap on 304 (`htmx:beforeSwap`, `shouldSwap = false`).
2. **`htmx.ajax()` without `source` uses `body` as the requesting element**, so `htmx:configRequest`/`beforeSwap` see `elt = body` and `htmx:abort` on the target does nothing. Pass `{source: el, target: el}`.
3. **A stream per page leaks through the back/forward cache.** Pages parked in bfcache kept their `EventSource` open; after 5 navigations the server saw 6 streams (Chrome's 6-per-origin HTTP/1.1 limit) and the 7th page never went live.
   htmx and own need `pagehide` to close the stream, `pageshow` (persisted) to reopen, and `visibilitychange` to close in hidden tabs (the same limit is shared by tabs, and the console is HTTP/1.1 only). Datastar does this itself (GET streams close when the tab is hidden, `openWhenHidden: false`). Fixed in both glues, verified by `tests/probe_leak.ts` and `tests/probe_vis.ts`.
4. **Datastar retries default to 10 attempts** (`retryMaxCount: 10`, backoff doubling to 30 s), after which it gives up with `retries-failed`; the console left open while the broker is down would die. Use `{retry: 'always', retryMaxCount: Infinity}`. After a retry Datastar does not fire `started` again, so the banner is hidden on the first `datastar-patch-*` event instead. With the default `retry: 'auto'` a clean stream end (graceful shutdown) still reconnected in 1.4 s.
5. **Datastar `<dialog>`**: the server patches the body in, but nothing opens it. 4 lines of glue (a MutationObserver calling `showModal()`, and clearing the dialog on `close` so the next identical patch is a change). Same for banner text.
6. **Inputs inside a swapped region** (htmx): the server-rendered `value=` is stale by the time it arrives. See test 1.

## CSP, evaluated attributes, accessibility, motion

| | htmx 2.0.11 | own | Datastar 1.0.5 |
|---|---|---|---|
| `default-src 'self'` only (the console's policy) | **works**, all tests above ran under it | **works** | **does not run**: `GenerateExpression` error, page never goes live (`results/datastar-strict.json`) |
| What it needs | `<meta name=htmx-config content='{"allowEval":false,"allowScriptTags":false,"includeIndicatorStyles":false}'>`. `allowEval:false` disables event filters, `hx-on:*`, `hx-vals`/`hx-headers` with `js:`; none are used. Indicator styles off (no injected `<style>`), script tags in responses off | nothing | `script-src 'self' 'unsafe-eval'` (verified, works), or **CSP mode**: `data-nonce="<n>"` on `<html>` plus `script-src 'self' 'nonce-<n>'` with a fresh nonce per full page (verified, works). Datastar compiles each `data-*` expression with `Function()` (eval mode) or an inline `<script nonce>` per expression (nonce mode) |
| Consequence | attributes can only make requests, never run code | none | attributes ARE code. HTML injection of one `data-on:*` attribute runs JavaScript, nonce or not (docs: "CSP mode does not make Datastar expressions safe to use with untrusted content"). Kafka keys, values, headers, topic and client names are hostile input shown on these pages. User text inside a `data-*` value must be JSON-escaped (done in the spike for the `client` signal) and containers of hostile text wrapped in `data-ignore`. The nonce also varies per response, which the board-diff gate must normalise |
| htmx-style injection | injected `hx-post`/`hx-trigger=load` can still fire a same-origin request; `hx-disable` on containers of hostile text, plus the origin proof on POST | n/a | `data-ignore` on the same containers |

- **Out-of-order and overlapping responses.** htmx: per-element request queue, default `hx-sync` strategy is `drop` for an element with a request in flight (docs), no ordering across elements; solve with `htmx:abort`/`hx-sync="...:replace"`; verified (test 10). Datastar: events on one stream arrive in order; a new request to the same method and URL cancels the old one; verified. Own: one `AbortController` per fragment; verified.
- **`prefers-reduced-motion`.** None of the three reads it. htmx adds `htmx-swapping`/`htmx-settling` classes with a 20 ms settle delay and only animates if the page's CSS says so; `hx-swap="... transition:true"` and Datastar's `useViewTransition` (default false) use the View Transitions API, which is not reduced-motion aware by itself. The spike's own row-flash animation is inside `@media (prefers-reduced-motion: no-preference)` and that is the whole story for all three.
- **Accessibility.** Focus in the dialog is native: `showModal()` moved focus into the dialog (first control, Cancel), Esc closed it and focus returned to the opener (verified in all three). None of the libraries manages dialog focus. Keep `aria-live` on an element the swap does not replace: the spike puts it on `<p id=live>` around the swapped `<span id=sentence>`; a plain htmx `outerHTML` swap of the live element itself can drop the announcement, idiomorph and Datastar morph keep the element. `#held` and `#flash` carry `role=status`/`aria-live=polite`. Updating a sentence every 100 to 300 ms would flood a screen reader; the console must throttle the live sentence (not tested with a screen reader).
- **WASM is not a candidate.** A Rust-to-wasm client (Leptos, Dioxus, Yew) needs a wasm toolchain and a build step in a repo that is otherwise plain cargo, ships 0.3 MB or more of wasm against a size gate that is already strained (this page's whole client layer is 4 to 55 KB), and the board-diff gate compares server-rendered HTML, which a client-rendered page does not provide without also doing SSR and hydration.

## Licences (vendored JS is not a cargo dependency, so deny.toml does not see it)

| File | Version | Licence | Beside it |
|---|---|---|---|
| htmx.min.js | 2.0.11 (npm `latest`; published 2026-09-22) | 0BSD (BSD Zero Clause) | `vendor/htmx.LICENSE`; 0BSD asks for nothing, but keep it |
| idiomorph-ext.min.js | 0.8.0 | 0BSD (npm metadata; not independently read) | none vendored; fetch before use |
| datastar.js | 1.0.5 (GitHub release v1.0.5, 2026-11-09, `prerelease: false`; the npm package `@starfederation/datastar` is stale at 1.0.0-beta.11, so take the file from the GitHub release) | MIT | `vendor/datastar.LICENSE`. MIT requires the notice with copies and the minified file carries only `// Datastar v1.0.5`, so ship the licence text. "Datastar Pro" is a separate commercial licence; nothing used here is Pro |
| `datastar` crate | 0.4.1 (2026-09-29), MSRV 1.89 | MIT | cargo-deny sees it; 0.x semver |
| own script | n/a | ours | n/a |

Vendored file hashes (sha256 prefix): htmx.min.js `d6fdc75f204e6bde`, idiomorph-ext.min.js `5811b9a7eda14878`, datastar.js `7380e0344b7f937d`.

## Verdict

All three can build this page: in the stable layout every behaviour test passes for every variant, and under the same server the page code is identical (only the attribute-bearing elements differ, as D4a assumed).
What separates them is not behaviour but weight, what the CSP allows, and who owns the code. **Datastar** is the least client code to own (11 lines), has the best morph and the lowest server cost at 100 streams,
but does not run under the console's `default-src 'self'` policy, needs a per-response nonce or `unsafe-eval`, turns every attribute into executable code in a product that displays hostile Kafka data,
moves Pause and filter state into the server (per-connection state, a stream restart per debounced keystroke), pulls serde_json, and its Rust SDK is 0.x and adds 51 KB for what 10 lines do; it is the wrong fit for this console. **htmx** passes everything in the layout it is designed for and is safe under the console's CSP with `allowEval:false`,
but costs 52 KB (63 KB with the morph extension that the hostile layout needs, 1 config line away from passing), and the D4 recipe as written (ETag, fetch on tick) has three traps (browser hides the 304, `htmx.ajax` source, bfcache stream leak) that our glue still had to solve; the 52-line glue is not "40 lines of EventSource" but close.
**Our own script** passes every test including the hostile layout and the late-answer race, runs under the strictest policy, costs 4.2 KB raw (1.8 KB gzip) in total, and the 77 lines are ours: the only non-trivial piece is the 12-line morph, which is unkeyed (matches children by position), so it is right for text tables and wrong for rows that carry their own state (an expanded row, a selection) until it is given id-keyed matching (about 10 more lines) or replaced by idiomorph (11 KB, 0BSD).

## Recommendation

**Own script (C), which differs from the default "htmx if not decisive".** It is not decisive on behaviour (A and C tie), it is decisive on weight against a strained size gate (about 51 KB fewer client bytes than htmx and 62 KB fewer than htmx with morph; 66 KB less in the release binary) and it removes the "which htmx options and extensions" surface; the cost is 77 lines we maintain, and the reversal is cheap because only `console.js` and the manifest change (the page functions, ids and endpoints are the same for all three). Conditions that would flip it:

- to **htmx** if the owner does not want to own JavaScript at all, or the console grows to need more than about 200 lines of client logic (history/boost navigation, inline editing, optimistic UI, polling fallbacks), or htmx 4 (core morph, 37 KB) reaches `latest`; keep the glue and the three fixes above;
- to **own + idiomorph** (11 KB) if a page needs rows that carry state across updates and the keyed morph in our script proves insufficient;
- to **Datastar** only if the CSP is relaxed to a nonce policy deliberately, hostile text is wrapped in `data-ignore` everywhere (a lint), and the server accepts per-connection state; none of that is attractive for a Kafka console.

Whatever is chosen, the stream rules from this spike belong in D4: close on `pagehide` and hidden tabs, reopen on `pageshow`/visible, ETag handled by the script not the browser, newest request wins per fragment, `retry` unbounded, and `Idiomorph.defaults.ignoreActiveValue` or an equivalent if inputs ever sit inside a swapped fragment.

## What was not settled

- Only Chrome 152 headless was driven; Safari and Firefox are untested (`<dialog>`, `EventSource` and bfcache behave differently there). No screen reader pass. No test under real network latency or HTTP/2.
- Keyed lists: the own morph is unkeyed; a row with state (expanded drawer, selection) was not built, so that limit is argued, not measured.
- The Datastar server code does not share fragment renders between connections (each stream renders per tick); CPU numbers would change with a shared render cache. The htmx/own CPU number includes simulated-client requests made by one bun process, not real browsers.
- The htmx default (`drop`) overlap behaviour and Datastar's default retry limit were read from docs and a short outage, not tested for a long one.
- Bytes on the wire including HTTP headers were estimated, not captured.
- Test 1's keystroke loss on the hostile layout is timing dependent (it needs a keystroke inside the 2 to 5 ms response window); counts above are from 5 runs per variant before the one-line fix and 3 to 4 after.

## Files

`app/` (Cargo.toml, `src/main.rs`, `view.rs`, `ticks.rs`, `v_*.rs`, `assets/`), `vendor/` (htmx, idiomorph, Datastar, licences, htmx 4 for size only), `bin/` (the five release binaries), `build.sh`,
`tests/` (`lib.ts`, `browser.ts`, `server.ts`, `flaky.sh`, probes: `probe304.ts`, `probe_htmx.ts`, `probe_ds.ts`, `probe_leak.ts`, `probe_vis.ts`), `results/` (logs, JSON, package lists), `dl/` (Datastar SDK source and docs text).
