# Client interop

Real Kafka clients against the broker: sarama, franz-go and kafka-go (Go libraries, in `go/`), and librdkafka
through `kcat` (a process of its own, which is also what lets one be killed without warning). Needs `go` and `kcat`.

    interop/run.sh                       # builds the release broker, starts it on a free port, runs everything
    interop/run.sh -only churn           # one scenario by name
    FRANZ_LOG=1 interop/run.sh -only franz-go    # franz-go's debug log (its join, balance and sync steps)

    interop/run.sh -sasl                 the SASL cells (below) against a broker started with SASL on
    interop/run.sh -tls                  the TLS cells (below): the broker built with --features tls, over SASL_SSL and SSL

    interop/run.sh -kafka-version 2      the broker stands for Kafka 2 (--kafka-version 2), the clients set up for it; with -only, -sasl, -tls
    interop/run.sh -versions             every major, 1 to 4, a broker and a fresh data directory each, then a summary per major
    interop/run.sh -sasl -versions       the SASL cells at every major

    interop/run.sh -java                 the official Java client (kafka-clients 4.4.0) instead of the Go clients; with -only,
                                         -sasl, -tls, -kafka-version

The Go clients alone run by default. `-sasl`, `-tls` and `-versions` run the Java client after them (see "The Java client").

Against a broker already running: `cd interop/go && go run . -broker 127.0.0.1:9092` (add `-kafka-major N` if it was started
with `--kafka-version N`); the Java client: `cd interop && java -cp "$(bun java/fetch.ts 4)" java/Interop.java -broker 127.0.0.1:9092`.

## Kafka majors (`-kafka-version`, `-versions`)

`--kafka-version N` makes the broker offer each API at the versions a real broker of Kafka N offered (`crates/kafka-wire/src/profile.rs`).
The suite runs against each major as a user of that major would: the Go program is told the major (`-kafka-major N`), and the
broker decides what runs. At the start it asks the broker for ApiVersions (franz-go, `interop/go/versions.go`) and prints the list;
a scenario that needs an API the broker does not advertise is printed as `skip <name> Kafka N does not offer <API>` and not run.
No table in the suite says which major has what.

What each library needs for an old broker:

- **sarama** does not negotiate request versions from ApiVersions for everything; it picks them from `cfg.Version`, which must
  not claim a newer broker than the one it talks to. One helper, `saramaVersion()` in `versions.go`, sets it from the major:
  1 → `V1_1_0_0`, 2 → `V2_8_0_0`, 3 → `V3_9_0_0`, 4 → `V4_0_0_0` (the last release of each major, which are the brokers the
  version table was recorded from). Every sarama client of the suite (consumers, producers, the admin, the transactional
  producer, the SASL and TLS cells) takes its configuration from `saramaConfig()`.
- **franz-go** negotiates every request from ApiVersions; nothing to set.
- **kafka-go** negotiates from ApiVersions on each connection; nothing to set.
- **librdkafka** (kcat and `txnprod`) negotiates too: `api.version.request` is on by default, and every broker from 0.10 on answers
  ApiVersions, so `broker.version.fallback` (which applies only when ApiVersions is off or fails) never matters here. Nothing to set.

Each major as run on 2026-11-14 (`interop/run.sh -versions` and `interop/run.sh -sasl -versions`). No broker defect was found. The one
failure was the harness's: the leader of `mixed` (see "What the clients taught"). A skip names the API the broker did not advertise:

