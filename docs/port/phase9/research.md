# Phase 9 research: what the console stands on

Written 2026-11-12. Read-only: nothing in any repo was edited. The only builds are the weight spike under
`/private/tmp/claude-501/-Users-srini-Code-doze-dev/014d8981-fa59-41b3-a494-fc4a4035c0bb/scratchpad/phase9/weight/` (no Chrome was started; the PNGs
of the canvas were looked at with the image reader: home, topics-list, topic-detail, topic-record, topic-produce, live-wire, group-detail,
group-lag, topic-config, topic-log, settings-home, access-open).

Sources read: `docs/port/PLAN.md` (Phase 9 row, design table, crate map, decision log), `phase8.md`, `behaviours/security.md` (SEC-180 to 189),
`debt.md` I17 I20 I21, `console/{DIRECTION,GROUNDING,REALITY}.md`, the canvas working copy (`kit/README.md`, `kit/SCREENS.md`, `kit/world.md`,
`kit/kafka.css`, `kit/paper.css`, all 94 `src/<section>/<id>.html` front matters), the code (`kafka-broker/src/{lib,broker,caller,authorizer,cleaner}.rs`,
`broker/dispatch.rs`, `kafka-server/src/{server,connection}.rs`, `connection/exchange.rs`, `doze-kafka/src/cli/console.rs`,
`kafka-auth/src/hostguard.rs`), the Go console (`console/routes.go`, `static/`, `e2e/tests/`, `internal/broker/telemetry.go`,
`internal/protocol/errors.go`). The doze-aws direction has no Rust console yet (its Rust tree has a `kafka-console` stub and a hyper gateway);
what transfers is its rules (design before code, every element grounded, board-diff gate, one stream while a space is on screen), not code.

Prior art (thin, and said so). Kafka UIs: Kafbat UI, AKHQ, Redpanda Console, Conduktor. A search found their feature lists (topics, messages,
consumer groups and lag, config, schemas) and nothing on their internals; all show current state with no stored lag history, which agrees with the
owner dropping the lag trend. For the Rust stack, public projects pair axum with maud and vendored htmx (maud chosen for a smaller dependency tree
and fast compiles), embed small assets with plain `include_bytes!`, and larger bundles with `include_dir` or `rust-embed`; axum's `Sse` plus a
keep-alive is the usual live feed. That matches what the spike measured.

## 1. What the 94 screens stand on

The canvas front matter says (counted by script, `screens.json` beside this file): 63 `now`, 10 `capture:wire`, 3 `capture:wire+refusal`, 3
`capture:rebalance`, 1 `capture:connections`, 2 `capture:cleaner` (one also wire), 12 `lab`. By kind: 21 pages, 46 states, 18 dialogs, 9 drawers.
The 63 `now` is optimistic: many of them need a small read accessor (marked "small" below) because the broker's types are `pub(crate)`.

Legend: **exists** = the protocol handler answers it today, call it through `Broker::handle`; **small** = a read accessor on the broker or storage,
no new state; **ring/table/recorder** = a new bounded capture must exist first.

