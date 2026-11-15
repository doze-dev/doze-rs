# Data control: what is using the disk, and giving it back

The owner, 2026-11-15: "I want users to be in control of their data. Sometimes data may get too high in the disk and people may not
know why. Through doze kafka, figure out ways to clearly show how much data is being used and give people a way to completely
truncate at topic level and everything if they choose to."

Built on branch `worktree-agent-a171c05e20c5757c1` (2026-11-15). The rules and their tests are CON-1515 to CON-1522
(`behaviours/console.md`) and SEC-294 (`behaviours/security.md`).

## What the user sees

- **Data**, a page of its own in the sidebar (between Schemas and Access), linked from Home's instance card, from Settings' Data
  section, from the heads-up and from the top bar's Disk chip.
  - *On disk*: the data directory's total, a bar of what kind of thing holds it (topics; the metadata store `meta.redb`, which
    holds the committed offsets, transactions, ACLs, users and schemas; partition directories that belong to no topic; other
    files), each with its bytes, its share and a line of what is in it; the directory, with Copy.
  - *The disk it is on*: the volume's size and room (`statvfs`; on a platform that does not say, the page says so), and doze-kafka's
    share of it.
  - *Topics, largest first*: each topic's size and share of the directory, its partitions, its oldest data, the retention in force
    (or **Keeps everything** when it deletes with no time or size bound), how fast it grew lately, and a sentence of why it holds
    what it holds ("No retention limit: it keeps everything written to it, for ever.", "Compacted: it keeps the latest value of
    every key ...", "Retention 7 days: whole segments past it are deleted. The oldest goes in about 3 days."). Each row has Empty,
    and Delete (the topic's existing Delete dialog).
  - *Give space back*: Empty all topics and Erase everything, each saying what it frees.
- **A heads-up** on Home, on the Data page, as a dot on the sidebar's Data and as a Disk chip in the top bar, when the disk is nearly
  full (below 5% or 2 GiB free), when the data directory is a tenth of the volume (and at least 1 GiB), or when a topic that keeps
  everything passes 1 GiB. Home does not say "Nothing needs you" while it shows.
- **`doze-kafka data`** prints the same: the directory by kind, the topics largest first with their share and an UNBOUNDED mark,
  and why each holds what it holds.

## The three actions

| | Empty topic | Empty all topics | Erase everything |
|---|---|---|---|
| Removes | every record of the topic, compacted topics too, the active segment with the rest | the same, for every topic | every topic, every group with no members and its committed offsets, every transactional id whose transaction is over, every schema subject and schema, partition directories no topic owns |
| Keeps | the topic, its partitions and configs, committed offsets | the same | ACLs, users made over the API, the broker's dynamic configs and flags, the registry's global level, the producer id counter and the next schema id |
| Right (ACLs on) | DELETE on the topic | ALTER on the cluster | ALTER on the cluster |
| Confirmation | the dialog says what goes | type `empty all topics` | type `erase everything` |
| Refused while | a transaction is under way on the topic | any transaction is under way | any transaction is under way |
| CLI (broker stopped) | `doze-kafka data empty --topic NAME` | `doze-kafka data empty --all-topics --yes` | `doze-kafka data erase --everything --yes` |

**Empty is the broker's own action, not DeleteRecords.** Kafka refuses DeleteRecords on a topic that only compacts; emptying is the
writer's `delete_before(None)` (the same durable path DeleteRecords to the high watermark takes: the new start is recorded durably
first, a fresh segment begins at the end, the old segments go), run between two appends by the partition's own task. So:

- offsets never go backwards: each partition carries on from its end, and a record written after gets the next offset;
- a consumer below the new start is told OFFSET_OUT_OF_RANGE (as after retention) and starts again by its `auto.offset.reset`;
  earliest and latest are the same offset, so nothing is read twice; committed offsets are left as they are;
- a producer carries on; an idempotent producer whose batches are gone is taken as new, as after DeleteRecords;
- a fetch waiting at the end goes on waiting there.

**Transactions: refused, not cut.** Emptying while a transaction has records in the log and no marker would leave its marker
ending nothing, and its commit would commit nothing. The broker refuses before touching anything when the coordinator has a
transaction under way on the topic (or anywhere, for the two wide actions), and the partition's writer checks its own index again,
between appends, so a transaction that begins in between is caught: CONCURRENT_TRANSACTIONS, with "commit or abort it (or wait
for its timeout), then try again". The console says it in the dialog before the button, which stays shut.

**Erase and live connections.** Topics go the way DeleteTopics deletes them (the registry first, then the open partitions close,
then the files), so clients see each one vanish and fetches waiting on one are answered; groups rebalance. A group with members
connected stays a group, with nothing committed. With auto-create on, a producer that keeps writing makes its topic again at once:
that is Kafka's behaviour too. Users and ACLs are kept on purpose (who may use the broker is not data); the dialog says so and points
to the Access page for removing them. No option to clear them here: two pages that each do one thing are clearer than a checkbox
that turns "start fresh" into "lock everyone out".

**Crash safety.** A stop at any point of emptying leaves the partition whole or empty, never with a hole and never handing out an
offset twice: `log::tests::trim::emptying_stopped_at_any_point_leaves_the_log_whole_or_empty_and_never_reuses_an_offset` halts the
process after every operation and opens it as a restart and after power cuts of every kind. Erase is a run of DeleteTopics and
DeleteGroups steps, each already crash-safe; a stop part-way leaves fewer topics, never a broken one.

## Cost

Nothing runs on a timer. The usage is a walk of the data directory (bounded at 200,000 entries, nothing opened for writing) plus
one batch header per partition, made when a page asks (Home, the Data page). "When the oldest segment goes" is read from the
retention sweep's own last verdict (`partitions::upkeep`), not worked out again. Growth is measured from the looks pages ask for:
at most one a minute is kept, sixteen of them, the 500 largest topics each; while nobody looks, nothing is measured. The top bar's
chip reads the last look and never walks.

## The CLI and a running broker

`doze-kafka data` with the broker running does not take the lock and never opens `meta.redb` (redb would repair a file another
process has open): it reads the files alone, says what they hold by topic, and points to the console for the rest. The actions
need the broker stopped: they take the data directory's lock (a running broker holds it), open the directory as the broker does,
do the same broker calls the console makes, and stop cleanly (the clean-stop record is written, so the next start trusts the
files). A running broker is told to the user in a sentence with the way forward; exit 1.

## Not done, and why

- **The doze module's admin actions.** The module speaks to its broker over the Kafka protocol only (`module/client.rs`), and no
  Kafka API empties a topic: DeleteRecords to the high watermark would be a different "Empty" (refused on compacted topics, no
  transaction guard). Erase has no protocol equivalent at all. Adding either also changes the app's pinned console fixture
  (`module/console.snapshot.json`, `apps/doze-app/fixtures/consoles.ts`). The app opens the console (`web_url`), where the Data page
  is. Left for the owner: offer them through a console API call from the module, or as DeleteRecords-based "Empty" for delete
  topics only.
- **Internal topics.** This broker keeps committed offsets, transactions and schemas in `meta.redb`, not in `__consumer_offsets`,
  `__transaction_state` or `_schemas` logs, so there is no internal topic to exclude: Empty all topics touches only the user's
  topics by construction, and the metadata store is shown as its own kind.
- **Per-table sizes inside `meta.redb`** (how much of it is offsets, how much schemas): the page shows the counts instead. redb does
  not shrink its file when records go, so Erase frees the topics' bytes, not the store's.
