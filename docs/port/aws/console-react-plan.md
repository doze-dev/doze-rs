# The doze-aws console in React + shadcn/ui, sharing Kafka's components, themed apart

Owner decisions, 2026-11-13/14: the doze-aws console moves to **React + shadcn/ui + TanStack Router**, as doze-kafka's did
(`docs/port/console/react-plan.md`, R0 to R5 done on main). The two consoles **share their components** but are **themed
differently**: "while we can use the same components, we should be able to theme the two differently ... leave that kind of room".
Same hard constraint as Kafka: one process, the bundle embedded in the binary, no Node at runtime, works offline.

The design is the one already drawn: direction B (Paper, light, transit-line map), six spaces (Home, Live, Resources, Access, Lab,
Settings), services joining Resources through a descriptor and never as new chrome (canvas
https://claude.ai/artifact/JkCiMcWtKs5doXpxUQghuS). Boards are the floor; improvements allowed; grounded-in-code rule applies.

## What is shared and what is each app's

```
web/                          bun workspace (one bun.lock)
├─ packages/ui/               SHARED: behaviour, never a colour
│  ├─ src/components/ui/      the shadcn components (Button, Select, Combobox, MultiSelect, Dialog, Sheet, Sidebar, Tooltip, ...)
│  ├─ src/components/app/     shared frame pieces: CommandPalette, RefreshControl, DataTable (+ Virtual), TopBar slots, ThemeMenu
│  ├─ src/lib/                api client (same-origin proof header), sse, search-params, refresh, recents, theme switching
│  └─ src/styles/base.css     Tailwind v4 setup, the @theme mapping from role variables to utilities, motion, no values
├─ apps/kafka/                today's web/src, moved (pages, Kafka's theme, its generated types)
│  └─ src/theme.css           Kafka's values: Bat yellow + Ink, warm neutrals, Bricolage for the wordmark
└─ apps/aws/                  new
   ├─ src/theme.css           doze-aws's values: Paper (direction B), the doze brand tokens, its own accent
   ├─ src/services/           one descriptor per service (what Resources lists, columns, actions, deep pages)
   └─ src/pages/              Home, Live, Resources, Access, Lab, Settings
```

Each app builds to its own crate: `crates/kafka-console/web-dist/` (unchanged path) and `crates/aws-console/web-dist/`, both
committed, both checked stale by a Rust test, as Kafka's is today.

### The room for theming, in four levels (cheapest first)

1. **Values.** Every shared component reads only role variables: `--background`, `--foreground`, `--card`, `--primary`,
   `--accent`, `--muted`, `--border`, `--ring`, `--radius`, `--font-sans`, `--font-mono`, `--font-display`, plus the status
   roles both products need (`--good`, `--warn`, `--bad`, `--refused`, `--link`, `--pick`, tints). Each app's `theme.css` sets
   them for light and dark. This is where almost all of the difference lives: colour, type, corner radius, shadows.
2. **Density and motion.** Also variables, not code: `--row-h`, `--control-h`, `--gap`, `--dur-in`, `--dur-out`, `--ease`. AWS
   lists can be denser than Kafka's without forking the table.
3. **Variants.** Components keep shadcn's `cva` variants; an app may add its own variant (`<Badge tone="denied">` for AWS's
   Denied, distinct from Refused) by extending the variant map it passes in, not by editing the shared file.
4. **Override.** When a component must look or behave differently in one product, the app keeps its own file and imports that
   one; the shared one is untouched. Used sparingly, and each override is listed in `apps/<app>/OVERRIDES.md` with its reason.

Roles that only one product has (AWS's `--denied`, the transit-line map's line colours, Kafka's `--hatch`) live in that app's
theme and that app's components; the shared package does not know them.

**Guard rails (tests, so the room does not leak):**
- a lint over `packages/ui` that fails on any literal colour (`#…`, `rgb(`, `oklch(`, Tailwind palette classes like
  `bg-yellow-400`): shared code names roles only (Kafka's `components/ui` already passes this today);
- each app's `theme.css` must define every role `packages/ui` reads (a test lists the `var(--…)` uses and checks both themes);
- Playwright renders the component gallery under **both** themes, light and dark, in Firefox, WebKit and Chromium: a component
  change that breaks one product's look is caught before either ships.

### Who moves Kafka's files

The workspace move (`web/src` → `apps/kafka/src`, shared parts → `packages/ui`) touches the Kafka session's tree. It is done as
one mechanical commit with no behaviour change (Kafka's Playwright suite and the stale-bundle test must pass unchanged, and the
built bundle must be byte-identical or explained), agreed with that session before it lands.

## The doze-aws side (Rust)

- A new crate `aws-console`: the axum routes under the gateway's existing `/_doze/` prefix (or its own listener; decided in R0):
  `GET /assets/*` (embedded, gzip, immutable), `GET /api/v1/...` (JSON view-models), `POST /api/v1/...` (writes through the peer bus
  as the console's own caller, so IAM sees them), `GET /api/v1/tail/live` (SSE), `GET /*` → `index.html`. Host guard, origin
  proof, CSP `default-src 'self'` as Kafka's.
- View-models in Rust with `serde` + `ts-rs` (a test writes `apps/aws/src/api/types/*.ts`; stale fails), so the UI cannot drift
  from the backend.
- Live: the call record the gateway already sees (service, action, resource, caller, status, refused/denied, duration, cause
  chain) kept in a bounded ring, buckets updated on the request path (no timer: zero idle wake-ups stays true).
- Resources: each service exposes a small descriptor (lists, columns, detail fields, actions mapped to its own operations, deep
  page if any); the generic Resources pages render any descriptor, so a service joins without new chrome.

## Slices (each shown to the owner in Firefox before the next starts)

| Slice | Content | Agent sessions (estimate) |
|---|---|---|
| W0 Workspace | `web/` → workspace; `packages/ui` extracted; Kafka unchanged (its suite green, bundle identical); colour lint and theme-completeness tests | 1 |
| A-R0 Foundation | `aws-console` crate, embed + SPA fallback + CSP, ts-rs contract, AWS theme (Paper), shell with the six spaces, palette, RefreshControl, gallery under both themes | 2 |
| A-R1 Live | the transit-line map (flows, one bead per call), the call tape (SSE, aggregated repeats, refused vs denied), the call Sheet with its cause chain | 2 to 3 |
| A-R2 Resources | descriptors for all 17 services, the generic list and detail pages, create/delete through the bus | 3 |
| A-R3 Deep pages | SQS messages, S3 objects, Lambda invocations and logs, Step Functions executions (graph + events), DynamoDB items, Logs tail, CloudFormation stack events | 3 to 4 |
| A-R4 Access, Lab, Settings, Home | IAM views and the "would be refused" log, Connect with both URL kinds, Lab, settings | 2 |
| A-R5 Close | two independent audits (security; UX in three browsers at four widths), fixes, weight measured and argued | 1.5 |

Total about 14.5 to 16.5 agent sessions. Weight is measured at every slice against the 20 MB ceiling (line = measured + 5%).

## Open before W0

- Kafka session's agreement to the workspace move (or the owner's word that it may proceed).
- Console on the gateway's port under a path, or its own port (Kafka uses its own listener).