| | Kafka 1 | Kafka 2 | Kafka 3 | Kafka 4 |
|---|---|---|---|---|
| sarama, franz-go, kafka-go, librdkafka: a group of two of each | pass | pass | pass | pass |
| franz-go-848, librdkafka-848 (two of each) | skip (a) | skip (a) | skip (a) | pass |
| franz-sarama, franz-kafka-go, sarama-franz, librdkafka-franz, franz-librdkafka, mixed, mixed-rr | pass | pass | pass | pass |
| upgrade-848, classic-joins-848, churn-848 | skip (a) | skip (a) | skip (a) | pass |
| churn | pass | pass | pass | pass |
| eos-franz-go, fence-franz-go, txn-franz-go, txn-sarama, txn-librdkafka | pass | pass | pass | pass |
| compaction | pass | pass | pass | pass |
| **plain: passed / skipped / failed** | **18 / 5 / 0** | **18 / 5 / 0** | **18 / 5 / 0** | **23 / 0 / 0** |
| SASL cells: 4 libraries × PLAIN, SCRAM-SHA-256, SCRAM-SHA-512 | 12 pass | 12 pass | 12 pass | 12 pass |
| SASL users made over the API (14 lines) | skip (b) | pass | pass | pass |
| **SASL: passed / skipped / failed** | **12 / 1 / 0** | **26 / 0 / 0** | **26 / 0 / 0** | **26 / 0 / 0** |

(a) `ConsumerGroupHeartbeat` is not advertised. Kafka 1 and 2 have no such API. The Kafka 3.9.1 broker the table was recorded from
lists it (v0), but KIP-848 was early access in 3.x: `group.coordinator.rebalance.protocols` defaults to `classic`
(`GroupCoordinatorConfig` in 3.9.1), so a 3.9 broker as installed does not run consumer-protocol groups. `--kafka-version 3`
leaves it out (`profile(3).offers_consumer_groups()` is false, pinned in `profile/tests.rs`).
(b) `DescribeUserScramCredentials` and `AlterUserScramCredentials` came with Kafka 2.7 (KIP-554); the 1.1 broker does not list them.
SCRAM itself is from 0.10.2, so the 12 SCRAM and PLAIN cells run at Kafka 1.

Transactions run at every major (they arrived in 0.11): at Kafka 1, every library negotiates InitProducerId, AddPartitionsToTxn,
AddOffsetsToTxn, EndTxn and TxnOffsetCommit at v0, and sarama (`V1_1_0_0`) uses its oldest transactional requests. The three ACL APIs
are not offered at Kafka 1 (the protocol crate dropped their only version there, KIP-896), but no scenario uses ACLs, so nothing is
skipped for that. TLS is transport: `-tls` accepts `-kafka-version` (both brokers start with it), and is run at the default, 4.

## The scenarios

Every one is on a fresh topic of four partitions made through sarama's admin client.

**Groups.** Two members of one library (four scenarios); two libraries with each as the first joiner, and so the
leader; all four together, with the `range` assignor and again with `round-robin`. Every member joins, and the test
waits until sarama's `DescribeConsumerGroups` says Stable with all of them; then 300 keyed records are produced and
read until all have been seen, and the members leave (which commits). Every member must have read something. Then
50 more are produced and a new consumer of the same group must read exactly those 50.

**churn.** All four libraries in one group while 2000 records are produced over ten seconds. franz-go leaves; a
librdkafka member is killed with SIGKILL, so the broker is told nothing and its seat must go when its 6 s session runs
out (checked: between 4 s and 7.5 s later); a new sarama member joins. Every record must still be read (some twice:
what the killed member read and had not committed), and at the end every partition's committed offset must be its end.

## SASL (`-sasl`)

A mode of its own, so the scenarios above stay as they are (they run with no login). `run.sh -sasl` starts the broker with
`--sasl-mechanisms PLAIN,SCRAM-SHA-512,SCRAM-SHA-256 --sasl-user alice:alice-secret-pw` (`SASL_PASSWORD` changes the password) and
runs one cell for each library and mechanism: sarama, franz-go, kafka-go and librdkafka (kcat) with PLAIN, SCRAM-SHA-256 and
SCRAM-SHA-512, twelve in all (and the users step below). A cell makes a topic (franz-go over PLAIN), writes 20 keyed records and reads them back with the
library's own producer and consumer, then logs in with a wrong password, which must fail with an authentication error within
15 s (the output prints the words each library gave). sarama gets its SCRAM client from `xdg-go/scram`, as its documentation
says; the others have SCRAM built in or in their own `sasl` packages. `interop/go/sasl.go`.

