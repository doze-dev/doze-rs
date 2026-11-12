# Console polish: what was fixed, and the backlog

Two passes. The first (slice p9o, 2026-11-13) is at the end of this file. The second, the integration pass (UX agent 3, branch `ux-integrate`), looked at every built page in real Chrome at 1440 by 900 and 1024 by 768 (`tools/ux/shots.ts`; the pictures are under `target/ux/shots/before/` and `target/ux/shots/after/`, one file per page and size, `<path with underscores>.<width>.png`) and fixed what a demanding designer flags. Prefer a few deep fixes: these are the ones.

## Integration pass: what was found and what was done

| # | Where | What a designer sees | Fix | Before / after |
|---|---|---|---|---|
| 1 | every page at 1024 px | the Messages table runs 74 px past the window (the page scrolls sideways), Live 122 px, a topic's Wire 42, Partitions 29; the Messages toolbar's "Reading as" is cut | below 1180 px the rail keeps its icons (the names are tooltips), the page padding is 28 px, a table cell is 12 px; a key never wraps (CON-758) | `before/topics_orders.1024.png`, `after/topics_orders.1024.png` (and `live`, `topics_orders_wire`, `topics_orders_partitions`) |
| 2 | Messages, row two | "From time, UTC" is a bare field among pills, 30 px high beside a 36 px toolbar, with nothing saying what to write | a Popover button "From time" with a labelled field, an example, the rule, and Apply (CON-753) | `before/topics_orders.1440.png`, `target/ux/region-messages-time.png` |
| 3 | Messages, the partition control and "Reading as" | buttons that rebuilt the whole page; a topic with 50 partitions would draw 50 buttons | Segmented controls (links, roving focus); a Select past 8 partitions | `target/ux/region-messages-key.png` |
| 4 | Messages, a record's value | JSON wraps mid-token in a narrow cell ("ord-\n88418") | the value is one line with an ellipsis, a key never wraps | `after/topics_orders.1024.png` |
| 5 | a filter field in focus | two rings (a square one inside the rounded box) | one ring, on the box (CON-759) | `target/ux/region-topics-empty.png` (before, the inner square), `after/topics.1440.png` |
| 6 | Topics, "Show internal" | the switch never moved: the toggle was drawn `off` whatever the state | the `on` class follows the state | `target/ux/region-topics-empty.png` |
| 7 | Topics, a filter that matches nothing | the toolbar was replaced by another (no Refresh, a chip and a Clear), so typing lost the focus and the toolbar changed shape | the same toolbar in every state; the card says Clear filter (CON-750) | `target/ux/region-topics-empty.png` |
| 8 | Groups | four chips with counts, buttons | a Segmented control with the counts | `before/groups.1440.png`, `after/groups.1440.png` |
| 9 | Config | the row's two buttons (Edit, Reset) for a topic's own value | a DropdownMenu; a value that is not the topic's keeps one button | `target/ux/row-menu.png` |
| 10 | every heading and paragraph | one-word last lines ("it.", "orders-0.") | `text-wrap: balance` on headings, `pretty` on paragraphs | `before/topics.1440.png`, `after/topics.1440.png` |
| 11 | any menu, select, popover | a menu opened near the left of the content ran 114 px over the rail | the card is nudged clear of the rail (CON-754) | `target/ux/menu-by-the-rail.png` |
| 12 | the palette | one flat list; no memory | Topics, Groups, Pages; the last five places; an empty state (CON-755) | `target/ux/palette-sections.png`, `target/ux/palette-recents.png` |
| 13 | a truncated key, an exact time, an icon button | the browser's late native tooltip, or none | one shared card (CON-756) | `target/ux/tooltip-shared.png` |
| 14 | Back and Forward | the page glided down to where it had been (the page has `scroll-behavior: smooth`) and Forward found the top | an instant restore; the scroll of the page left by Back is kept too (CON-757) | scenario `transitions-back-forward-restore-scroll` |
| 15 | a click during a page change | the second click aimed at the element that was going: the address and the page disagreed | it waits for the swap; the later click wins over a slower earlier answer (CON-757) | scenarios `transitions-fast-clicks-do-not-stack`, `transitions-overtaken-click-never-lands-late` |
| 16 | reduced motion | the browser's default fade between pages still ran | `::view-transition-*` animation off (CON-757) | scenario `transitions-reduced-motion-runs-none` |