| Screen ids | Data it stands on | Status | The broker call |
|---|---|---|---|
| home, home-quiet, home-attention | topic and partition counts, uptime, last stop clean, open partitions of N, flags, the "needs you" rules (an open transaction near its timeout; a partition offline) | exists + small (uptime `started_ms`, `StopState`, open count are `pub(crate)` or private) ; "9 apps spoke in the last 3 h" needs recorder | Metadata, ListTransactions, `Broker::open_partitions`, view `instance()` |
| connect, connect-standalone | listen and advertised addresses, SASL mechanisms on, TLS on, registry off | exists (config) | none; `Facts` handed in by the binary |
| connect-advertised | Metadata's own answer for the advertised host:port, plus a TCP dial of it and an ApiVersions v0 | small: the console dials the address it advertises (a 40-line raw client in the console crate; the check is about the network, not the handle) | Metadata; raw ApiVersions |
| connect-first-request | the first request of a client id this broker has not seen, with its versions | recorder (needs client id and ApiVersions software fields) | recorder `first_seen` |
| topics-list, topics-none, topics-filtered | name, partitions, cleanup policy, size on disk, last write, "read by" groups | exists for name, partitions, policy; **small** for size on disk and last write (segment sizes are `pub(crate)`); "read by" = groups with committed offsets on the topic (`MetaStore::groups_with_offsets`); "written by" = recorder only (the sentence is dropped until 9e) | Metadata, DescribeConfigs, view `partition_files()`, OffsetFetch (all groups) |
| topic-detail, topic-messages-empty, topic-messages-filtered, topic-readcommitted | records (key, value size, timestamp, "for read_committed" column), bounds, LSO | exists | Fetch (isolation level, `max_wait 0`), ListOffsets (start, end, last stable, by timestamp), DescribeTransactions for the open-transaction banner; records decoded with `kafka-wire` `each_record` (the walker compaction uses) |
| topic-record, topic-record-binary | key, value, headers, both timestamps, codec, producer id/epoch/sequence, transaction state, above or below LSO | exists (the batch header carries producer id, epoch, base sequence; `Txns` maps a producer id to its transactional id) | Fetch of one batch, DescribeTransactions |
| topic-produce, topic-produce-keyless, topic-produced | produce one record, the call beside the form, Kafka's refusal in its words | exists | Produce (v9); the refusal text is the handler's own (`produce/check.rs` `Refused.message`) |
| topic-create*, topic-created, topic-create-refused | CreateTopics with configs; the teaching lines per config; refusals | exists (teaching text: `topic_config/docs.rs` has Kafka's doc strings) | CreateTopics |
| topic-partitions, topic-add-partitions(-refused), topic-delete-records(-refused), topic-delete, topic-deleted | per-partition start, end, HW, LSO, size; the three admin calls | exists; size small | ListOffsets, CreatePartitions, DeleteRecords, DeleteTopics (the confirm names the groups that read the topic: OffsetFetch) |
| topic-offline | partition offline after a failed flush, with the cause | small (`PartitionLog::is_offline` and its cause string exist, not reachable) | view `partition_state()` |
| topic-wire, group-wire | the last requests that touched this topic or group | recorder | recorder filtered by topic or group |
| topic-config, config-edit, config-edit-list, config-edit-lag, config-reset, config-saved, config-refused, config-refused-value | value, source (dynamic topic, dynamic broker, static flag, default), default, type and range, Kafka's doc string, when a change takes effect | exists: DescribeConfigs with synonyms and docs (`configs/view.rs`, `topic_config/{table,docs}.rs`); "when it takes effect" is a static table of about 40 rows to write | DescribeConfigs (with synonyms and documentation), IncrementalAlterConfigs (Set, Delete, Append, Subtract) |
| groups-list, groups-none | state, protocol, members, lag | exists | ListGroups, DescribeGroups, ConsumerGroupDescribe, OffsetFetch, ListOffsets |
| group-detail, group-detail-848, group-members, member-detail, group-rebalancing | members, assignment, heartbeat age, leader, protocol and timeouts, who has not rejoined and the time left, 848 epoch and target vs assigned | exists for most; **small** for session and rebalance timeouts, last heartbeat, "joined" flag, the leader and the 848 target (all private to `GroupMachine`) | DescribeGroups, ConsumerGroupDescribe, view `group_detail()` |
| group-lag, lag-no-commits, lag-ahead, lag-expiring | log end minus committed, "no commits yet", committed past the end, offsets expire at `empty_since_ms` + retention | exists (`StoredOffset.committed_ms`, `GroupLife.empty_since_ms`) | OffsetFetch, ListOffsets; expiry from view `group_life()` (small) |
| group-reset-offsets(-refused), group-offset-delete, group-delete(-refused) | the four group writes, preview, refusal | exists | OffsetCommit (empty group, generation -1), OffsetDelete, DeleteGroups |
| group-story, story-rebalance, story-flapping | generations and members over time with the cause of each rebalance | **group event ring** (no history exists: `begin_rebalance` takes no cause; five call sites listed in GROUNDING 2.1) | view `group_events(group)` |
| live-wire, live-paused, live-quiet, live-load, request-detail, request-refused, request-fenced, request-too-large | one row per request: time, client id and software, API and version, topics, result, duration, folded repeats; a refusal with the rule, Kafka's code and words, what would pass | **recorder** + `Summarize` + error table | recorder `since(cursor)`, `get(id)` |
| live-clients | connections: client id, host, software, versions per API | **connection table** (new, owned by the server) | recorder `clients()` |
| topic-log, log-planner, log-retention, log-empty | segments (base, next, size, newest record, active), the ruler, planner verdict, last round, last retention action, tombstones with their date, durability | **small to medium**: `LogReader::segments()` (new), `compact::plan::judge` exposed, `CleanerState` per compacted partition and a last-retention record (both discarded today: `Cleaned` is logged at debug; `check_logs` drops the trim result) | ListOffsets; view `log_facts(topic, partition)` |
| topic-producers, txn-open, txn-holding | producers on a partition (id, epoch, last sequence, idle), the open transaction and what it holds back | exists | DescribeProducers, DescribeTransactions, ListTransactions, ListOffsets (LSO below HW) |
| settings-home, broker-config-edit(-refused/-saved) | the five dynamic broker configs, version profile (APIs and ranges), intervals, weight, data dir, flags | exists: DescribeConfigs (broker), IncrementalAlterConfigs (broker), `Profile` ranges, `Config`; flags and their defaults come from clap in the binary; memory and thread count need a platform read (see 7) | DescribeConfigs, IncrementalAlterConfigs |
| access-open | what is on and off: SASL mechanisms, ACL default and super users, TLS, who can connect, stored SCRAM user names, the console principal | exists (Phase 8 state): DescribeAcls, DescribeUserScramCredentials, `Config`; **the board is stale**, see 7 | DescribeAcls, DescribeUserScramCredentials |
| lab-*, live-lab-rule, log-compact-now (12) | fault rules in the request path | later, separate feature | none in Phase 9 |

## 2. What must exist first, in order

### 2.1 The in-process call (9a, small)

`Broker::handle(&self, &Listener, &Caller, &RequestHead, Request) -> Outcome` is already the single entry point the connection uses, and it takes the
`Caller` Phase 8 built. The console builds a `Request`, a `RequestHead { key, version, correlation_id, client_id: "console" }`, a loopback `Listener`
and a console `Caller`, and gets the same `Response` structs a client gets. Consequences: every read the protocol can express and every write is one
code path with the clients' (same checks, same order, same refusal words, same ACLs); no second implementation of "what does Metadata say". The
console's own requests never reach the connection loop, so the recorder leaves them out without a rule (the Go console had to add one).
Cost: building `kafka_protocol` request structs is verbose; 11 write APIs only (the boards use `data-op` of IncrementalAlterConfigs, OffsetCommit,
DeleteGroups, OffsetDelete, Fetch, CreatePartitions, CreateTopics, DeleteRecords, DeleteTopics, Produce, Metadata; counted by script) and about 12 read
APIs. Bound everything a console call asks for (Fetch `max_bytes`, `max_wait 0`).

### 2.2 `ConsoleView`, the read-only inspect surface (feature `inspect` of `kafka-broker`, grown per slice)

Read-only by construction (`&self`, returns plain owned structs, no handle to a writer), each method takes the console `Caller` and checks Describe on the
topic or group (so facts the protocol hides are no more visible than the protocol's own Describe). It holds only what the protocol cannot say:

| Method | Returns | Where it reads | Slice |
|---|---|---|---|
| `instance()` | started, last stop vouched or not, open partitions and the limit, descriptors | `Broker.started_ms`, `StopState`, `Partitions::open_count`, `doze-kafka/src/descriptors.rs` | 9a |
| `partition_files(topic)` | per partition: size on disk, last write, offline state and cause | `LogReader::segments()` (new; the reader already holds the arc-swap snapshot), `PartitionLog::is_offline` | 9b |
| `group_detail(group)` | leader, session and rebalance timeouts, per member last heartbeat and rejoined flag, 848 target vs assigned, `empty_since_ms` | `Groups::describe`, `GroupMachine` private fields, `GroupLife` | 9c |
| `group_events(group)` | the ring (2.4) | new | 9f |
| `log_facts(topic, partition)` | segments (base, next, size, newest, state), checkpoint, planner verdict (`judge`), `CleanerState`, last retention action, tombstone horizon, durability | `kafka-storage` `compact::plan::judge` (`pub fn` today inside a module that must be re-exported), new `CleanerState` and `RetentionNote` kept by `cleaner.rs` and `retention.rs` | 9g |
| `producers(topic, partition)` | the five-batch window per producer (the view returns only the last sequence today) | `Producers` | 9g |

Each accessor is a few dozen lines, but each touches a private type, so each is small only if done with the slice that needs it. They stay lazy: a call
builds its answer and nothing is kept. The idle cost is zero, which is what `weight.md` requires.

### 2.3 The request recorder (9e, the largest single piece)

**Where it hooks.** `kafka-server/src/connection/exchange.rs`, `answer()`: the request is decoded (`Incoming::Request { head, request }`), handed by value to
`ctx.broker.handle(..)`, and the `Outcome::Respond(response)` is written by `write()`. The recorder cannot be called with the request after `handle`
(it is consumed), so the observer is two-phase: `begin(&head, peer, &caller, &request) -> Pending` before `handle` (cheap, bounded: topics, group id,
transactional id, acks, counts, request bytes), and `end(pending, &response, elapsed, response_bytes)` after (per-partition results, error codes).
Close reasons (`End::*`) are reported by `serve` for the last row of a connection. With no observer installed the cost is one `Option` test per
request. The trait is `kafka_server::Observer`; the implementation lives in `kafka-console` (the dependency points console to server, not the other
way). SaslHandshake and SaslAuthenticate are answered by `sasl()`, not the broker, so they get their own two lines of observation (mechanism and
result only, never the bytes).

**What is captured per row** (mirrors Go `traffic.Event`, minus what has no source): sequence, connection id, first and last time and count (folded
repeats), peer address, `client.id`, client software name and version (from ApiVersions, which `kafka-wire` validates and drops today), principal
(Phase 8 `Caller`), API name, key and version, correlation id, topics (at most 20 and 1 KiB, the rest counted), group, member, generation,
transactional id, a clipped reason string (JoinGroup and LeaveGroup `reason`, 128 B), error name and code and class, closed-because, duration,
request and response bytes, and up to 16 per-partition results (topic, partition, base offset, records, bytes, error code). Never: record bytes, header
values, request bodies, anything secret.

**Ring and weight.** 2,000 rows, strings clipped (128 B name, 256 B text), a `VecDeque` that grows on demand (idle is zero; 2,000 rows are about
0.4 to 0.6 MB; add a byte budget of 1 MiB as well as the row count, because the Go edition once held 1.8 GiB for a 900 KiB join reason, TRF-1). Folding:
repeats of the same (connection, API, topic, result) within 10 minutes become one row with a count and up to 64 ticks, so heartbeats and idle fetches
do not push the interesting rows out. Honest window: the screens say "about the last 3 h"; a ring of 2,000 rows covers whatever it covers, so the page
states its real span ("the last 2,000 requests, since 22:09"), computed from the oldest row.

**`Summarize`.** A trait with `of_request(&Request) -> Facts` and `of_response(&Response, &mut Facts)`; one small module per shape, about 40 served APIs:
produce (acks, partition results), fetch (partitions, offsets, bytes, wait, isolation), metadata and topic admin (CreateTopics, DeleteTopics,
CreatePartitions, DeleteRecords), offsets (OffsetCommit, OffsetFetch, OffsetDelete, ListOffsets), classic group (FindCoordinator, JoinGroup, SyncGroup,
Heartbeat, LeaveGroup, DescribeGroups, ListGroups, DeleteGroups), KIP-848 (ConsumerGroupHeartbeat, ConsumerGroupDescribe), transactions
(InitProducerId, AddPartitionsToTxn, AddOffsetsToTxn, EndTxn, TxnOffsetCommit, DescribeProducers, DescribeTransactions, ListTransactions), configs
(DescribeConfigs, AlterConfigs, IncrementalAlterConfigs), security (DescribeAcls, CreateAcls, DeleteAcls, the two SCRAM credential APIs). Files:
`recorder/summary/{produce,fetch,topics,offsets,groups,txn,configs,security}.rs`, each under 200 lines, snapshot-tested with golden files.
The Go equivalent was about 2,100 lines of reflection; a typed match over the generated `Request` enum has no wildcard, so a new API without a
summary does not compile (the plan's exhaustive-match rule applied to the observer).

**The error table.** `console/src/errors/{table,classes}.rs`, one row per error code the broker returns (61 distinct `ResponseError` identifiers appear in
`kafka-broker`, `kafka-server`, `kafka-wire` today; the Go table has 65): name, code, class (`expected`, `refused`, `denied`, `not implemented`,
`failed`), usual cause, what would pass. The Go edition enforces completeness with a test that walks the broker's source for error names; do the same
(a test reads `kafka-broker/src/**/*.rs` through `CARGO_MANIFEST_DIR` and fails on a name with no row). "Refused" (Kafka would too) is violet, "Denied"
(an ACL or a login) amber, per the direction; Phase 8 makes Denied real, so Live gets the amber class that the boards deliberately left out.

**Secret redaction.** The recorder is the new place a secret could leak, so it joins the Phase 8 redaction gate: no field is built from
SaslAuthenticate's `auth_bytes`, SCRAM nonces or proofs, or `AlterUserScramCredentials` salted passwords and salts (user name and mechanism only);
a config value whose name looks like a secret (`password`, `secret`, `key`, `sasl.jaas.config`, `ssl.*`) is hidden in IncrementalAlterConfigs rows; the
planted-password test of `kafka-auth` is extended to run a login and a credential change with the recorder and the console on, then search every
row, every `Debug` output, the log, the rendered HTML and the event stream for the planted value.

**Connection table** (live-clients): the same observer sees accept and close; keep per connection (id, peer, opened, last request, client id,
software, per-API-version counts, request and error counters). At most `max_connections` entries.

### 2.4 The group event ring (9f)

`GroupMachine` is sans-IO (`step(Event, now) -> Vec<Effect>`), so the idiomatic place is a new `Effect::Note { kind, cause, member }` that the actor
pushes into a per-group ring (200 events per group, 100 groups, the Go bounds). The cause is an argument that `begin_rebalance` lacks today: new
member (`join.rs:176-177`), rejoin with changed protocols (`join.rs:223`), member left or session expired or rebalance timeout
(`rebalance.rs:197` via `leave.rs:39` and the timer paths), migration (`migrate.rs:110`), topic change; for 848 the epoch bumps in
`consumer/{join,heartbeat,reconcile}.rs`. The client's own `reason` on JoinGroup v5+ and LeaveGroup is on the wire and unread; carry it, clipped. The
gate for the slice is that no state-machine behaviour changes: all Phase 4 and 5 oracle scenarios and property tests must be byte-identical (the
note is an extra effect, not a different transition).

### 2.5 Small ones

The advertised-listener probe (raw ApiVersions, 9a); process facts for Home and Settings (`/proc/self/statm` and thread count on Linux; on macOS
there is no safe, dependency-free way to read the resident size under `forbid(unsafe_code)`, so the card says "not measured on this platform" or spawns
`ps`; the boards draw 4.5 MB and 3 threads); a `Facts` struct built by the binary from clap (flags with value, default and whether set: the 24-flag
table of Settings).

### 2.6 Protocol leftovers that the docs assign to Phase 9 (not console work)

`debt.md` I17 and `oracle.md`: DescribeCluster (v0 to v2) and the KIP-714 telemetry pair (GetTelemetrySubscriptions, PushTelemetry) are what the 11
remaining replay failures are (the ten `telemetry-*` scenarios and `version-matrix`). The Go edition's answer is simple (a fixed subscription of
`org.apache.kafka.` metrics every 5 s, payloads kept in memory, 1 MiB cap, a subscription id that is a CRC of the instance id so it survives a
restart; `internal/broker/telemetry.go`, 148 lines). No board shows client metrics, so the Rust edition only needs the handshake and a bounded store
if a later screen wants it. Estimated 1 to 1.5 sessions; independent of the UI.

## 3. The Go console's routes: kept, mapped, dropped

`console/routes.go` has 64 rows (41 GET, 23 POST, 5 of them read-only previews). Mapping to the boards (Rust paths are proposals; every link is built
from a `Base` so a prefix works):

| Go route group | Rust |
|---|---|
| `/` Traffic is home; first-run page | `/` is **Home** (new); the wire is `/live` |
| `/traffic`, `/live` partials, `/traffic/request`, `/traffic/new`, `/api/counts` | `/live`, `/live/rows` (fragment), `/live/{id}` (drawer), `/live/clients`; the "N held back" counter is client-side while paused; counts come with each fragment |
| `/topics`, `/live/topics`, `/new/topic`, `/new/topic/shell` | `/topics` (fragment on tick), `/topics/new` with steps config and review; the shell command is the "call beside it" panel |
| `/topics/{name}` (+ tape, new, message, message/raw) | `/topics/{name}` Messages, `/topics/{name}/partitions`, `/log`, `/config`, `/consumers`, `/producers`; `/topics/{name}/records/{p}/{offset}` drawer; "raw bytes download" is **not drawn**: dropped unless added to the board |
| delete, purge, partitions, config (+ `/config/{key}` preview and set) | POST delete, POST records/delete (DeleteRecords to an offset; "purge" is the same dialog), POST partitions, POST config with a GET preview fragment (Before, After, when it takes effect) |
| produce, compose/preview, resend | POST produce; the preview is the "call beside it" fragment; **resend is dropped** (no board) |
| `/groups`, `/groups/{id}` (+ live), reset (+ preview), offset set and delete, delete | `/groups`, `/groups/{id}/members|lag|story`, POST reset with preview, POST offsets/delete, POST delete; "set one offset" is the reset dialog's "to offset" |
| `/trace` (2), schemas (14) | trace is out of the design (owner, 2026-11-10); schemas are Phase 10 |
| `/cluster`, `/connect` | `/connect`; cluster is the This instance card on Home plus Settings |
| `/access` (stored ACLs, checker, create ACL, preview) | `/access`, one read-only page (owner decision): the checker and create ACL are dropped |
| `/inspect/{kind}`, `/dock`, `/confirm/{id}`, `/api/palette`, `/api/list/{section}` | drawers are routes (above); the dock is the wire fragment filtered by topic or group; a confirm's sentence is measured server-side when the dialog is opened; `/api/palette` stays as `/palette.json` (jump to a topic or group, the board's search box) |
| `/static/` | `/static/{*path}` from the embedded manifest |
| (new) | `/events` the one stream; `/healthz` (PLAN keeps it) |

About 45 route rows after the mapping. The route table is one `const` array in `kafka-console/src/routes.rs` (path, method, mutates, `data-op`, the
screen ids it serves); the router is built from it, and it is exported as JSON for the board-diff gate (this is what survives of PLAN's "routes
exported as JSON for the e2e route gate").

## 4. The design system, as the canvas defines it

Tokens (`kit/paper.css` `.root`): `--bg #fbfaff`, `--side #f3f1fa`, `--panel #fff`, `--line #e4e0f0`, `--line2 #d4cde6`, `--well #f6f4fb`, `--text #3a3550`,
`--dim #6f6888`, `--bright #17141f`, `--accent #7a4fe0`, `--good #17773f`, `--warn #9a6400`, `--cool #2f62d9`, `--bad #cc3550`, `--refused #a3279a`, `--sel #ece6fb`,
`--pick #e3d9ff`, `--track #ece8f5`; domain hues `--d-msg/--d-data/--d-compute/--d-config/--d-observe/--d-infra`. Type: Inter (UI) and JetBrains Mono (names,
offsets, times), 14 px body, h1 36 px weight 300. Light theme only. Sizes: `paper.css` 20 KB (260 lines, 79 distinct class names), `kafka.css` 7.4 KB.

How components map to maud (each a function returning `Markup`, one file per family under 300 lines):

| Canvas classes | maud component (module) |
|---|---|
| top bar `.topbar .brand .search .chip .tgl`, rail `.rail .ni .pin .hints`, `.main .crumbs` | `shell::{topbar, rail, page}`; the pills (addresses, version profile, "Open") are data, not literals |
| `.page-h h1 .sub .acts`, `.tabs .tab .n`, `.sec`, `.card`, `.g2/.g3/.g4`, `.split`, `.row` | `layout::{page_header, tabs, section, card, grid}` |
| `.btn .pri .danger .sm .ghost`, `.pill`, `.tag`, `.st ok|warn|bad|refused|info|idle`, `.rw ok|refused|warn|bad`, `.new` | `ui::{button, link_button, pill, tag, state, result_word}` with enums for variants so a misspelt class does not compile |
| `.kv`, `table.t` (`.wire`, `.r`), `.toolbar .finput .fchip .seg .pager` | `table::{Table, Column}`, `kv::list`, `filter::{input, chip, segmented, pager}` |
| `.callout`, `.banner`, `.empty`, `.code`, `.sent`, `.apicall`, `.wire-dock`, `.fold` | `notice::{callout, banner, empty}`, `code::{block, apicall}`, `wire::{dock, row, folded}` |
| forms: `.form .field .input .help .teach .opt-row .radio .check .choice .toggle .actions-bar .steps` | `form::{field, teach, choice, steps}` |
| Kafka's own: `.ruler .mks .segs .sg .gp .axis .key`, `.lagbar`, `.story .ev .cause`, `.rules .r`, `.recv .hdrs` | `kafka::{ruler, lagbar, story, rules, record_view, headers}` |
| `.tlmap`, `.rule` | not built (the map is dropped; Lab is later) |

Rules to carry into the Rust: positions are `--at:62%` custom properties on elements (the canvas forbids `position:absolute` inline), so the real pages
need a CSP with `style-src 'self' 'unsafe-inline'` or the ruler and lag bars need a nonce; `.root` is a fixed 1440 px frame in the canvas, so the
real stylesheet needs a fluid shell and a narrow-width rule (the Go suite had a `narrow` spec; no board draws a narrow page); `htmx` attributes become typed
`const` ids shared by markup and tests (the plan's rule); every page that is a fragment target has a stable id.

Assets and what they weigh (bytes of the real files; `weight/assets/` holds the Go edition's set used in the spike):

| Asset | Go edition | New design |
|---|---|---|
| htmx 2.0.11 | 52,182 | 52,182 (the SSE feature is a separate htmx extension, about 10 KB, or about 40 lines of own JS over `EventSource`; recommended) |
| Alpine 3.17.4 | 55,891 | not needed if dialogs use `<dialog>` and copy/toggle are 100 lines of own JS |
| CodeMirror 6 | 359,084 | not needed (the boards use plain `.code` blocks and `<textarea>`) |
| `app.css` | 123,444 | `paper.css` + `kafka.css` + a fluid shell, about 30 KB |
| fonts | 4 woff2 IBM Plex, 90,928 | Inter and JetBrains Mono Latin-subset woff2 (OFL), about 90 to 100 KB (not measured: the kit declares no `@font-face`; the canvas relied on installed fonts) |
| own JS (shell, keys, copy, SSE, palette) | `shell.js` 115,747 | about 10 to 15 KB |
| total | about 0.83 MB (with CM6) | about 190 to 200 KB |

The spike embedded the 7 Go files (322,445 bytes) so the weight numbers are an upper bound for assets.

## 5. What cannot be built as drawn

1. **Doze names.** Every board's top bar shows `kafka.larder.doze:9092` with a `via doze` chip and a `larder` workspace label; the owner decided IP and port
   only, no doze names (PLAN, decision log). The address pill, `connect-standalone`'s wording and `world.md` must lose them. The board-diff allow-list
   cannot paper over a wrong top bar on 94 boards: fix the kit's `chrome` once and regenerate.
2. **The top bar pill "Open · no SASL yet" and `access-open`** are stale: Phase 8 built SASL PLAIN, SCRAM, ACLs, TLS (feature), the credential APIs and
   the guard. The Access page (still one page, by decision) must show what is on (mechanisms, ACL default and super users, TLS, users by name, the
   console's own principal) and the "The broker will not pretend" card quotes refusals ("SASL is not supported yet in this edition") that no longer
   exist. Needs a redraw tagged `new`; debt I20 says the same.
3. **The window.** `live-wire` says "about the last 3 hours", `live-quiet` "nothing in the last 3 h". A 2,000-row ring cannot promise a span; the text
   must be computed (see 2.3).
4. **"Written by" and "read by" sentences** on the topics list and topic page: "written by" is recorder-only, so before 9e the sentence is "read by"
   only; the board is complete only at 9e. Likewise Home's "9 apps spoke in the last 3 h" and `home`'s first-request strip.
5. **Pinned items in the rail** (`orders`, `fulfilment`): no store holds them. Keep them client-side (`localStorage`) or drop them; no server state.
6. **Idle memory and threads on Home and Settings** (4.5 MB, 3 threads): readable on Linux, not on macOS without `unsafe` or a spawn; say "not measured"
   there.
7. **"Reached from" lines and "Other moments of this page" link rows** are canvas navigation scaffolding (`.from`, the kit's `via` chips), not product.
   The board-diff gate must ignore them by rule, and the real pages must not ship them.
8. **Dialogs and drawers** are drawn as overlays over a dimmed parent. Real implementation: each is a route returning the dialog body, opened by htmx
   into a `<dialog>`; they must also work as a full page (a direct URL, no JavaScript), since the parent is a fixed route. Board-diff compares them on
   their own content.
9. **`data-op="doze:..."` and every `lab` board** (12) are not buildable until fault rules exist in the request path.
10. **Record trace, the map, the lag trend** are gone (owner). `lag-behind` and `group-lag` keep only current lag; the footer says "The broker keeps no
    lag history".
11. **The shell's `⌘K` palette** is drawn as a search box ("Jump to a topic or group, or do something"); "do something" has no catalogue on the boards.
    Build "jump to" only (topics and groups from Metadata and ListGroups); the actions part is not grounded.
12. **Fixed 1440 px frame, no narrow layout, no dark or high-contrast theme** (the Go console had four themes; the boards are one). Build one theme.
13. **A KIP-848 group's "target vs assigned"** exists only if `ConsumerView` is exposed (small); a classic group's "has not rejoined" only if the machine's `joined`
    flag is exposed. Both are in `group_detail()`.
14. **`topic-wire` and `group-wire`** show "what touched this topic". Requests the console itself makes are not recorded (they bypass the connection), so
    the dock can look quieter than the Go edition's did; the page must not claim it shows everything.

## 6. Security facts the plan must carry (found while grounding)

- **Cross-site requests.** The allowed-hosts guard stops DNS rebinding (a `Host` that is not an IP literal or `localhost`). It does not stop a web page in
  another tab posting a form to `http://127.0.0.1:9093/topics/orders/delete`: the `Host` is correct, a form post needs no preflight, and the console has no
  login. Every non-GET must pass an origin check (`Origin`, or `Sec-Fetch-Site: same-origin`, must match the request's `Host`; refuse otherwise with 403)
  and no GET may change anything (the stream included). A pure function in `kafka-console/src/origin.rs`, tested and fuzzed like `hostguard`.
- **Record values are hostile input.** Message keys, values and headers come from any client; the console shows them. maud escapes everything by
  default; the rule is that no component takes pre-rendered HTML and the JSON and hex views are built from tokens in Rust. A test renders a value of
  `<script>` and `"><img onerror>` in every place a record appears. Send `Content-Security-Policy: default-src 'self'; frame-ancestors 'none'`,
  `X-Content-Type-Options: nosniff`, `Referrer-Policy: same-origin` (was no-referrer until the pages pass: the unbuilt page goes back to the page the visitor came from; no other origin is ever told).
- **The console reads and writes the broker as a super user** when SASL is on (the Go edition's `__console` login; SEC-188's text says so). That is why the
  start-up rule refuses a console beyond loopback with SASL or TLS on unless `--console-unprotected`. Phase 9 mounts the guard, answers 403 with the
  guard's decision, and reports the exposure on the Access page.
- Body limits (forms: 1 MiB, the produce value cap), header limits (hyper's defaults), no cookies, no sessions.

## 7. Weight spike (measured)

Method. Scratch workspace `weight/` (never in a repo), release profile copied from `doze-rs/Cargo.toml` (thin LTO, one codegen unit, strip, abort),
darwin/arm64, rustc 1.99.1, `nice -n 19`, `-j 3`, one build at a time. Seven binaries from three packages: `base` (a tokio echo server on the
features `doze-kafka` uses: rt, rt-multi-thread, signal, net, time, macros, io-util, sync, plus bytes, tokio-util, futures-util, tracing, serde,
serde_json, tokio-stream so that the real tree's shared crates are already in), `hand` (a hand-rolled HTTP/1.1 server on tokio alone: request line and
headers capped at 16 KiB, bounded body, keep-alive, Content-Length answers, one SSE stream, Host guard, form decoding, static files, 4 pages), and
`web-axum` (the same pages and routes on axum 0.8 with path parameters, a `Form` extractor, a Host-guard layer, SSE and static files). All serve the
same 4 pages, a form POST, `/static/*` (7 real files, 322,445 bytes) and `/events`. Pages are maud (feature) or `format!`. Assets are `include_bytes!`
(the default) or rust-embed 8.14 or include_dir 0.7. Files: `weight/{base,hand,web-axum,shared}`, `build-all.sh`, `measure.ts`, `results/`.

| Binary | Bytes | vs base | `cargo tree` lines (the CI gate's unit) | New distinct packages vs the real doze-kafka tree | Idle RSS | RSS after 300 requests, one open stream |
|---|---|---|---|---|---|---|
| base (tokio echo) | 499,872 | 0 | 39 | 0 | 2,304 KB | 2,304 KB (no load) |
| hand (HTTP by hand, assets by `include_bytes!`) | 846,960 | +347,088 (322,445 are the assets, so about 24.6 KB of server, routing, pages) | 39 | 0 | 2,320 KB | 2,768 KB |
| hand + maud | 847,152 | +192 vs hand | 43 (+4) | 3 (`maud`, `maud_macros`, `proc-macro2-diagnostics`; `syn` and `itoa` are already in the tree) | 2,320 KB | 2,720 KB |
| axum | 1,156,416 | +309,456 vs hand | 67 (+28) | 22 | 2,592 KB | 3,520 KB |
| axum + maud | 1,156,464 | +48 vs axum | 71 (+32) | 25 | 2,592 KB | 3,472 KB |
| axum + maud + rust-embed | 1,173,008 | +16,544 vs axum + maud | 90 (+19) | 33 (+8: `mime_guess`, `unicase`, `walkdir`, `same-file`, `rust-embed` x3, `const-oid`) | 2,608 KB | 3,680 KB |
| axum + maud + include_dir | 1,173,024 | +16,560 vs axum + maud | 73 (+2) | 27 (+2: `include_dir`, `include_dir_macros`) | 2,608 KB | 3,472 KB |

The 22 packages of axum over the real tree: `atomic-waker`, `axum`, `axum-core`, `form_urlencoded`, `http`, `http-body`, `http-body-util`, `httparse`, `httpdate`,
`hyper`, `hyper-util`, `matchit`, `mime`, `percent-encoding`, `ryu`, `serde_path_to_error`, `serde_urlencoded`, `smallvec`, `sync_wrapper`, `tower`, `tower-layer`,
`tower-service`. Already in the tree and so free: `tokio`, `bytes`, `tokio-util`, `tokio-stream`, `futures-util`, `tracing`, `serde`, `serde_json`, `itoa`,
`memchr`, `pin-project-lite`, `mio`, `socket2`, `syn`, `proc-macro2`, `quote`. Not in the tree: `hyper`, `http`, `tower`, `base64` (the plan's list was
optimistic on three of the four). The `form` feature accounts for about 3 of the 22 (`serde_urlencoded`, `form_urlencoded` and `serde_path_to_error`; not
measured separately); `tower-http` is not needed (assets are served by 15 lines).

Behaviour checked by the harness: a `Host: evil.example` request got 403 and a missing asset 404 on all six web binaries; 300 requests in 21 to 37 ms.

cargo-deny, run in the scratch workspace against the repo's `deny.toml` unchanged (`all-features`): **advisories ok, bans ok (one duplicate warning: `syn`
2 and 3, a warning by the file's own rule), sources ok, licenses ok for every third-party crate** (MIT, Apache-2.0, BSD-3-Clause for `matchit`'s
`MIT AND BSD-3-Clause`, Unicode-3.0; the only "failures" are my three scratch crates having no `license` field). So the stack is acceptable as it stands.

Caveats. Darwin deltas; the CI limits are Linux x86_64, where the binary has measured about 1.2 times the darwin size (4.90 MB against 4.03 MB at the
close of Phase 7), so scale code deltas by 1.2 and leave asset bytes as they are. The four spike pages are far smaller than 94 screens: the maud macro
expands to inline string pushes, so its cost per page is real code, not a runtime; an estimate for 60 distinct page functions at 1 to 3 KB each is
100 to 200 KB, plus about 80 KB for recorder, summaries and the error table. These two are estimates, not measurements, and 9a measures the first
real figure.

**What the console would cost in the real build (darwin; Linux in brackets):** axum, hyper, tower, http: +309 KB (+370 KB); maud: about nothing in
runtime, the pages' own code 100 to 200 KB; recorder and views about 80 KB; assets about 200 KB for the new design (322 KB if the Go set were copied);
so about **0.7 to 0.8 MB (Linux 0.8 to 0.95 MB)** and **+25 distinct packages, +32 lines in the gate's own count**, idle RSS **+0.3 MB** and about
+1.2 MB over the base after serving pages and holding a stream. The Linux binary after Phase 8 is about 5.137 MB against the 5.15 MB limit (`weight.md`'s own estimate; the CI has not measured it, "0.25% under the limit"), and the
package count is 122 on Linux against 126 (the `weight-tls` job, limit 6.1 MB and 137 packages, is the pattern to copy), so **the console cannot be inside today's gates at all**; it needs its own gate (about 6.0 MB, about 156
lines, idle under 8 MB: the 8 MB limit still holds, 4.96 + 0.3 = 5.3 MB idle).

**Hand-rolled versus axum.** The hand-rolled server saves about 0.3 MB and 22 packages and costs about 700 lines of HTTP (request parser, header and
body limits, keep-alive, HEAD and OPTIONS, `Expect: 100-continue`, chunked bodies, percent decoding, SSE framing) that must be fuzzed and owned
(`PLAN.md`'s rule is that wire input is never trusted; hyper has years of that). The Phase 10 registry needs an HTTP server too, so the stack is paid once.
Recommendation in `phase9.md`: axum 0.8 with `default-features = false` (features `tokio`, `http1`, and `form` only if a hand-written decoder is not
preferred) and maud, assets by an `include_bytes!` manifest.

Spike left behind: `weight/results/{summary.txt,rss.txt,*.tree,real-tree.txt}`; the target directory and binaries are deleted at the end.

## 8. What the Playwright suite was, so the board-diff gate can replace it deliberately

34 specs (a11y, a11y-traffic, access, access-create, compose, console-errors, design, disconnected, dock, editor, groups, groups-boards, harbour, inspector, keyboard,
keys, live, long-names, message-inspector, messages, narrow, routes, schemas, settings, shell, smoke, strip, time, topics, topics-boards, topics-boards2,
trace, traffic, traffic-boards) booting the real binary with a seeded broker, a route gate reading `Routes()` JSON, and a `design` gate that rendered each
board and the live page and diffed them element by element with an allow-list (`e2e/design/README.md`). The Rust gate keeps the three ideas that mattered
(route table is the one list, boards are the contract, an allow-list with reasons) and drops the browser from CI. Things the suite covered that the new
gate must still cover by other means: keyboard (j/k, ⌘K) and focus order (accessibility lint plus one manual page), console errors (the real page loads
with no 4xx or 5xx for its own assets: a route test fetches every `href` and `src` of every rendered page), long names (a golden fixture with a 249-byte
topic name), disconnected state (the stream drops: a banner; a unit test of the client script is not planned, so one manual check), narrow layout
(no board; not built).
