# Debt register (Phases 0 to 6)

Opened 2026-11-09 at the owner's instruction: nothing known is carried past Phase 6. Every item is cleared (with a test
that fails when the behaviour is removed), or is blocked by a later phase's feature and moves into that phase's plan with
a named scenario, or is a decision that is written down with its reason, or is the owner's. Status: `done`, `moved`,
`decided`, `owner`.

## A. The Phase 6 audit (HEAD 3ed9e1e said: not done; 1 blocker, 6 major, a list of minors)
| # | Finding | Status |
|---|---|---|
| A1 | BLOCKER: a transactional Produce to two partitions took the transaction's lock once for each; a writer arriving between the two holds deadlocked it, and the sweeper with it | done: held once for the request (`transactions_race`) |
| A2 | M1: transactional ids never forgotten, in memory or in the store | done: `transactional.id.expiration.ms`, entries retired under their lock (`transactions_expiry`, crash sweep) |
| A3 | M2: InitProducerId ignored the producer id and epoch the caller holds (KIP-360) | done: bump / answer again / rotate / fence (`rules`, `transactions_codes`) |
| A4 | M3: recovery had no test (a stop between the stored decision and the markers, between partitions, before Complete) | done: a stop at every operation of an EndTxn, kill and power cut, commit and abort; one failed operation anywhere (`transactions_crash`) |
| A5 | M4: catalogue items citing tests that do not check them (describe APIs, Prepare rows, full disk, the hold) | done: assertions for every describe item; the Prepare rows through a real stop; faults through `SimFs`; the hold by a deadlock repro that fails on the old code |
| A6 | M5: TxnOffsetCommit ignored generation and member (KIP-447) and was not decided in Kafka's order | done (`transactions_codes`) |
| A7 | M6: read-committed work queued behind the writer and a first use read the whole log inside it | done: an index shared with the readers; a log read once at open, and only if noted to hold transactions (`partitions::tests::txns`, `log::tests::transactional`) |
| A8 | Minors: timeout from the transaction's start; producer state kept a day; an old repeat is OUT_OF_ORDER; any other epoch is PRODUCER_FENCED; AddPartitions judges partitions first; ListOffsets by time under read_committed; the real last stable offset at every isolation; markers raise the partition's producer epoch; producers with nothing left in the log are forgotten; DescribeTransactions names the offsets partitions; ListTransactions v1 duration | done (each with a test; `transactions.md`) |
| A9 | Minors left as they are, with the reason: a marker written twice after a crash (Kafka may write one twice after a retry; the index ignores the second) | decided |
| A10 | Gates red at the audit's HEAD (clippy too_many_lines in `versions_txn`); a librdkafka transactional producer and a zombie scenario missing from interop | done: split; `txn-librdkafka`, `fence-franz-go` |
| A11 | Re-audit of the fixes | open until the re-audit reports |

