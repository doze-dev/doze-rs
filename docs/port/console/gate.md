# The console gate: board-diff, snapshot and accessibility lint

Three bun tools, no browser, no npm packages (they use bun's own `HTMLRewriter`). They replace the Playwright suite (`docs/port/phase9.md`, D9 and "Testing"). The pixel check is a fourth, optional, manual tool at the end.

| Tool | Does |
|---|---|
| `tools/board-signature.ts` | The one place the *signature* is defined and extracted: `signature(html, {routes, base})` reads a board or a rendered page and returns the same typed structure for both |
| `tools/boards-snapshot.ts` | Signatures of the 96 boards into `crates/kafka-console/design/boards.json` |
| `tools/board-diff.ts` | Compares rendered pages with their boards; the gate |
| `tools/a11y-lint.ts` | Accessibility rules over rendered pages and the kit CSS |
| `tools/boards-refresh.ts` | Copies the canvas into `design/` (the contract) |
| `tools/pixels/board-pixels.ts` | One page against its board PNG, by hand, never in CI |

## Commands

```
bun test tools/test                                   # the tools' own tests (71): identity for all 96 boards, mutations, allow-list, lint
bun tools/boards-snapshot.ts [--check]                # write (or verify) design/boards.json
bun tools/board-diff.ts                               # every board that has a render in target/board-diff/ (fixture) or target/board-diff-live/ (live)
bun tools/board-diff.ts --ids home,connect            # only these
bun tools/board-diff.ts --require home,home-quiet,home-attention,connect,connect-advertised,connect-first-request
bun tools/board-diff.ts --list                        # built / not built, per board and per render
bun tools/board-diff.ts --json                        # the report as JSON
bun tools/board-diff.ts --dir D --live-dir L --base /console --routes F --allow F --slice 9a
bun tools/a11y-lint.ts [files...] [--css a.css,b.css] [--known file] [--warn] [--json]   # default files: target/board-diff/*.html
bun tools/boards-refresh.ts <canvas-dir> [--dry-run]
```

Exit status: 0 clean; 1 a difference, a bad allow-list, an unbuilt `--require`d board, or an a11y finding; 2 unusable input (no snapshot, or `boards.json` out of date with `design/boards`).

## What a page must offer

The gate reads the HTML the console serves. It needs very little of it, but it needs these:

- a `<main>` (or `.main`) holding the page's content; the top bar, rail and breadcrumb sit outside it (they are not compared, apart from the landmarks);
- a breadcrumb (`nav.crumbs` or `aria-label="Breadcrumb"`) whose last item has class `cur` (or `aria-current="page"`): that is the title, signature item (a);
- for a dialog or drawer, an element that is showing (`role="dialog"`, `<dialog open>`, `.modal`, `.drawer-panel`, not `hidden`) holding the board's content. Such a page is an *overlay* page: the signature is of the dialog only, it has no title and no `h1`. An empty `<dialog>` waiting for htmx is not an overlay;
- links as ordinary `<a href>`; the gate resolves each to a screen id through `design/routes.json`;
- `[data-vary]` on anything whose text changes between renders (times, ages, ids); its text is ignored;
- `data-scaffold` on anything that is canvas navigation and not product.

## The signature (what is compared)

`tools/board-signature.ts` defines the type `Signature` (documented field by field); this is the list. A board is read after the canvas kit would have drawn it, so its shell (top bar, rail, breadcrumb) is implied by its front matter.

| Item | Field | Content | Notes |
|---|---|---|---|
| (a) | `title`, `h1` | last crumb; `h1` texts | an `h1` with `.mono` is a *name* (the topic or group the page is about): compared strictly, not live |
| (b) | `headings` | `h2 Text`, `h3 Text` in order | `.mono` content in a heading is masked as `{}` |
| (c) | `tables` | per table: `th` texts in order, has body rows, `wire` | tables are matched to the board's by shared column labels, so a deleted table does not shift the rest; rows are not compared live |
| (d) | `tabs` | label (count dropped), `on` | |
| (e) | `buttons` | every `.btn` and `<button>`: label, variants (`pri danger fill sm`), action | action is `op:Name` (`data-op`), else `ui:kind` (`data-ui`), else `to:<screen id>` (a link), else `submit`, else `none` |
| (f) | `links` | the screen ids linked, a set | a rendered link is resolved through `routes.json`; unresolved is `?/path` and is a difference; a route shared by several screens resolves to the one the board links to |
| (g) | `cards` | per card (keyed by its heading): `dt` labels, `.field` label texts (the `.opt` note dropped), per `.callout` and `.banner` its variant and first bold phrase; plus a `page` pseudo-card for everything outside a card | live compares the callout's variant, not its words |
| (h) | `extras` | set of `data-extra` values | |
| (i) | `ops` | `data-op` name to count | live compares the set |
| (j) | `states` | `st ok/warn/bad/refused/info/idle`, `rw ...`, with counts | **changed from the plan**: a count outside table cells, and "some row has it" (`... in rows`) inside them. A set would let `st ok` become `st bad` unseen whenever the page already has a `st bad`. Live compares the set |
| (k) | `landmarks` | `banner`, `breadcrumb`, `main`, `rail`, `dialog` | read from the page; implied for a board |

Plus `kind` (`page` or `overlay`). Never in the signature: cell text, numbers, anything under `[data-vary]`, glyph SVGs and tokens, `[data-scaffold]`, `.from`, the breadcrumb, rail and top bar contents.

Two renders are compared per board:

- **Fixture render** (`target/board-diff/<id>.html`), strict on everything above.
- **Live render** (`target/board-diff-live/<id>.html`), structure only: same columns, buttons and ops, links, cards. Names, row presence, callout wording, counts of repeated buttons and ops are not compared.

## What it does not check

Colour, spacing, overflow, type, whether a button looks like a button, copy beyond labels, cell content, whether the numbers are right. It says the page *has the board's parts*; it cannot say it looks right. That stays the independent look in every slice gate, and the optional pixel check.

## The allow-list

`design/allow.json`, an array of `{board, field, selector?, reason, expires?, render?}`.

- `field` is a signature field name as the report prints it (`button`, `link`, `table.col`, `heading`, `dt`, `field`, `note`, `tab`, `extra`, `op`, `state`, `landmark`, `title`, `h1`, `kind`) or `*`.
- `selector` (optional) names one difference: its value as printed, or `scope: value`, or `kind value`. Without it the entry silences every difference of that field on that board.
- `reason` is required. `expires` is a slice tag (`9a` to `9i`, `close`); with `--slice 9b` an entry that expired with `9a` is an error. `render` limits an entry to `fixture` or `live`.
- Errors that fail the gate: an entry with no reason, an unknown board or field, a bad `expires`, and a **stale entry**: one that matches no difference on a board that was compared. Fix the page and the entry becomes stale; delete it.
- Standing rules, not entries: scaffolding (`data-scaffold`, `.from`), glyphs, `[data-vary]`, the shell outside `<main>`.

## How a slice adopts its boards

Boards with no render are reported "not built" and are not failures, so slices can be adopted one at a time. A slice's gate names its boards: `bun tools/board-diff.ts --require <ids>` (9a: `home,home-quiet,home-attention,connect,connect-advertised,connect-first-request`), which fails if any of them has no render in either directory and is otherwise as strict as everything else that is built. As soon as a board is built it is compared, required or not, so a regression in an earlier slice shows. `xtask console-boards` renders and then runs this.

## The accessibility lint

`lang` · one `h1` (none on a dialog page) · no skipped heading level · every control (`button`, `a[href]`, `input`, `select`, `textarea`) has an accessible name (`aria-labelledby`, `aria-label`, a bound or wrapping `<label>`, text, `alt`, `title`; **not** a placeholder) · `th` has `scope` or sits in `thead` · no positive `tabindex` · a dialog has `aria-labelledby` pointing at text · `aria-live`/`role=status|log` only on a page with `data-extra="live-sentence"` · `.st` and `.rw` have a word · `@media (prefers-reduced-motion)` if the CSS animates or transitions (the kit does neither today) · WCAG AA contrast, computed from the CSS custom properties: every rule with a `color` is paired with its own `background`, or with the surfaces `--bg`, `--panel`, `--well` (and `--side` in the rail); 4.5:1 for text, 3:1 for the domain hues (`.d-*`, which only tint glyphs); `opacity` (disabled), pseudo-elements, `svg` and the `.arr`/`.sl` separators are skipped.

Not checked: screen-reader reading, focus order beyond native controls (one manual pass per slice). `--known` takes `[{board, check, reason}]` for canvas defects that are listed instead of failing; a stale entry is an error. `tools/test/a11y-known.json` lists the canvas defects found so far.

## The pixel check (optional, manual, never in CI)

```
bun tools/pixels/board-pixels.ts <id> --url http://127.0.0.1:9093/... --canvas <canvas-dir> [--mask x,y,w,h]... [--threshold 0.1] [--fail-pct 1.5]
bun tools/pixels/board-pixels.ts <id> --page-png shot.png --canvas <canvas-dir>      # two PNGs, no Chrome
```

`cd tools/pixels && bun install` once (pngjs, pixelmatch; its own `package.json` and `bun.lock`). It serves nothing: pass the URL of a running console (or the `boards` example). One Chrome (`--headless=new --screenshot`, 1440 wide, as tall as the board's PNG, 20 s timeout, `nice -n 19`, its own user-data-dir) is started only when `pgrep -fl "Google Chrome" | wc -l` is 0, and killed after the page. A screenshot cannot mask `[data-vary]`, so changing text is covered by `--mask` rectangles. The side-by-side (board, page, difference) goes to `target/board-diff/pixels/<id>.png`. `--all` is refused unless `PIXELS_ALL=1`, and then needs `--url-template` with `{id}`.

## Refreshing the contract from the canvas

See `crates/kafka-console/design/README.md`: `bun tools/boards-refresh.ts <canvas-dir>`, `bun tools/boards-snapshot.ts`, `bun test tools/test`, then commit. CI runs `bun tools/boards-snapshot.ts --check` (the committed `boards.json` must be what the boards give) and `board-diff` refuses to run against a stale one.
