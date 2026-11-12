# The interaction tests: driving the console in real Chrome

`tools/ux/drive.ts` drives the console the way a person does (real mouse and key events over the DevTools Protocol, `Input.dispatchMouseEvent` and `Input.dispatchKeyEvent`) and asserts **numbers**: `scrollY` before and after, bounding boxes before and after, the drawer's rectangle, `layout-shift` totals from a `PerformanceObserver`, the animation each element runs (`getAnimations()`), the element that has focus, the address and the history length, and every console error, warning, exception and CSP violation the page produced. The structure gate (`board-diff`) and the DOM tests cannot see any of these; the owner did, so this exists. It is **not in CI** (it needs Chrome).

## Run it

```
cargo run -p kafka-console --features examples --example boards 127.0.0.1:19293     # the fixtures server, a port of your own
mkdir <scratchpad>/chrome.lock                                                         # when other agents share the machine: one Chrome at a time
bun tools/ux/drive.ts --url http://127.0.0.1:19293 [--only drawer,select] [--shots target/ux] [--list]
rmdir <scratchpad>/chrome.lock                                                         # and kill the server you started (by its pid, never by pattern)
```

`--list` prints the scenario names; `--only a,b` runs the scenarios whose name equals or starts with a given word; screenshots and `results.json` go to `--shots` (default `target/ux/`). The harness refuses to start when a Chrome is already running, uses a fresh profile directory and a free debugging port, and kills only the process it started. Exit status is 1 when any scenario fails. `bun tools/ux/probe.ts --url <base> <path> "<js>" "key:Enter" "click:<selector>" "shot:<file>"` loads a page and runs steps, for looking at one thing while writing a scenario.

## Write a scenario

Scenarios are small named functions in `tools/ux/scenarios/*.ts`; each file exports `scenarios: Record<string, (t: T) => Promise<void>>` and `drive.ts` loads every file. A scenario gets `t.page` (`goto`, `click`, `hover`, `key`, `type`, `focus`, `rect`, `eval`, `waitFor`, `scrollTo`, `scrollY`, `shift`, `resetShift`, `shot`, `back`, `latency`, `served`, `requests`), and asserts with `t.ok(cond, message)`, `t.near(a, b, tolerance, message)` and records numbers with `t.num(name, value)`. A scenario fails on any thrown error and on any console problem. The page's own instrumentation (`window.__ls`, the layout-shift observer) is installed before the page's scripts.

Rules for a page scenario (the UX brief): `scrollY` before and after an in-page control must be equal; the toolbar's and the header's boxes must not move; the layout-shift total of the scenario stays under 0.02; focus stays on the control that was used; the breadcrumb is the entity path only.

## What is covered (foundation)

| File | Scenarios | Asserts |
|---|---|---|
| `navigation.ts` | `stack-loaded`, `nav-swaps-main-only`, `nav-scroll-new-page-top-and-back-restores`, `nav-view-transition`, `nav-preload`, `nav-progress-hint-only-when-slow` | the document survives a click; title, rail and history follow; a new page starts at 0 and Back restores `scrollY`; `startViewTransition` ran with the 180/120 ms CSS; one preload and a click answered from the cache; no hint for a fast request, one for a slow request |
| `drawer.ts` | `drawer-geometry`, `drawer-motion`, `drawer-closes-three-ways-and-keeps-the-page`, `dialog-geometry`, `scroll-lock-does-not-shift-the-page` | drawer top 0, height = window height, right = window right, 480 px, above the top bar, left shadow; slide starts one width to the right (`translateX(100%)`), 240 ms `cubic-bezier(.22,1,.36,1)`, scrim 180 ms, out 180 ms; dialog scale .98, 180 ms, centred; Esc, scrim and Back close and give back scroll, focus, address; the page does not move when the scroll lock engages |
| `components.ts` | `gallery-screenshots`, `select-*`, `combobox-filters-and-creates`, `menu-keyboard`, `tooltip-delay`, `segmented-roving`, `filter-in-place` | the keyboard contracts of `components.md`, the 400 ms tooltip, the flip near the window's edge, hostile labels as text, and an in-place filter that keeps scroll, toolbar and focus with a layout shift of 0 |
| `overlays-and-palette.ts` | `dialog-form-result-replaces-the-page`, `palette-opens-filters-navigates`, `live-filter-in-place` | a dialog's POST result replaces `main` and the overlay in the same document; the palette navigates client-side; Live's Select chips change the tape in place |

UX agent 2 adds the page scenarios (breadcrumbs, the Consumers tab, Planned, the Messages toolbar, the expanders) as new files beside these.

## Traps found while writing it

- A sticky top bar (52 px) covers an element scrolled to `y < 52`: click targets are scrolled into view with `block: 'nearest'`, and the test that scrolls by hand leaves room.
- `getAnimations()` is paused at `currentTime = 0` to read where an animation *starts*; the whole tab is slowed (`Animation.setPlaybackRate`) so there is time to look.
- A `<select>` whose option list has no empty value and a value of `""` shows its FIRST option in a browser: the components add a hidden placeholder option (`components::select`), found by the Combobox scenario.