**Users made over the API (`interop/go/users.go`, the second half of `-sasl`).** Fourteen more lines. franz-go's `kadm` makes a user, `dave`, with `AlterUserScramCredentials` (SCRAM-SHA-256 at 4096 iterations, SCRAM-SHA-512 at 8192; kadm
salts the password and runs PBKDF2 itself, in Go) through a login as `alice`, who is configured at start; `DescribeUserScramCredentials` lists both users; an attempt to take over `alice` is refused (93). Then the eight cells above run again, as
`dave` with SCRAM-SHA-256 and SCRAM-SHA-512 (a wrong password refused). Then: PLAIN with the right password is refused by every library (a stored SCRAM verifier cannot give a password back); `dave`'s SCRAM-SHA-512 credential is deleted and
every library is refused with it while SCRAM-SHA-256 still logs in; the SCRAM-SHA-256 one is deleted and every library is refused; a second deletion is `RESOURCE_NOT_FOUND` and `dave` is no longer described. The broker of `-sasl` has no ACL, so any
logged-in user may manage users (the open-broker posture, SEC-D51 in `docs/port/behaviours/security.md`). Needs `github.com/twmb/franz-go/pkg/kadm` (in `go.mod`).

## TLS (`-tls`)

Another mode of its own (needs `openssl`; LibreSSL, which macOS ships, works). `run.sh -tls` builds the broker with `--features tls` (a C compiler, for ring), makes a
throwaway CA and a server certificate for `localhost` and `127.0.0.1` (the two-step `ecparam` key and `-sha256`, because LibreSSL's one-line forms write a key
or a signature that rustls refuses), and starts two brokers: one with the certificate and `--sasl-mechanisms SCRAM-SHA-512` (SASL_SSL), one with the certificate alone (SSL).
Each of sarama, franz-go, kafka-go and librdkafka (kcat) trusts that CA and nothing else, and on each listener writes 20 keyed records and reads them back. Then the
refusal that listener allows: a wrong password on SASL_SSL (an authentication error, within 15 s), and on SSL a client that does not have the CA, which must fail as a
certificate failure. Eight cells. `interop/go/tls.go` (the cells are the ones of `sasl.go`, with `secure` set).

## The Java client (`-java`, and after the Go clients under `-sasl`, `-tls`, `-versions`)

The official client, Apache `kafka-clients`: the reference implementation, and what most users run. `interop/java/Interop.java`
is one file run by the JDK's source launcher (`java -cp <jars> Interop.java`, no Maven or Gradle), with the same contract as the
Go program: one `ok`, `skip` (with the reason) or `FAIL` line per scenario, titled `java-<version>/<scenario>`, so the per-major
summary of `-versions` counts it. `-only <scenario>` picks one.

- **JDK**: Temurin 21.0.13+9 (LTS), pinned in `interop/.prototools` (`java = "temurin-21.0.13+9"`); `cd interop && proto install`.
  `run.sh` runs `java` from `interop/`, so proto's shim picks that JDK (the macOS `/usr/bin/java` is only a stub).
- **Jars**: `bun interop/java/fetch.ts 4` (or `2.8`) downloads them from Maven Central into `~/.cache/doze-interop/java/`
  (`$DOZE_INTEROP_CACHE/java`), checks each against the SHA-256 pinned in the script, and prints the classpath. Two lines:
  kafka-clients **4.4.0** (latest stable) with zstd-jni 1.5.7-17, lz4-java 1.11.2 (`at.yawk.lz4`), snappy-java 1.1.10.7; and
  **2.8.2** (what users of Kafka 1 and 2 era brokers run) with zstd-jni 1.4.9-1 and lz4-java 1.7.1 (`org.lz4`), as its POM
  names, but snappy-java 1.1.10.7 instead of 1.1.8.1, which has no native library for Apple Silicon and no fallback. Both
  with slf4j-api and slf4j-simple 1.7.36 (`JAVA_LOG=debug` for the client's log; `JAVA_TRACE=1` for a failure's stack).
