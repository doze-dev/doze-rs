# @doze/ui

The components the doze consoles share. **Behaviour and layout, never a colour**: every component reads role variables
(`--background`, `--primary`, `--good`, `--row-h`, ...) and each console sets them in its own theme file. doze-kafka and
doze-aws use the same Button, DataTable, Sheet and palette and look like two products.

Seeded from doze-kafka's console (`web/src`, copied, not moved). The plan: `docs/port/aws/console-react-plan.md`.

## What is in it

| Path | What |
|---|---|
| `src/components/ui/` | The shadcn/ui components (Radix): Button, Dialog, Sheet, Select, Sidebar, Tabs, Tooltip, ... as the CLI wrote them, plus the changes listed below |
| `src/components/app/` | The frame and page pieces: CommandPalette, TopBar, PageHeader, RefreshControl, ThemeSwitch, DataTable (virtual past 500 rows), StatStrip, Combobox, MultiSelect, UtcDateTimePicker, CopyButton, CodeValue, IdValue, EmptyState, Alert and ErrorBlock (a failed fetch: what failed, the server's words and code, what to do, Try again), StatusDot, HeldPill, Done, Spans |
| `src/lib/` | `api` (the fetch client and event stream with the same-origin proof header), `sse`, `leaving`, `search-params`, `search`, `refresh`, `recents`, `theme`, `sidebar-state`, `opener` (focus return), `format`, `shell`, `utils` |
| `src/hooks/` | `use-overlay-opener`, `use-inert-page`, `use-mobile` |
| `src/styles/base.css` | The Tailwind v4 setup and the mapping from roles to utilities. Nothing of a product is in it |
| `themes/kafka.css`, `themes/aws.css` | The two products' values, light and dark, and the font files |
| `scripts/`, `tests/` | The guard rails (below) |

Every component is shown, working, in `apps/gallery` under both themes.

## How an app uses it

The package is source: an app's Vite build compiles it, so only what the app imports is in its bundle. It is consumed inside
this workspace (`"@doze/ui": "workspace:*"`); nothing is published.

```jsonc
// apps/<app>/package.json
{ "dependencies": { "@doze/ui": "workspace:*", "react": "19.3.1", "react-dom": "19.3.1", "@tanstack/react-router": "1.170.46" } }
```

```css
/* apps/<app>/src/index.css */
@import "@doze/ui/styles/base.css";   /* Tailwind, the role mapping, and the @source of the library's own classes */
@import "@doze/ui/themes/aws.css";    /* or the app's own src/theme.css */
```

```tsx
// apps/<app>/src/main.tsx
import { Toaster } from "@doze/ui/components/ui/sonner";
import { TooltipProvider } from "@doze/ui/components/ui/tooltip";
import { startTheme } from "@doze/ui/lib/theme";
import "./index.css";

startTheme({ storageKey: "doze-aws.theme" }); // before the first render; light, dark or the OS's
```

```tsx
import { DataTable } from "@doze/ui/components/app/data-table"; // one file at a time, or
import { Button, DataTable } from "@doze/ui";                    // from the index
```

Three things the app must have in its page, because the overlays rely on them:

- the app renders into `<div id="root">` (the page behind an open Dialog or Sheet is made `inert` there);
- its main region is `<main id="main" tabIndex={-1}>` (where the focus goes when an overlay's opener is gone);
- a `<TooltipProvider>` around the app, and one `<Toaster />`.

`PageHeader` links its breadcrumb with TanStack Router's `Link`, so the app has a router (a peer dependency). `CommandPalette`
and `TopBar` do not navigate: the app passes `onGo` and its own groups. `createApi({ unreachable, onRefusal })` makes the
app's client; every request and the event stream carry `X-Console: 1`.

## Theming, in four levels (cheapest first)

1. **Values.** A theme file sets every role for light (`:root`), for dark by choice (`:root.dark`) and for dark by the OS
   (`@media (prefers-color-scheme: dark) { :root:not(.light) }`, so the first paint is right before any script runs).
   Colour, type (`--font-sans`, `--font-display`, `--font-mono`, `--body-size`), `--radius`. Almost all of the difference
   between two products is here. The roles: run `bun -e 'import {rolesRead} from "./scripts/roles.ts"; console.log(rolesRead("src"))'`,
   or read `themes/kafka.css`.
2. **Density and motion.** Also roles, not code: `--control-h` (a button, a field, a select), `--row-h` (a DataTable row,
   read where the table is), `--dur-in`, `--dur-out`, `--ease` (a Dialog and a Sheet coming and going), `--dur-highlight`
   (a new row). doze-aws is denser than doze-kafka with the same table. Reduced motion is honoured in `base.css`.
3. **Variants.** The components keep shadcn's `cva` variants and take `className` (merged with `tailwind-merge`, so the
   app's class wins). A product-only tone (doze-aws's Denied) is a class or a wrapper in that app, using that app's own
   role, not an edit here.
4. **Override.** When a component must behave differently in one product, the app keeps its own file and imports that one.
   The shared file is untouched. Each override is listed in `apps/<app>/OVERRIDES.md` with its reason. Use sparingly.

A role only one product has (doze-aws's `--denied`, doze-kafka's `--hatch`) lives in that app's theme and that app's
components: this package does not know it.

## Guard rails

Run by `bun run lint`, `bun run test` and `bun run e2e` in `console/`.

- **No literal colour in `src/`** (`scripts/colour-lint.ts`, `tests/colours.test.ts`): a hex, `rgb(`, `hsl(`, `oklch(`, a
  Tailwind palette class (`bg-yellow-400`, `text-white`, `bg-black/50`) fails. `color-mix()` of roles, `transparent` and
  `currentColor` are fine.
- **Every role read is set by both themes, light and dark** (`scripts/roles.ts`, `tests/roles.test.ts`): the test lists
  what `src/` reads (`var(--x)`, `h-(--x)`, `getPropertyValue("--x")`) and does not set itself, and checks each theme file's
  three blocks; the two dark blocks must be the same.
- **The gallery in three browsers** (`apps/gallery/e2e`): every page under kafka and aws, light and dark, in Firefox, WebKit
  and Chromium; no console error; Dialog, Sheet, Popover, the palette, Select, Combobox, MultiSelect and the menu driven by
  mouse and keyboard; the focus ring checked on every control Tab reaches.

Not covered by the lint: a named colour in an inline `style`, and a Tailwind utility for a role that `base.css` does not
map (it produces no CSS, so it shows in the gallery as a missing colour, not as a failure).

## What changed from doze-kafka's files

So that moving doze-kafka onto this package is mechanical, the copies differ from `web/src` only in:

- imports: `@/components/...` became relative paths (an app's own `@` alias cannot reach into a package);
- `bg-black/50` (Dialog and Sheet overlays) became `bg-scrim`; `text-white` on a destructive Button and Badge became
  `text-destructive-foreground`. `themes/kafka.css` sets both roles to the old values;
- Button, Input and Select's default height is `--control-h`; a Dialog's and a Sheet's durations and curve are `--dur-in`,
  `--dur-out`, `--ease`; a DataTable's row is `--row-h`. `themes/kafka.css` sets them to what was hard-coded (36 px, 200 ms,
  150 ms, ease-out, 40 px);
- `lib/theme.ts`: `startTheme({ storageKey })` (doze-kafka passes `doze-kafka.theme`);
- `lib/recents.ts`: `kind` is any word, the storage key is a parameter, and an address starting `//` is dropped;
- `lib/api.ts`: `createApi(options)` in place of a module bound to doze-kafka's words and its closed-to-network state
  (`onRefusal` is the hook for that); one SSE parser (`lib/sse.ts`) for the stream, fixed for a CRLF cut across two chunks;
- `CommandPalette` takes its groups and `onGo`; `TopBar` takes its right side as children; `HeldPill` and `Spans` no longer
  import doze-kafka's tail and API types.

Left in `web/`, as doze-kafka's own: `app-sidebar`, `brand`, `nav`, `disk-alarm`, `TailSpeed`, `lib/kafka.ts`,
`lib/topic-search.ts`, the page parsers of `lib/search.ts`, `api/closed.ts`, `api/path.ts`.

## Commands

```sh
cd console
bun install
bun run build      # typecheck the package, build the gallery
bun run test       # unit tests and the two guard-rail tests
bun run lint       # eslint, then the colour lint
bun run e2e        # the gallery in Firefox, WebKit and Chromium (bun run install-browsers once)
bun run --cwd apps/gallery dev   # the gallery with hot reload
```
