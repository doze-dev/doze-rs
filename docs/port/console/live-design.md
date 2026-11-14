# Live updates in the console: one design

**Decided 2026-11-13 (owner: "Stop this piecemeal work. This is excessively broken."). Replaces every earlier real-time instruction** (the Home live fragment, the stream of ticks of slice 9a, the Live pause of slice 9e, the fade of `console.js`). Catalogue rules: CON-800 to CON-812. Debt rows: J411 to J440 in `docs/port/debt.md`.

## The measured problem

Real broker, traffic slowed five times. With the console open on one page the stream emitted about 28 `tick` events per 10 s on Home, 24 on Live and 22 on Topics: two to three a second on every subscribed page, because the recorder bumped one shared tick for every request of every client (heartbeats, fetches included) and a 2 s floor timer bumped it again. Every tick refetched the page's fragments and morphed them, fading whatever differed, relative times ("2 s ago") included. Rows without a stable identity were matched by position, so one new row at the top changed every row below; the fade was applied to parents (a whole `tbody`); live numbers sat inside prose, so sentences re-wrapped from one line to two; tables laid their columns out from the rows they held, so columns compressed and spread as rows of other widths arrived.

## How established dashboards do it

Pages are snapshots; refreshing is a deliberate act. Grafana: an off-by-default refresh picker (Off, 5 s, 10 s, 30 s, 1 m), with a server-enforced minimum of 5 s. RabbitMQ management: a "Refreshed <time>" stamp and a "Refresh every N seconds" selector. Hawtio and Artemis: an auto-refresh option with a configurable rate. Sidekiq: an explicit "Live Poll" button, off by default. Kafka message viewers put tailing behind a toggle or a separate Live Tail view. A tail is the only thing that is genuinely live: opt-in, batched, rate-limited, and it never moves what you are reading.

## The rules (CON-800 to CON-812)

1. **Every page is a snapshot.** No `EventSource`, no timer, no poll, no tick, on any page, until a person asks (CON-800). There is no per-request wake-up of page subscribers and no floor timer (CON-801).
2. **The Refresh control** (`ui/refresh.rs`, `assets/live.js`, Alpine `refreshControl`) sits at the right of the breadcrumb row of every data page: a `Refresh` button (key `r`), a stamp `Updated 14:32:07` (UTC, the server's render time, never "s ago"), and an interval picker `Off · 5 s · 10 s · 30 s · 1 min` (CON-802). Default Off; minimum 5 s; remembered per browser in `localStorage` (`doze:refresh`), one preference for all pages. A chosen interval polls the page's regions (`[data-frag]`) with `If-None-Match`; an unchanged region answers 304 and nothing happens; a changed one is morphed in place, only what differs (CON-803). Nothing animates. The stamp changes when new data was applied. No polling while the tab is hidden; on becoming visible, one refresh if the interval has elapsed (CON-804). A pending interval request is aborted by the next one; a failed one leaves the page as it was and the stamp unchanged.
3. **Prose never holds a live value** (CON-805). A fact that changes is a **metric slot** (`ui/metrics.rs`: `Stat`, `stat_strip`, `stat_tile`: a label, a value in its own slot with reserved width, tabular figures, `nowrap`, an optional unit, an optional static tooltip) or a table cell. A sentence is static text rendered once. Times are absolute (`Last write 14:32:05`, `Started Nov 6 18:02`); the relative form is only in a tooltip computed at render, never updated (CON-806).
4. **A layout never depends on a data value** (CON-807). A conditional block lives in a region of reserved height (the transaction callout: a fixed-height region that is either the callout or a calm "No open transactions"). A sentence's length never moves anything below it.
5. **Tables are DataTables** (`ui/datatable.rs`, CON-808): `table-layout: fixed` with a `<colgroup>` the page declares (px, % or one fill column); cells `nowrap`, `overflow: hidden`, ellipsis with the full value in `title`; numeric columns right-aligned, tabular figures; the `<thead>` is never part of a refresh (it carries `data-morph-skip`; the tail only ever touches `tbody`); every row has a stable id (`id="r-<key>"`, `data-row`); fixed row height per density (comfortable 44 px, compact 36 px); a reserved minimum height for the empty state. No layout depends on the rows held.
6. **The only streaming views are tails, both opt-in** (CON-809): the Live page (the request tape) and a topic's Messages page. Each opens as a snapshot of the newest rows with a segmented control `Snapshot | Live tail` (default Snapshot) and the Refresh control. In Live tail: the server **coalesces** (at most one event a second, only when rows were added or folded; the cluster's traffic rate never becomes the page's update rate); the client **batches**; rows are **prepended**; a new row gets a 1.5 s background highlight; rows are keyed by id, so nothing existing is rewritten (a folded repeat's count and time change in place, silently); the header and toolbar are never part of an update; **held** when you are not at the top, or hovering, selecting or focused in the table: a `↑ 37 new · Show` pill, and the scroll position never changes. The connection indicator: green `Live`, amber `Reconnecting…`, grey `Snapshot`, red `Disconnected · Retry in 8 s` (backoff 1, 2, 4, 8, 15 s). Pausing is switching back to Snapshot. On reconnect the browser's `Last-Event-ID` is the newest row id the client has, so the server replays only newer rows: no duplicates (CON-810). The stream names only the tail region; a per-subscriber cap stays (`max_streams`).
7. **No opacity fades on updates, ever** (CON-811). The only motion is the new-row background highlight and the overlay and page transitions already built. A number that changes just changes.
8. **A deadline is a slot** (CON-812): an absolute time plus a client-side countdown in its own `<time>` element, updated locally each second with no request.