- **Which client where**: `JAVA_CLIENTS` names the lines (default `4`); `-versions` runs `4 2.8`. The 4.4 client runs at every
  major (it negotiates); 2.8 runs at Kafka 1 and 2 and prints one skip line at 3 and 4. The file compiles against both, so
  what one lacks is reached by name (describeTopics' answer, `OffsetSpec.maxTimestamp`, `SubscriptionPattern`, the KIP-664
  admin calls), and a scenario the 2.8 client cannot express is skipped with the client named.
- **What runs** is decided as in the Go program: ApiVersions v0 is asked over a bare socket at the start (it is answered before
  any login), and a scenario that needs an API the broker does not advertise, or advertises only below what the client speaks,
  is skipped with the API named.

The scenarios (plain mode):

| Scenario | What it does |
|---|---|
| `roundtrip-{none,gzip,snappy,lz4,zstd}` | 200 keyed records with headers (one empty) and a null-key null-value record, over three partitions, read back exactly |
| `idempotent` | 2000 records with `enable.idempotence` and five in flight: once each, in order per key |
| `txn-commit-abort` | commit 10, abort 10, commit 5: `read_committed` sees 15, `read_uncommitted` 25 |
| `txn-send-offsets` | consume-transform-produce with `sendOffsetsToTransaction` (group metadata; the group id alone where TxnOffsetCommit is below v3), one transaction aborted and redone: output once each, input committed to its end |
| `txn-last-stable-offset` | an open transaction holds the `read_committed` end (listOffsets and `endOffsets`) at its first offset while the log goes on |
| `group-classic`, `group-848` | two members split four partitions (a rebalance), read and commit; one leaves and the other takes over; a new member resumes with exactly the records after the commit |
| `rebalance-commits-classic`, `-848` | four members join and two leave while records flow; each commits asynchronously after every poll and synchronously in `onPartitionsRevoked`; no commit may be refused and no record read twice. Under KIP-848 a commit in flight when the member's epoch moves arrives at an older epoch (KIP-1251, doze G848-10) |
| `group-848-regex` | `subscribe(SubscriptionPattern)`: the broker resolves the expression; a matching topic made later is picked up, one that does not match is left out |
| `admin-groups-classic`, `-848` | list and describe (Stable, the member and its assignment); delete and an offset change refused while a member is in (GroupNotEmpty, UnknownMemberId); the static member removed by the admin; offsets altered, one deleted; the group deleted |
| `admin-topics`, `admin-configs`, `admin-create-partitions`, `admin-delete-records`, `admin-list-offsets`, `admin-max-timestamp`, `admin-cluster`, `admin-acls`, `admin-scram-users`, `admin-transactions` | the Admin client's calls of each, with their refusals (exists, unknown, a decrease, past the end, an unknown config, two SCRAM alterations of one user in a request at Kafka 4, too few iterations) |
| `offsets-for-times-log-append`, `-compressed` | `offsetsForTimes` on a LogAppendTime topic and inside gzip batches: the first record at or after each target, record-exactly |
| `produce-fetch-limits` | a record over `max.message.bytes` refused (RecordTooLargeException); one under it read with fetch limits of 1 KB (KIP-74) |

Under `-sasl`: PLAIN, SCRAM-SHA-256 and SCRAM-SHA-512 cells (a topic made, 20 records written and read, a wrong password refused
as `SaslAuthenticationException` within 15 s), and `sasl-users` (a user made with `alterUserScramCredentials` logs in with both
SCRAM mechanisms, cannot with PLAIN, and is refused once deleted). Under `-tls`: SASL_SSL with SCRAM-SHA-512 (and a wrong
password), SSL, and SSL without the CA (`SslAuthenticationException`), the CA given as a PEM truststore (KIP-651).

