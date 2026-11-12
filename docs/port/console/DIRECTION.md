# The Kafka console: direction (draft 1, 2026-11-09)

Status: proposal for the owner's review; nothing is drawn. Grounding: `GROUNDING.md` beside this file. Frame and rules are those of
the doze-aws direction (canvas `8fb38ec7-de4e-4570-b8c8-d1eabfe70929`): design before code, every element grounded, pages published
as they are ready, every board rendered and looked at. Decisions taken by the owner (defaults, 2026-11-09): the old 34-spec suite is
retired and replaced by a board-diff gate; one stream opened only while a space is on screen (no polling); the app is its
`client.id`, credentials per app when SASL exists; the Lab causes only what production code can honestly cause; the console is
in-process; a new canvas, the Go boards kept as a checklist of what must not be lost.

## Thesis
Kafka UIs show what the Kafka APIs return: topics, offsets, messages, lag. This broker also knows the things the protocol hides:
the log as segments, what compaction and retention just did, why a group rebalanced, who holds a transaction open, why a request
was refused. The console is a place where a developer **understands their Kafka**, with the wire kept beside every action.

## The five jobs
| | Job | The question | Lives in |
|---|---|---|---|
| 1 | Connect | Point my client at this; know the moment its first request lands, and why it can't if it can't | Home |
| 2 | Watch | What is my app sending right now, and what just happened | Live |
| 3 | Build | Create topics, produce, read a message completely, change a config knowing what it does | Resources |
| 4 | Understand | Why is this group behind, why did it rebalance, why was that refused, what did compaction do | everywhere, led by Resources and Access |
| 5 | Rehearse | Drop a connection, expire a member, force a rebalance or a clean, slow one topic, then watch what my app does | Lab |

## Six spaces (labelled; never one icon per feature)
1. **Home**: both addresses with snippets for franz-go, sarama, librdkafka, kcat and Java; the advertised-listener check; a
   first-request detector; the instance at a glance (uptime, last stop clean or not, weight).
2. **Live**: the requests as they happen, as one table, newest first: app (`client.id`), API and version, topics, result, time; a
   refusal is a row that explains itself. The window is the recorder ring (the last 2,000 rows, said with its computed span), never a promised 3 hours. (The transit-line map of apps, topics and groups, its drawers and the scrubbed 3 h window
   were drawn and then dropped by the owner on 2026-11-10: out of this design, no lag trend either. See REALITY.md.)
3. **Resources**: the kinds join through a descriptor, not new chrome: **Topics** (Messages, Log, Config, Consumers, Producers),
   **Groups** (Members, Lag, Story), **Transactions** (open, since, timeout, what they hold back); Schemas join in Phase 10.
4. **Access** (Phase 8): one honest page that says what is on and off (sign-in, ACLs, TLS, the console principal) and lists the ACLs and users read only (canvas corrected 2026-11-12; Phase 8 is built).
   No previews of ACLs or denied requests are drawn.
5. **Lab**: a short list of rehearsals, each with an expiry and a visible rule on the connection layer; no disk faults (those stay
   in the tests).
6. **Settings**: one page in sections: the five dynamic broker configs, the Kafka version profile, the cleaner and retention
   intervals, the instance's weight, the data directory and what a restart does, the flags.

## What would make it better, not only prettier (each tied to data; see GROUNDING Part 4)
- **The log as segments** (K9, K8): an offset ruler with start, high watermark and last stable offset marked; sealed and active
  segments with size and newest time; after compaction the gaps shown where records were; the planner's answer to "why not
  compacted yet" (ratio against threshold, the active segment, an open transaction, a minimum lag); tombstones with the date
  they go; the last round and what it removed. No Kafka UI can show this.
- **A group's story** (K3, K4): generations and members over time, each rebalance with its cause (new member, left, session
  expired, rebalance timeout, topic change, protocol migration), who has not rejoined and how long the wait has left; lag per
  partition with the words Kafka consoles lack ("no commits yet", "committed past the end: the topic was recreated", "offsets
  expire in 3 days").
- **One message, completely** (K5, K15): key, value, headers, both timestamps, size, codec, null or empty or tombstone; which
  producer, epoch and sequence wrote it; whether it sits in a committed, aborted or still-open transaction, above or below the last
  stable offset, and so what a read-committed consumer sees against a read-uncommitted one; trace a header across topics.
- **A refusal explained** (K6): message too large, a keyless record on a compacted topic, timestamp out of bounds, out-of-order
  sequence, fenced epoch, unknown topic, offset out of range: the rule, Kafka's code and words, and what would pass.
- **Config teaching** (K10): value, where it comes from (topic, dynamic broker, static flag, default), Kafka's own doc string, the
  range, and when a change takes effect (now, next roll, per request); a before-and-after preview.
- **The call beside the action** everywhere: produce, create, alter and delete show the API call and the equivalent client
  snippet, as the aws direction does.

## Real or vanity (the tests the aws direction used)
Real: anything the broker records for its own work. Vanity until proven otherwise: throughput charts of days, a cluster map of one
node, replication views (there is none: one broker, said once, plainly), anything needing request bodies we do not keep.
Motion only for an event; reduced motion shows the same as a highlighted row.

## What has to exist in the broker first (the feed; small, bounded, lazily built)
1. A read-only inspect surface on `Broker` and `LogReader` (gates most jobs; no new state).
2. A request observer at `kafka-server/src/connection.rs:exchange` and a bounded in-memory recorder (2,000 rows, clipped strings, a
   `Summarize` per served API); gates Live, the first-request detector, the dock on every resource.
3. A small event vocabulary at the points that already decide: the cause of a rebalance, the reason of a refusal, the result of a
   cleaner round and of retention. Idle cost stays at zero; weight rises only with a written reason (`weight.md`).

## Work plan
1. This direction reviewed (this file). 2. A new canvas: Brief, Real or vanity, Spaces, Patterns, System (doze tokens, the aws
kit where it fits), the Live screen in the Paper direction; each published as it is ready. 3. The screens by wave: Home and
Live; Topics with the Log page first (the one nobody else has); Groups and the story; Transactions; Lab; Settings; Access and
Schemas as NOT BUILT boards. 4. A Grounding board (exists / new / later) and a compare-with-the-Go-console checklist. 5. Only then the
feed in Rust and the console in Phase 9, to the boards, with a board-diff gate.