Looked at and left (the page is right): Home, Connect, Planned pages, the group Members, Lag and Story, Consumers, the dialogs.

## Backlog from this pass

| P | Where | What |
|---|---|---|
| 2 | the top bar | its right edge is 24 px from the window and the page's is 48 px: the chips do not line up with the page's right edge |
| 2 | Live | "Clients · 9" is a small button (32 px) beside 36 px chips; its text and the chips' differ by a pixel in centre |
| 2 | Live at 1024 | the toolbar wraps: "Clients · 9" drops to a second row; the refused callout's list leaves a gap line |
| 2 | Messages | a value shows its preview only; hover shows no more (J385) |
| 3 | the palette | second-level jumps (a topic's tabs); no loading state needed while the answer is local (J383) |
| 3 | every table | sticky headers (J203) |

---

# First pass (slice p9o, 2026-11-13)

Reviewed in real Chrome (headless, 1440 by 900) against the fixtures server: the 30 built pages, the 7 overlay boards in place and by direct visit, the palette, a short viewport (500 px), a 64 KiB value in the drawer and a 249-byte topic name. Screenshots were read one at a time; the checks (horizontal overflow, hit targets under 24 px, clipped text) were run on every page by script.

## Fixed in this slice

| Where | What was wrong | Fix |
|---|---|---|
| every page | the top bar and the rail scrolled away with a long page, so a drawer under the bar left a gap | both are sticky; the drawer sits right under the bar at any scroll |
| overlays | a dialog taller than the viewport was cut at the top; the record drawer's Copy button stayed hidden (it is revealed by the script at load) | the overlay scrolls inside itself; the script reveals what it puts in |
| page header | a 249-byte topic name ran 3,966 px wide and scrolled the page sideways | the name wraps where it must (header, breadcrumb) |
| Messages | the filter toolbar touched the table (the kit's margin lost to a more specific rule) | the 16 px the board has |
| tables | an offset or a topic name was a 20 px high target | the hit area is 34 px with no change of layout |
| forms | a focused field drew two rings (the box and the input) | one ring, on the box |
| buttons, rows, tabs, rail | no hover state, or none that eased | 120 ms colour, border and shadow; pressed states; focus rings that match the boards, also inside overlays |
| copy buttons | a click gave no sign | the label says Copied for 1.4 s in the width it had |
| no-topic pages | a dead end | a second button, Find a topic |
| shell | the keys the rail promises did nothing | j and k, Enter, Esc, and ⌘K / Ctrl+K are built |

## Backlog (prioritised)

| P | Board or page | What |
|---|---|---|
| 1 | every table (`topics-list`, `topic-detail`, `topic-partitions`) | sticky headers: `table.t` clips its corners with `overflow:hidden`, which makes the table the scroll container; draw the radius another way (J203) |
| 1 | `topic-detail`, `topic-readcommitted` | a key or a value that is cut in its cell shows no full value on hover; the drawer has it, the table could say so with a `title` and a middle ellipsis for keys (the end of a key is its useful part) |
| 2 | `topics-list` | a 249-byte topic name in a table cell: the row grows; wrap it or middle-ellipsis it with the full name on hover |
| 2 | `topic-record` | next and previous record buttons in the drawer's head (j and k do it; a click should too) |
| 2 | palette | groups (J201); recent topics first; the topic's tabs as second-level jumps (orders > Config) |
| 2 | `home`, `connect` | prose links ("Snippets for every client", "Open the transaction") are 20 px high targets; they are text links, give them the hit area |
| 3 | planned pages | the "Not built yet" copy says Home and Connect are here; it should say what is |
| 3 | overlays | a scrollbar gutter when the page behind had none shifts it by the scrollbar's width on Windows and Linux (macOS overlay scrollbars do not) |
| 3 | `topic-produce` | the dialog opens with the focus on Partition; Value is where people type: focus it when the key is filled |