The Java client at each major, as run on 2026-11-15 (`-versions`, `-sasl -versions`, `-tls`), after the Go clients. No failure.
`4.4` is kafka-clients 4.4.0, `2.8` is 2.8.2:

| | Kafka 1 | Kafka 2 | Kafka 3 | Kafka 4 |
|---|---|---|---|---|
| roundtrip none, gzip, snappy, lz4; idempotent; txn-commit-abort, txn-send-offsets (a), txn-last-stable-offset | 4.4, 2.8 pass | 4.4, 2.8 pass | 4.4 pass | 4.4 pass |
| roundtrip-zstd | skip (b) | 4.4, 2.8 pass | 4.4 pass | 4.4 pass |
| group-classic, rebalance-commits-classic, admin-groups-classic (c) | 4.4, 2.8 pass | 4.4, 2.8 pass | 4.4 pass | 4.4 pass |
| group-848, rebalance-commits-848, admin-groups-848, group-848-regex | skip (d) | skip (d) | skip (d) | 4.4 pass |
| admin-topics, -create-partitions, -delete-records, -list-offsets, -cluster; offsets-for-times-log-append, -compressed; produce-fetch-limits | 4.4, 2.8 pass | 4.4, 2.8 pass | 4.4 pass | 4.4 pass |
| admin-configs | skip (e) | 4.4, 2.8 pass | 4.4 pass | 4.4 pass |
| admin-max-timestamp | skip (f) | skip (f) | 4.4 pass | 4.4 pass |
| admin-acls | skip (g) | 4.4, 2.8 pass | 4.4 pass | 4.4 pass |
| admin-scram-users, sasl-users | skip (h) | 4.4, 2.8 pass | 4.4 pass | 4.4 pass |
| admin-transactions | skip (i) | skip (i) | 4.4 pass | 4.4 pass |
| SASL cells: PLAIN, SCRAM-SHA-256, SCRAM-SHA-512 | 4.4, 2.8 pass | 4.4, 2.8 pass | 4.4 pass | 4.4 pass |
| TLS cells: SASL_SSL, SSL, SSL without the CA | | | | 4.4 pass |
| **plain, 4.4: passed / skipped** | **19 / 10** | **23 / 6** | **25 / 4** | **29 / 0** |
| **plain, 2.8: passed / skipped** | **19 / 10** | **23 / 6** | one skip line | one skip line |

The 2.8 client is not run at Kafka 3 and 4 (one skip line each). (a) At Kafka 1, `sendOffsetsToTransaction` gets the group id alone
(TxnOffsetCommit v0; generation fencing needs v3, KIP-447). (b) Produce up to v5; zstd needs v7 (KIP-110). (c) At Kafka 1 the member is
dynamic (JoinGroup v2; static members need v5) and the OffsetDelete step is left out (Kafka 2.4, KIP-496). (d) No ConsumerGroupHeartbeat
(see (a) of the Go table). (e) No IncrementalAlterConfigs (Kafka 2.3). (f) ListOffsets below v7 (KIP-734, Kafka 3.0). (g) No ACL APIs at
Kafka 1. (h) No SCRAM credential APIs (Kafka 2.7, KIP-554). (i) No ListTransactions, DescribeTransactions, DescribeProducers (Kafka 3.0,
KIP-664), and the 2.8 client has no such calls.

**What the Java client found.** One broker defect, fixed: Metadata v12 by topic id (T9 in `docs/port/behaviours/topics.md`). The
Java consumer asks for the topics a KIP-848 regular expression assigned it by id, with the name left at `""`; doze read `""` as a
name and answered INVALID_TOPIC_EXCEPTION, so the member never consumed (`group-848-regex`). Kafka 4.3.1 answers by id whenever an id
is given. One mistake was the test's: two SCRAM alterations of one user in one request are DUPLICATE_RESOURCE in KRaft (doze is
right; `admin-scram-users` now checks that refusal at Kafka 4 and makes the two credentials one request each).

