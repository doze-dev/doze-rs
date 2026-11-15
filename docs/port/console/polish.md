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

| 17 | Live, every filter | **no filter changed anything** on the fixtures server (the tape ignored the query); the search box did not ask until Enter | the fixtures tape passes rows through the recorder's rules (an idle poll row included); the search box asks 250 ms after the last key, replaces the history entry; every filter of every page has a scenario that counts rows (CON-760); a Rust test does the same against a real broker's tape | `filters-live-change-the-tape` (16 rows, 3 for a typed client, 4 for a client or a topic, 2 refused) |
| 18 | the Produce dialog and every overlay | taller than the window, cut off, the buttons out of reach | a flex column: fixed title, scrolling body, foot in view; drawers 100dvh (CON-761) | `target/ux/overlay-produce-1280x720.png`, `overlay-produce-800x600.png`; 19 overlays at 3 sizes |
| 19 | Home | a thin right column, long values, a layout that overflowed | a strip of six figures, Address and "This instance" in 7 to 5 (second at least 380 px, stacked under 1180), the way on across the page (CON-764) | `target/ux/home-1024.png`, `home-1280.png`, `home-1440.png`, `home-1920.png` |
| 20 | a Select's list | as wide as a narrow trigger: "fulfilme nt" | as wide as its longest option up to 420 px, one line with an ellipsis (CON-762) | `target/ux/select-menu-width.png` |
| 21 | every focused control | a purple rectangle inside the search pill; three different ring styles | one ring system (CON-763) | `target/ux/focus-0.png` to `focus-18.png` |
| 22 | the create-topic form at 1024 | three choice cards ran out of their card | the row wraps | scenario `every-page-fits-at-1024` |

Looked at and left (the page is right): Home, Connect, Planned pages, the group Members, Lag and Story, Consumers, the dialogs.

## Backlog from this pass

| P | Where | What |
|---|---|---|
| 2 | the Produce dialog | the teaching lines under a field ("Kafka says / Default / Takes effect / Here") are four lines each; a compact disclosure under the field would shorten the dialog (the dialog scrolls and its buttons stay in view, so it is a refinement) |
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
| 3 | planned pages | ~~the "Not built yet" copy says Home and Connect are here~~ closed (p9x): it lists the pages that are here |
| 3 | overlays | a scrollbar gutter when the page behind had none shifts it by the scrollbar's width on Windows and Linux (macOS overlay scrollbars do not) |
| 3 | `topic-produce` | the dialog opens with the focus on Partition; Value is where people type: focus it when the key is filled |

# Integration of 9g (Log, Producers) and 9h (Settings, Access): what Chrome showed (slice p9x, 2026-11-13)