## B. Phase 5 (groups, KIP-848)
| # | Item | Status |
|---|---|---|
| B1-B6 | The six mutations no unit test caught (cooperative `owned` retain, classic SyncGroup "moved on", `hand_over` timer and previous assignment, `hand_back` timers, the registry's retry behind a deletion, `end_revocation_if_given_up`) | done: each has a test and each test fails when its branch is removed (`group/tests/{classic_join,timers}.rs`, `registry/tests/queued.rs`) |
| B7 | OffsetFetch v9 did not look at the member and epoch | done (source): UNKNOWN_MEMBER_ID, STALE_MEMBER_EPOCH / ILLEGAL_GENERATION for a classic member (`offset_fetch_checks`, `consumer/tests/fetch.rs`). Also found: `require_stable` fetches now answer UNSTABLE_OFFSET_COMMIT for offsets a transaction is still deciding |
| B8 | Unrecorded words: "RackId can't be empty.", "MemberId can't be empty." and the order of the request checks; a consumer heartbeat to a classic group (not a consumer group; the upgrade refusals incl. the size) | done from the source (`consumer_group_words`, `group/tests/timers.rs`) |
| B9 | The topic table rebuilt for each heartbeat | done: built when the topics change, shared by the groups (`topics_version`, `a_topic_made_after_the_member_subscribed_is_given_to_it`) |
| B10 | A member cap checked after a conversion; the group record rewritten on each classic join into a consumer group | done: the size refusal comes before the upgrade; a keeper test counts the writes |
| B11 | Rack-aware assignment; the heartbeat and assignment intervals only a test knob | decided: a one-broker cluster has no racks to place by, so the rack ids are kept and shown and change nothing, as in Kafka for a broker with no `broker.rack`; the intervals stay at Kafka's defaults, the assignment one a test knob |

## C. Phase 4 (groups, classic)
| # | Item | Status |
|---|---|---|
| C1 | DescribeGroups showed a fixed `/127.0.0.1` | done: the peer address, for both group describes (`group_peers`, a TCP test) |
| C2 | A request queued behind a group's deletion was put again only for a JoinGroup | done: every request is (`registry/tests/queued.rs`) |
| C3 | One global cap on issued member ids; an id lasted five minutes whatever the session timeout | done: a cap for each group, and an id lasts the session timeout of the join that was given it (`registry/tests/issued.rs`, `member_ids`) |
| C4 | librdkafka and FindCoordinator, owed from Phase 3 | done: librdkafka (kcat and a C producer) and every other client ran FindCoordinator, groups, transactions and the new protocol in `interop/` without a finding |

## D. Phases 2 and 3 (storage, minimum broker)
| # | Item | Status |
|---|---|---|
| D1 | Sealed segments each hold two descriptors | done: open partitions are weighed by their segments and closed, least recently used first, to stay inside a budget taken from the descriptor limit (`partitions::tests::descriptors`, `open_files`) |
| D2 | A read asked the file for `max_bytes` plus 16 KiB before the limit was known (7.5 MiB for 8 KiB) | done (`log::tests::read`) |
| D3 | Go behaviours not built | decided: a closed partition's bounds are got by opening it (the cache evicts the least recently used; a never-written one is not opened); `max_bytes` of 0 returns the first batch, as Kafka's minimum-one-message read does; `segment.bytes` at runtime is AlterConfigs: Phase 7 |
| D4 | Rewriting a batch into the topic's `compression.type` | moved: Phase 7 (topic configs) |

## E. Phase 6
| # | Item | Status |
|---|---|---|
| E1 | `endtxn-while-preparing` is a table test, not a replay | decided: the harness keeps it as evidence only (`NoReplay`; it relied on a setting that does not hold the markers back); the table is tested by the rules, through a real stop, and the one difference of timing (the second request waits where Kafka answers CONCURRENT_TRANSACTIONS) is written down in `transactions.md` TX-7 |
| E2 | No disk-full test of the transaction path | done: a stop at every operation, and one failed operation (full disk and device error) anywhere in an EndTxn |
| E3 | The transaction scenarios took 18 s | done: the readers' two-second quiet periods, taken for each partition in turn; now at once (9 s) |
| E4 | KIP-890 transaction v2 is not offered | decided: a scope decision (`transactions.md`); clients fall back to the classic protocol, which the recordings and every client exercise |
| E5 | `idempotent-write-from-topic-write` | moved: Phase 8 (needs ACLs and SASL) |

## F. Process
| # | Item | Status |
|---|---|---|
| F1 | The two-sided test diet of `kafka-wire` | see the Phase 1 test diet entry in `PLAN.md` |
| F2 | A private GitHub repository for the port | owner (not a technical item; CI is kept local by the owner's choice) |
| F3 | Java client | closed by the owner (Go clients only) |

## Carried into later phases, by name
- **Phase 7 (compaction):** a compacted batch keeps its last offset delta with fewer records, so `Peek::read` must stop
  insisting that the record count agrees with it and the log walks (`visit_batches`, `read`) must advance by the last offset
  delta; the transaction index and the producer history read through the same walk. `segment.bytes` and the other topic
  configs act at runtime (AlterConfigs); batches are rewritten into the topic's `compression.type`.
- **Phase 8 (security):** `idempotent-write-from-topic-write`; ACLs on the transaction APIs (TRANSACTIONAL_ID_AUTHORIZATION_FAILED,
  GROUP_AUTHORIZATION_FAILED on TxnOffsetCommit, TOPIC_AUTHORIZATION_FAILED on AddPartitionsToTxn).
