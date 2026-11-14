# The doze-kafka console (React)

The console's page layer: React 19, shadcn/ui (Radix), Tailwind v4, TanStack Router, Query, Table and Virtual, cmdk, sonner,
react-hook-form + zod, built by Vite under bun. The plan and its rules: `docs/port/console/react-plan.md` and
`docs/port/console/live-design.md`. The build is **embedded in the doze-kafka binary**: one process, no Node at run time.

## Build

```sh
cd web
bun install --frozen-lockfile
bun run build        # Vite into crates/kafka-console/web-dist/, then .gz files, manifest.rs and source-hash.txt
```

Commit `crates/kafka-console/web-dist/` with the sources. `cargo test -p kafka-console` fails when the bundle is stale
(`web::tests::the_embedded_bundle_was_built_from_the_current_web_sources`) and when the TypeScript types of the API are
(`api::types::the_typescript_types_of_the_api_are_current`, which rewrites them: commit, then rebuild). CI rebuilds and
fails on a difference.

Text files are embedded gzipped (`pack` in `scripts/build.ts`) and served by `files::packed` (gzip as it is, inflated for a
client without gzip, ETag, 304). Hashed files are cached for a year; `index.html` is `no-cache`. Every GET under the root
that is not `/api/`, `/assets/` or a file answers `index.html`, so deep links work. There is no other console: the
server-rendered one that lived under `/classic/` was removed in slice R5.

## Check

```sh
bun run typecheck && bun run lint && bun test     # types, eslint (React hooks rules, no warnings), unit tests in src/
```

## Develop

`bun run dev` serves the app with hot reload and proxies `/api` to a running console (`DOZE_CONSOLE`, default
`http://127.0.0.1:9093`). The end result is always checked against the binary (below).

## The browser gate (local, before anything is called done)

```sh
cargo build --release -p doze-kafka             # the binary the gate runs (it embeds web-dist: rebuild after bun run build)
cd web/e2e && bun install && bun run install-browsers   # once: Firefox, WebKit, Chromium (not committed)
bun run test                                    # starts the binary on 127.0.0.1:39092/39094 with a seeded world
E2E_URL=http://127.0.0.1:29094 bun run test     # or against a console already serving a broker with traffic
```

One browser at a time (`workers: 1`). Every test fails on a console error or an uncaught exception. On a shared machine take
the lock first (`mkdir .../chrome.lock`, `rmdir` after).

## Conventions

- **A page**: `src/pages/<name>.tsx` exports `XxxPage`; a route in `src/router.tsx`.
  Search params are typed with a parser in `src/lib/search.ts` (no zod in the main bundle). A page is a snapshot: its query
  uses `staleTime: Infinity`, `placeholderData: keepPreviousData` and the shared interval (`useInterval` in
  `src/api/queries.ts`); the header is `PageHeader` with ancestors-only `crumbs`, the h1 once, and `RefreshControl` when the
  page has live data. Figures go in `StatStrip`, never in a sentence. Tables are `DataTable` with fixed `size`s and a stable
  `rowId`. An overlay with its own address is a search param (see the record Sheet in `pages/gallery.tsx`).
- **An endpoint**: in Rust, a response type in `crates/kafka-console/src/api.rs` (or `src/api/<area>.rs`) with
  `#[derive(serde::Serialize)]` and `#[cfg_attr(test, derive(ts_rs::TS))]` on it and on every view-model it holds; a handler
  answering `json(StatusCode::OK, &value)`; a line in `api::routes`; a line in `api/types.rs`'s list; a path in `GET_PATHS`
  (the planted-secret test reads them all). Run `cargo test -p kafka-console api::types` to write `web/src/api/types/*.ts`,
  then a hook in `src/api/queries.ts`. A refusal is `api::error(status, CODE, words)`, Kafka's error name as the code where
  Kafka refused. A write is a POST: the guard asks for `X-Console: 1` and the same-origin proof (the client sends both).
- **An e2e scenario**: `web/e2e/tests/<area>.spec.ts`, importing `test` and `expect` from `./fixtures` (console errors fail
  the test). Drive with the mouse and the keyboard, assert outcomes (the value chosen, the URL, the data), never only that
  something rendered. Run it in all three browsers.
- **Components**: shadcn's in `src/components/ui` (added with `bunx --bun shadcn@4.23.0 add <name>`; check the files land in
  `src/`), ours in `src/components/app`. Every component is shown, working, on `/gallery`.
