# The console screens against what can be built and what a developer will open

Written 2026-11-09, after the 137 screens of the direction canvas were drawn (artifact "doze-kafka console · direction").
The screens are a design, not a promise. This sorts them by two questions, so that Phase 9 builds the part that earns its
place first: **does what the screen stands on exist (or is it small to add)?** and **would someone using a local Kafka
for development open it, and how often?**

Sources: `kit/SCREENS.md` of the canvas (each screen's `data:` tag), the Rust broker as of `ecb8c87` plus the Phase 7 fixes
(the APIs it serves, what it records), and `console/GROUNDING.md` (what the Go console did and how it was used).

## What exists in the Rust broker today, and what does not

Exists, so a console can read it in-process: Metadata, topics and configs (DescribeConfigs and its sources), ListOffsets
(start, end, last stable), Fetch (messages), Produce, the group APIs including KIP-848 (ListGroups, DescribeGroups,
ConsumerGroupDescribe, OffsetFetch), DescribeProducers, DescribeTransactions and ListTransactions, DeleteRecords,
CreatePartitions, AlterConfigs, the five dynamic broker configs, the flags.

Does not exist yet, and several screens stand on it:

| Missing | Screens that stand on it | Honest size |
|---|---|---|
| A **request recorder** (a bounded ring of what each request was and how it was answered, with a `Summarize` for each API and a table of what each error means) | the whole Live space, every "On the wire" dock, request-detail, the refusal drawers, the first-request screen | 4 to 6 sessions. This is the largest single piece, and the Go console spent about 2,100 lines on it |
| A **group event ring** (why a group rebalanced: the machines know the cause at the moment they decide; nothing keeps it) | group-story, story-rebalance, story-flapping, the group wire | 1 to 2 sessions |
| A **connection table** (client id, host, software from ApiVersions) | live-clients | 1 session |
| **Log introspection** for a handle: segments, the planner's verdict (`judge` is `pub(crate)`), what the last cleaner and retention rounds did | topic-log and its drawers | 1 to 2 sessions |
| **Fault rules** in the request path (slow, fail, drop, expire, force) | the whole Lab space, live-lab-rule, log-compact-now | 4 to 6 sessions, and it is a feature of its own |
| A **per-minute activity ring and bead mechanism** | the transit map, the 3 h strip, the scrubbed window | 2 to 3 sessions after the recorder, and mostly interface work |
| **Phase 8** (ACLs, SASL) | access-home and its two previews | waits for Phase 8 |

The console crate itself (axum, maud, the shell, the in-process handle, the host guard, assets) is the plan's Phase 9 and is
the base of everything: 8 to 12 sessions for the first pages.

## The four tiers (137 screens)

**A. Build first: used every day, and what it stands on exists. 60 screens.**
Home and Connect (the advertised-listener check is the commonest first-day failure of a local Kafka, and it is cheap);
Topics (list, create, the topic page with Messages, record view, produce, partitions, add partitions, delete records,
delete); Groups (list, group page, members, lag, reset offsets, delete); Config (view and edit a topic's configs with the
broker's own refusals); the open transaction that holds a topic back (the "my consumer is stuck" case); Settings home.
The Messages table and a group's lag are where most of the time goes.

**B. Build second: real value, needs one new facility. 19 screens.**
The wire (live-wire, request-detail, the refusal drawers, the first-request screen, the dock on topics and groups): for
"what did my app send and what did the broker answer" it is the most useful thing in the tool after Messages, and it is
the recorder's cost. The group story. The Log page of a topic (segments and, for a compacted topic, the planner).
The connection list.

**C. Real but rare: fold into fewer pages, do not build each as a screen. 35 screens.**
- The six Settings info pages (intervals, flags, profile, weight, data, restart) become one page with sections.
- Producers and transactions: topic-producers, txn-open and txn-holding are the ones that matter; the other nine are states
  of those (timeout, fenced, expired, aborted, idempotent, offsets staged) and become sentences or rows, not pages.
- Log drawers and states (segment, index, flush, gaps, tombstones, planner-blocked, the "last round" drawers): reached
  only by someone running a compacted topic and asking why; keep the ruler and the planner verdict, make the rest rows.
- Config extras (synonyms, doc, compare, effect): the first two are one expandable row each; compare is optional.
- home-instance, topic-internal, group-empty states, the calm story: cheap states, build with their page.

**D. Aspirational: defer, or wait. 23 screens.**
- **Lab (12 incl. its two echoes).** Fault rehearsal is a good idea and a feature in its own right; it is used rarely by
  anyone, and it needs hooks in the request path. Not a reason to hold the rest back.
- **The transit map and its relatives (6):** map, both, app, topic and group drawers, the scrubbed window. A pretty way of
  reading what the wire table says exactly. The wire table is the product; the map can follow it.
- **Record trace (1)** scans recent records of every topic by a trace id: slow, budgeted, and the Go console's version was
  barely used. **Lag trend (1)** keeps hours of history that nothing records yet; the current lag, which is what is asked, stays.
- **Access (3)** waits for Phase 8 and is drawn once as "not built". Cheap to keep as one honest page.

## What people actually do with a local Kafka, and so where the weight goes

In order of how often: look at what is in a topic (Messages); produce a test record; see whether a consumer group is
keeping up or stuck (Lag, Members, Story when it is rebalancing); find out why a client cannot connect (advertised listener,
the wire); create or delete a topic and change a retention or cleanup setting. All of these are tier A or B. Compaction
internals, producer sequences, transaction states, segment indexes, broker flags and fault rehearsal are things a
developer opens a few times a year, usually while debugging one particular problem; they deserve to be findable and
correct, not a screen each.

## Suggested build order for Phase 9

1. The console crate and the shell, the Home and Connect pages, Topics (list, page, Messages, produce, create): the part a
   developer will use on day one. Tier A of Home and Topics: about 31 screens, 8 to 10 pages.
2. Groups and Config: the rest of tier A.
3. The recorder, then the wire (Live as a table, the dock on topic and group pages, request-detail, the refusal drawers).
4. The group event ring, then the Story tab.
5. Log page, then the folded Producers page.
6. Later, if wanted: the map, Lab, the trend.

That is about 78 screens of the 137 built before anything speculative, and the design keeps all 137 as the picture of where
it can go.

## Corrections the review turned up in the drawn screens

- `topic-log` and `topic-producers` stand partly on feeds (the cleaner's last round, client names from the wire): their
  `data:` tag was `now` and should say `capture`. The drawn pages already say so on the page.
- The topic page's "On the wire" dock is wire data, though the page is tagged `now`: without the recorder the dock is
  absent, and the page must read well without it.
- Home says "Nothing needs you" while a transaction is open and a group is rebalancing: correct by its own rule (neither is
  near a timeout), but the first-run reading of the screen should show what the rule is.
- Some small numbers in the screens (client ports, topic sizes, idle times) are invented to fit the world; they are marked
  in the agents' notes, and none is a claim about the broker.

## Owner's decisions (2026-11-10)

The owner answered the tiers above. The canvas ("doze-kafka console · direction") now draws **94 screens**, down from 137.

**Tier C: folded (32 screens gone, content kept in the parent page).**
- Settings: intervals, flags, profile, weight, data and restart are sections of `settings-home` (one page); the three broker-config dialogs stay.
- Producers: `topic-producers`, `txn-open` and `txn-holding` stay; producer detail, idempotent, aborted, expiry, fenced, list, none, offsets and timeout are rows and sentences on them.
- Log: `topic-log` (ruler and planner verdict), `log-planner`, `log-retention`, `log-empty`, `log-compact-now` stay; blocked planner, last retention, last compaction, delete-records effect, flush, gaps, index, segment and tombstones are rows and cards inside them.
- Config: synonyms and Kafka's text are expandable rows on `topic-config`; "when it takes effect" is one sentence under each row and a timeline on `config-saved`; compare is dropped.
- Cheap states: `home-instance` is the This instance card on Home; `topic-internal` is a callout under the Topics table; `group-empty` is the analytics group's Lag page (`lag-ahead`), with the "Nobody is in this group" callout and Delete group. `group-story-calm` (owner, 2026-11-12: fold it too) is a card on `group-story`, "What a calm group looks like" (search-indexer, one event), and the Story tab of search-indexer's pages is inert, as the Members tab of the empty group is.

**Tier D: Lab kept, the rest dumped (11 screens gone: 137 - 32 - 11 = 94).** All Lab screens stay (10, plus `live-lab-rule` and `log-compact-now`). Dumped: the transit map and its relatives
(`live-map`, `live-both`, `live-app`, `live-topic`, `live-group`, `live-window`), `topic-record-trace`, the lag trend (`lag-behind`; `group-lag` keeps the
current lag only) and the Access previews (`access-home`, `access-acl-preview`, `access-denied-preview`; `access-open` is the section's one page).

**Screens per section now:** Home 7, Live 10, Topics 26, Log 5, Config 8, Producers 3, Groups 20, Access 1, Lab 10, Settings 4 = 94.

**Phase 9 build order, revised.**
1. The console crate and shell, Home and Connect, Topics (list, page, Messages, produce, create).
2. Groups and Config (tier A).
3. The recorder, then Live as a table, the dock on topic and group pages, request-detail and the refusal drawers.
4. The group event ring, then the Story tab.
5. The Log page, then the folded Producers page, then the one Settings page.
6. Later and separate: **Lab** is its own feature (fault rules in the request path). The map and the lag trend are out of this design and out of the plan.
