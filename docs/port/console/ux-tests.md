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

UX agent 2's page scenarios (breadcrumbs, the Consumers tab, Planned, the Messages toolbar, the expanders) are `tools/ux/pages-drive.ts` (its own small harness, `pages-cdp.ts`). The integration pass adds four more files to `drive.ts`:

| File | Scenarios | Asserts |
|---|---|---|
| `regions.ts` | `region-topics`, `region-groups`, `region-config`, `region-lag-expiring`, `region-jump`, `region-messages` (partition, level, key, chip X, Clear filters, Back), `region-messages-older`, `region-messages-time`, `region-missing-falls-back-to-navigation` | each control, by `inPlace()` (`tools/ux/inplace.ts`): same document, `main` not swapped, `scrollY` unchanged, every box above the data unchanged (to 0.5 px), layout shift under 0.02 (it is 0 in every one), the address (clean), the history (pushed for a chip, replaced for typing), the focus on the control, no console problem; Back undoes |
| `popups.ts` | `popups-stay-clear-of-the-rail` | a menu opened at the far left of the content starts right of the rail (it was 114 px over it) and inside the window |
| `palette.ts` | `palette-sections-and-recents`, `tooltip-shared-for-title-and-icon-buttons`, `row-menu-opens-an-overlay` | Topics, Groups, Pages headings, a section with no match loses its heading, the place just opened is the first recent and only a kind and a name are stored, the empty state; the shared tooltip appears at 400 ms with the exact time and gives the `title` back; the row menu is inside the content and its Edit opens the dialog |
| `transitions.ts` | `transitions-no-document-flash-across-pages`, `transitions-forms-swap-in-place`, `transitions-reduced-motion-runs-none`, `transitions-fast-clicks-do-not-stack`, `transitions-overtaken-click-never-lands-late`, `transitions-back-forward-restore-scroll` | see "Transitions, measured" below |

The owner's review of the preview build added five more files (CON-760 to CON-764):

| File | Scenarios | Asserts |
|---|---|---|
| `filters.ts` | `filters-live-change-the-tape`, `filters-lists-change-the-data` | for every filter of Live, Topics, Groups, Messages and Config: pick a value (the enhanced Select by mouse, the search box by typing) and the ROWS change (counted before and after), the address follows, Back restores the earlier rows, the stream fragment keeps the filter |
| `overlays-fit.ts` | `overlays-fit-and-scroll-at-three-sizes` | 19 overlays at 1280x720, 1024x640, 800x600: the panel inside the window, a dialog's foot and every button in view without scrolling, the body scrolls when longer, a drawer from top to bottom on the right edge, the page behind does not scroll |
| `popups.ts` | `select-menu-fits-its-labels` | the list is as wide as its longest option (420 px at most), not narrower than the trigger, one line per option, an ellipsis, `title`, inside the window |
| `focus.ts` | `focus-one-ring-on-the-outer-shape` | 19 kinds of control, focused: exactly one 2 px ring in the primary colour, on the outermost shape; the input inside a box draws none |
| `overflow.ts` | `home-fits-at-four-widths`, `every-page-fits-at-1024` | Home at 1024, 1280, 1440, 1920 (not wider than the window, no element's content wider than its box, two columns 7 to 5 with the second at least 380 px, stacked under 1180); 27 other pages at 1024 |

## Transitions, measured (CON-757)

Run against the fixtures server, Chrome 152, 1280 by 800:

| What | Number |
|---|---|
| a walk of 10 clicks (rail, a table link, the topic's tabs, a crumb, the Groups tab, a group) | the same document the whole way (a marker on `window` survives every step); 10 page changes, 10 view transitions; 0 animations left running at the end |
| the same walk's `main` | replaced by each page change, the top bar and the rail kept |
| animations running during one page change with `prefers-reduced-motion: reduce` | **0** (17 with motion allowed: the check is not vacuous); `scroll-behavior` is `auto` |
| five clicks 40 ms apart (Live, Topics, Settings, Live, Topics) | ends on Topics with one `main`, the rail on Topics, 5 view transitions (a click that comes while a swap is on its way waits for it), 0 animations left |
| a click that follows another when the earlier answer is the slower (1200 ms against 60 ms) | the page, the address, the rail and the title are the later click's; the late answer does not swap and logs nothing (an abort logs a console error in htmx 2: this is why it is latest-wins and not an abort) |
| Back, then Forward, with scroll 240 on the Messages page and 120 on Config | each page comes back at the scroll it was left at (±2 px), instantly (the page has `scroll-behavior: smooth`; a restore uses `behavior: "instant"`) |

Traps found while writing these: `html{scroll-behavior:smooth}` makes `scrollTo` glide, so a scenario that scrolls and clicks at once clicks a moving page (three foundation scenarios failed for this: the harness now scrolls with `behavior: "instant"`, and so does the script's own restore of a scroll); the harness takes the shared Chrome lock only through the wrapper you give it (`mkdir <scratchpad>/chrome.lock`), `cdp.ts` ignores a lingering `chrome_crashpad_handler`; a link collected before a swap is a different element after it (the rail is swapped out of band), so a scenario that clicks "later" looks it up when it clicks.

## Screenshots to look at

`bun tools/ux/shots.ts --url <base> --out target/ux/shots/<name> [--sizes 1440x900,1024x768] [--eval "<js>"] /topics /groups ...` takes a page at each size and prints, for the page, horizontal overflow, controls under 24 px and clipped text: the part of a designer's pass a script can do. The pictures are for the Read tool.

## Traps found while writing it

- A sticky top bar (52 px) covers an element scrolled to `y < 52`: click targets are scrolled into view with `block: 'nearest'`, and the test that scrolls by hand leaves room.
- `getAnimations()` is paused at `currentTime = 0` to read where an animation *starts*; the whole tab is slowed (`Animation.setPlaybackRate`) so there is time to look.
- A `<select>` whose option list has no empty value and a value of `""` shows its FIRST option in a browser: the components add a hidden placeholder option (`components::select`), found by the Combobox scenario.

## The live design's proofs (`scenarios/live.ts`, run against a REAL broker's console with traffic)

`bun tools/ux/drive.ts --url http://127.0.0.1:<console port> --only stream-rate,refresh-interval,text-stable,tail-live,tail-messages,table-stable,live-above-the-fold` (and `tail-reconnect` with `BROKER_CTL=<script that takes stop and start>`). Hold the Chrome lock at most three minutes: `stream-rate` is about 105 s (`RATE_SECONDS` per page), `text-stable` about 80 s (`STABLE_SECONDS`), the others under 30 s. Numbers of the 2026-11-13 run (debug broker, the demo's traffic): requests after load 0 and DOM mutations after load 0 on 15 pages; Refresh 5 s: 6 requests in 16 s, all conditional, 14 of 44 cells touched (the ones whose data changed), layout shift 0; the strips, toolbars and table headers of six pages kept their boxes for 12 s of moving data; the tail added at most one batch a second (least gap 999 ms) and rewrote none of the rows that were there, with the page header and the toolbar byte-identical in screenshots 3 s apart and `scrollY` unchanged; the header cells of the tape changed width by 0 px over 20 s of new rows at 1280, header and body cells lined up to 0 px at 1024, 1280, 1440 and 1920 with no sideways scroll; at 1440x900 and 1024x768 the filters and the first row of the table are on screen without scrolling. Screenshots in `target/ux/`.
