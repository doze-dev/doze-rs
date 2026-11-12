# Console polish: what was fixed, and the backlog (slice p9o, 2026-11-13)

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
