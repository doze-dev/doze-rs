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