## What we got wrong (so that it is not done again)

- Whole-region replacement on every tick: a refresh morphs by stable id and touches only what differs.
- Auto-layout tables on live data: columns compressed and spread as rows of other widths arrived; a DataTable declares its columns.
- Applying every event to the DOM at once: a tail batches (at most one a second) and holds what you are reading.
- Unbounded buffers: the DOM keeps 200 rows, the held rows 500.
- Ignoring focus, scroll and the hidden tab: a poll waits for them, a tail holds for them.
- A sentence with a number in it: the line re-wrapped from one line to two as the number changed length.
- A fade of a parent block on every update: nothing fades.

## Speed (the owner: "control the speed a person wants to refresh in")

Polling pages: Off (default), 5 s, 10 s, 30 s, 1 min (least 5 s). Tails: Realtime (a batch each second, the fastest there is), Relaxed (3 s, the start), Slow (10 s), Paused (nothing is fetched). Both are remembered per browser (`doze:refresh`, `doze:tail-speed`); the tail asks the server for its pace (`pace`, clamped to 1 to 30 s: never faster than once a second); the tooltip of the control states the rate.

## Per-page table

| Page | Mode | What refreshes (regions) |
|---|---|---|
| Home | Snapshot + Refresh | `home-live`: stat strip, transaction callout region, cards |
| Topics | Snapshot + Refresh | `topics-stats`, `topics-table` |
| A topic: Messages | Snapshot + Refresh + **Tail** | `topic-stats`, `topic-messages`; tail prepends to the table body |
| A topic: Partitions, Consumers | Snapshot + Refresh | `topic-stats`, the page's table |
| A topic: Log, Producers | Snapshot + Refresh | `log-stats`, `log-ruler`, `log-cards`; `producers-stats`, `producers-table` |
| Groups, a group, Lag, Story | Snapshot + Refresh | `groups-stats`, `groups-table`; `group-stats`; `group-lag-stats`, `group-lag-table`; `story-now` |
| Live | Snapshot + Refresh + **Tail** | `live-wire` (stats, tape); tail prepends to the tape body |
| Config, Settings, Access, Connect | Snapshot, no control | nothing |

## Components (API, so pages can use them)

- `ui::metrics::{Stat, stat_strip, stat_tile}`. `Stat::new(label, value).unit("records").tip("exact value").ch(8)`; `stat_strip(&[Stat])` is a bordered row of stats (`<dl class="stats">`), `stat_tile(&Stat)` a single one (`<dl class="stats tile">`). The value slot is `width: Nch` (default 8), `nowrap`, ellipsis, `tabular-nums`; the unit sits after it. Text only; never a sentence.
- `ui::refresh::refresh_bar(base, regions)` renders the control; the shell places it in the breadcrumb row when a page lists regions.
- `ui::datatable::{DataTable, Col, Width, Density, data_table}`; a row is `(id, cells)`.
- `ui::tail::tail_bar(...)`: the `Snapshot | Live tail` control, the indicator and the held pill; the table is a DataTable whose `tbody` has `data-tail`.

## Proof

`tools/ux/scenarios/live.ts` against a real broker with traffic: `stream-rate` (requests after load, DOM mutations after load, 0 with the default settings on every page), `refresh-interval`, `text-stable`, `tail-*`, `table-stable`. Numbers in the final report of the pass and in `docs/port/console/ux-tests.md`.