Headless Chrome 152, 1440 wide, one at a time under the lock. Each of the 15 boards was rendered (the fixture render of the gate, with the console's own assets), taken at the height of the board's PNG and compared with it (`board-pixels --page-png`, canvas scaffold cut and the rail masked); every side-by-side was read. Then the fixtures server was clicked through end to end: Home, Topics, a topic, a record drawer (Esc, Back), a group, the Config dialog (an empty value answers in the dialog), Live, Settings and its dialog (Check), Access, the ACL dialogs (an empty one answers in the dialog, Back closes the delete), the palette. Console messages were captured over CDP (`Runtime.consoleAPICalled`, `Runtime.exceptionThrown`, `Log.entryAdded`).

## Fixed in this slice

| Where | What was wrong | Fix |
|---|---|---|
| Settings | p9h set its own `html{scroll-behavior:smooth}` for the section tabs, on top of the global one the UX pass added | p9h's rule is gone; the global one (reduced motion switches it off) does the tabs |
| Settings, edit dialog | "Who sees it" ran its list into the next sentence (`orders.dlq orders (7 d)`) and said "a, b, c" where the board says "a, b and c" | the list ends with a full stop and the prose lists say "and" before the last name |
| Log, Producers | the board draws the "On the wire" dock; the page did not | the dock (9e) is under both pages and watched with them |
| Producers | "Seen as" said "not known" for every producer | the recorder names the clients whose Produce carried the producer id |
| Access | the users table had no Iterations column | drawn as the board has it |
| every field-to-fix answer | a dialog's invalid field answered 422, which Chrome logs as an error in the console | a script is answered 200 with the dialog (J285) |
| every not-built page | the copy said only Home and Connect were here | lists what is here |

## Per board (after the fixes)

| Board | Remaining differences |
|---|---|
| `topic-log` | "Compact now" (Lab, J231); the header is the topic's own on every tab (the owner's rule), so Partitions, Produce and Delete topic stand where the board has a Lab button, and the Log's words are the first block under the tabs; the breadcrumb is Resources > Topics > name > Log |
| `log-planner`, `log-retention` | the page adds "The log, offset by offset" and the Gaps / Log start notes the board draws only on `topic-log`, so everything below is 80 to 100 px lower; `log-retention` has the partition chips (0, 1, 2) the board leaves to its header word (needed: the board gives no way to the other partitions); backlog P3: draw the notes only on the compacted state |
| `log-empty` | 10 px higher (the board's lede wraps to two lines); the same otherwise |
| `topic-producers` | the producers are a list of native disclosures with the board's columns (an expander is never a link), open for the one that holds the transaction; "Seen as" shows `orders (go, sarama)` from the tape, and "not known" where the tape never saw the producer (the board's "not seen for 3 h 12 min" is not built); "raised from 1 at 17:14:11" is not kept (J236) |
| `txn-open` | "Offsets staged: not listed" where the board says none (DescribeTransactions does not say); the transactional id is a link |
| `txn-holding` | the group sentence "payments-workers reads orders and is 12 behind on every partition" is not drawn (backlog P2: it can now come from the Groups pages) |
| `settings-home` | the breadcrumb is Home > Settings; the ledes run wider than the board's 520 px; the three sentences that waited are drawn (CON-582) |
| `broker-config-edit` | a Check button the board does not draw (an improvement, J277); the dialog sits 8 px lower (the canvas scaffold) |
| `broker-config-refused`, `broker-config-saved` | none that show |
| `access-open` | "See the page with security on" is not drawn (J265); Create ACL is offered on an open broker too (improvement); the canvas's NEW pill |
| `access-secured` | the canvas callout and the NEW pill; Iterations is there |
| `access-acl-create`, `access-acl-delete` | none that show; the page's delete says "also covered by 1 other ACL" where the board names it (the fixture's detail) |

## The click-through

- Overlays open in place (no navigation: a marker set on `window` survived), push their address, close with Esc and with Back, and restore the address: the record drawer, the Config edit dialog, the Settings edit dialog, the Create ACL and Delete ACL dialogs, the palette (with the seven pages first). A direct visit to each dialog's address is the parent page with it open (`tests/overlays.rs`).
- A refusal answers in the dialog: an empty value in the Settings dialog shows "Type a value." in the dialog, and the Create ACL dialog answers its own field error; the Config dialog's Save comes back as the Saved page (the fixtures accept any value: the refusals of a real broker are in `tests/live_config*.rs`), whose address is the parent's, so a reload shows the Config page (J205).
- Motion: the scrim fades in 160 ms and the dialog rises in 180 ms (read off `document.getAnimations()` 40 ms after the click); hover transitions 56 to 120 ms; only opacity, transform and colours move; the page area carries `view-transition-name: main`.
- Console messages: none (no exception, no warning, no 404). A field to fix in a dialog answered 422 and Chrome logs every 4xx a fetch gets as a red line, so an overlay answer to a script (`X-Fragment`) is now a 200 with the dialog and the mark (`handlers::overlays::fragment`); a plain POST keeps its 422.

## Backlog

| P | Where | What |
|---|---|---|
| 2 | `txn-holding` | the group sentence, from the Groups pages |
| 3 | `log-planner`, `log-retention` | draw "The log, offset by offset" and the Gaps note only where the board does |
| 3 | `topic-producers` | "not seen for 3 h 12 min" (a producer the tape has not seen since before its window) |