## What the clients taught

- Records are keyed, because franz-go's producer puts a whole unkeyed batch in one partition, which leaves the other
  members nothing to read.
- The first member of a group is seated before the others start, so the first in the list leads (the first to join
  leads, as in Kafka). Which library leads matters: kafka-go writes its subscription as version 1 without the owned
  partitions that version 1 carries (`0001`, the topics, null user data, and nothing more), and librdkafka as leader
  cannot read it and assigns kafka-go nothing. `-members librdkafka,kafka-go` shows it at every major; the broker passes
  the bytes through as Kafka does. Before the wait, `mixed` failed with "kafka-go read nothing" in about one run in five
  at Kafka 1 and 2, whenever kcat happened to join first (found by `-versions`).
- `kcat -o beginning` starts at offset 0 on every run whatever is committed; the resume needs
  `-X auto.offset.reset=earliest`. kcat reads no option after the first topic.
- kafka-go does not retry a first Metadata answer for a topic that does not exist yet (a recorded Kafka answers it
  the same way); franz-go asks for auto-creation only when told to. The topic is therefore made first.

## The new group protocol (KIP-848)

franz-go (`kgo.ServerSideBalancer()`; its `GroupProtocol` option is only the classic protocol's name) and librdkafka
(`-X group.protocol=consumer`) also run as consumer-protocol members (`franz-go-848`, `librdkafka-848`). Scenarios:
the same two libraries in a group of their own; `upgrade-848` (classic members first, then new-protocol ones join: an
upgrade) and `classic-joins-848` (the reverse); `churn-848`. A graceful shutdown of a group of both stops the
new-protocol members last, as the last of them gone turns the group classic again and a classic member's final commit
would be under a generation that is gone. kafka-go and sarama stay classic: kafka-go writes subscription version 1
without the owned partitions, which a coordinator that reads subscriptions cannot read, so it is left out of groups
with the new protocol. `-members franz-go-848,sarama` runs a group of the named clients, in join order.

## Transactions

Run with `-only eos-franz-go`, `-only txn-franz-go`, `-only txn-sarama`, `-only txn-librdkafka`, `-only fence-franz-go`.

**txn-franz-go, txn-sarama, txn-librdkafka.** A transactional producer of that library (librdkafka's is a small C program, `interop/c/txnprod.c`, which `run.sh` builds against the librdkafka that kcat uses, since kcat has no transactions) writes three transactions (commit, abort,
commit): one of ten records, one of ten that is aborted, and one of five, to a topic of three partitions. Three readers then read the
topic from the start: franz-go, sarama and kcat, each reading committed records only and again reading everything.
The committed readers must see exactly the records of the two commits (15, each once); the others all 25. Each takes
about 9 s: the readers wait two seconds without a record before they say nothing more is coming.

**fence-franz-go.** A zombie: a transactional producer has written five records when another with the same transactional id
starts and commits five of its own. The broker aborts the first's transaction and fences it, so the first's commit fails; a
committed reader sees only the second's records, and one reading everything sees both.

**eos-franz-go.** Consume-transform-produce: franz-go's `GroupTransactSession` reads 200 input records in batches of
25 and writes them to an output topic, committing the input offsets inside each transaction. One transaction is
deliberately aborted and redone. The output must hold every record exactly once for a `read_committed` reader, and
every input offset must be committed.

kafka-go is left out of the readers: its `ReadCommitted` ignores the aborted list and hands the control batches
(markers) up as messages, so it cannot tell a transaction from its markers. sarama has no new-protocol consumer, and
its transactional producer is the classic protocol, which is all this broker serves.
