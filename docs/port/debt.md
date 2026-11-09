# Debt register (Phases 0 to 7)

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
| A11 | Re-audit of the fixes | the first re-audit found three majors (a fence that left the last epoch to be presented back; the retired-entry lookup and the TxnOffsetCommit producer check and classic-group rules with no test that fails without them) and minors; all fixed and each fix mutation-checked (`transactions_fence`, `transactions_retired`, `transactions_offsets`, the reverse-order hold test, a power-cut sweep for the marker flush, pending offsets in the Prepare states, marker epochs after a reopen, the small-copy read); ListTransactions is offered at v0 to v1 with Kafka's duration rule. A narrow third look at the delta is in `PLAN.md` |
| A12 | The third audit (HEAD c189113: not done; no blocker, one major, minors) | done: a test for a marker restarting sequences after a reopen; ListTransactions/DescribeTransactions keep a finished transaction's start (only an id that began none is -1); the last stable offset clamped to the end (`clamp_stable`); tests for the Again path's new timeout and the offered ListTransactions range. Each fix fails its test when put back. Decided: the retention sweep reopens closed partitions (a full index scan for those that hold transactions) only when there are more partitions than `--max-open-partitions`; `forget_before` on the index stays for the memory it frees; producers known only from a marker are kept (stricter than Kafka, not observable). No fourth audit of this delta was run: the Phase 7 audit reads it |

## B. Phase 5 (groups, KIP-848)
| # | Item | Status |
|---|---|---|
| B1-B6 | The six mutations no unit test caught (cooperative `owned` retain, classic SyncGroup "moved on", `hand_over` timer and previous assignment, `hand_back` timers, the registry's retry behind a deletion, `end_revocation_if_given_up`) | done: each has a test and each test fails when its branch is removed (`group/tests/{classic_join,timers}.rs`, `registry/tests/queued.rs`) |
| B7 | OffsetFetch v9 did not look at the member and epoch | done (source): UNKNOWN_MEMBER_ID, STALE_MEMBER_EPOCH / ILLEGAL_GENERATION for a classic member (`offset_fetch_checks`, `consumer/tests/fetch.rs`). Also found: `require_stable` fetches now answer UNSTABLE_OFFSET_COMMIT for offsets a transaction is still deciding |
| B8 | Unrecorded words: "RackId can't be empty.", "MemberId can't be empty." and the order of the request checks; a consumer heartbeat to a classic group (not a consumer group; the upgrade refusals incl. the size) | done from the source (`consumer_group_words`, `group/tests/timers.rs`) |
| B9 | The topic table rebuilt for each heartbeat | done: built when the topics change, shared by the groups (`topics_version`, `a_topic_made_after_the_member_subscribed_is_given_to_it`) |
| B10 | A member cap checked after a conversion; the group record rewritten on each classic join into a consumer group | done: the size refusal comes before the upgrade; a keeper test counts the writes |
| B12 | OffsetCommit by a consumer-protocol member at an epoch older than its current one (found by the Phase 6 re-audit) | decided: `groups848.md` G848-10 (the empty-group half is fixed) |
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
| D3 | Go behaviours not built | decided: a closed partition's bounds are got by opening it (the cache evicts the least recently used; a never-written one is not opened); `max_bytes` of 0 returns the first batch, as Kafka's minimum-one-message read does; `segment.bytes` at runtime is AlterConfigs: done in Phase 7 (`broker_configs::a_topic_changed_while_open_*`) |
| D4 | Rewriting a batch into the topic's `compression.type` | done: at Produce, as Kafka does (`produce_records`, `kafka-wire` `recode::tests`); the cleaner keeps the codec a batch has (`compaction.md` CMP-D4) |

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
| F1 | The two-sided test diet of `kafka-wire` | done: 123 tests to 87, every mutant the old suite caught still caught (`PLAN.md`, "Phase 1 test diet (2026-11-09)") |
| F2 | A private GitHub repository for the port | owner (not a technical item; CI is kept local by the owner's choice) |
| F3 | Java client | closed by the owner (Go clients only) |

## G. Phase 7
| # | Item | Status |
|---|---|---|
| G1 | Differences from Kafka in the config and partition APIs (loggers and other resource types, the broker's config list and its five changeable configs, synonym names, flag defaults, answer order, `--max-partitions`, `segment.ms` by wall clock) | decided: `admin.md` ADM-D1 to ADM-D7 |
| G2 | Differences from Kafka in compaction (segments not merged, the last batch of each kept and a segment left with only that batch removed, exact keys in a bounded map, one cleaner with no throttle, the codec a batch has) | decided: `compaction.md` CMP-D1 to CMP-D4 |
| G3 | Empty batches left by compaction stalled librdkafka (a run of fetch answers with nothing to count as progress ends in "might be too large to fetch") | done: found by the `compaction` interop scenario, fixed in the reader (CMP-6); the walk that relearns producers had to be kept on the raw read, which the producer-retry test caught |
| G4 | The cleaner and retention under many partitions (a sweep reopens closed partitions; a reopened one that holds transactions reads its log once) | decided: as A12 |

## H. The Phase 7 audits (first, HEAD 9eded61: not done, 1 blocker, 4 major, 10 minor, 52 mutation gaps; second, HEAD b394527: done, no blocker or major, 5 minors, 6 test gaps)
| # | Finding | Status |
|---|---|---|
| H1 | BLOCKER B1: records of aborted transactions entered the compaction offset map, so a committed value of the same key was lost | done: a69555b (`compaction_txn`, broker `compaction_through_the_broker`); the second audit found the marker half untested, now covered (`one_producer_aborts_then_commits_the_same_key_and_the_old_value_is_compacted_away`) |
| H2 | M1: concurrent AlterConfigs lost acknowledged changes | done: 342647c (`alter_configs_race`) |
| H3 | M2: retention never took an expired or over-bound active segment; `segment.ms` unimplemented | done: 3c3d969 (ADM-1, S20 rewritten). `segment.ms` counts wall-clock age since the first write, not Kafka's record-time rule: decided, ADM-D7 |
| H4 | M3: lags planned differently from Kafka and untested; M4: a round held the writer for seconds | done: b106c8a; the writer yielding between parts is observed (`compaction_parts`) |
| H5 | N1, N2, N4, N5, N6, N7, N10 | done: 193f24e, 155658a. N2's two halves are tested: a stop during the rewrite (`compaction_stop` in storage) and shutdown setting the flag (`compaction_stop` in the broker) |
| H6 | N3: a hot key left a file for every segment it filled | done: b394527 (CMP-D1; a segment left with only its ending batch is removed, the first never). The second audit tried to break it with sweeps at every stop and power cut, failed operations, concurrent readers, DeleteRecords at every offset, retention after a clean and a 400-seed model run: it held. Kept in the repo: `tests/compaction/gaps.rs`, `gaps_disk.rs`, `tests/real_disk/compaction.rs`, `broker/tests/compaction_model.rs` |
| H7 | N8: an 18 MB Go binary was committed once | decided: untracked and ignored (155658a); it stays in the local history, which has never been pushed. Before the repository is published, remove it with a history filter: owner |
| H8 | N9: `.tmp` names are shared by overlapping rounds | decided: one cleaner task is the only caller of `check_cleaner`; a stale file is truncated by the next round and tidied at open (tested); no per-partition guard is added |
| H9 | The 52 mutation gaps | done: all re-run on the fixed code (57 mutants with seven for N3; 15 died to the old suite, 42 survived). 34 now die to a new test (storage: `retain`, `compaction_edges`, `compaction_txn`, `compaction_segments`, `thinned`, `producer_last`, `skip`; broker: `configs`, `topic_config`, `producers`, `alter_configs`, `describe_configs`, `create_partitions`, `reconfigure`, `delete_records`, `actor/transactions`). 8 are written down, not tested: `j03`, `j04`, `j07` (guards that repeat each other, or that clean a segment one round early, which is correct: an aborted record may go at any time), `j24`, `j25` (post-write checks of a file the rewrite made, which fire only if the rewrite is wrong; the rewrite itself is tested), `lc08` (the file-time fallback of a segment with no records; after N3 only the first segment can be one, and it is clean), `cp08` (`check_assignment` has already refused every list this check could refuse), `p6-clamp-call` (the clamp's input is already in range; only a race between two reads could differ). Two more are killed only at function level because no request reaches them (`d06`, `cp12`) |
| H10 | Found by the mutation work, not by the audit: a broker number with spaces around it passed validation and was silently not applied; the refusal of an over-long config value did not use Kafka's words, looked at values the request did not set, and counted bytes where Kafka counts UTF-16 units | done: `configs/broker.rs` trims as validation does; `configs.rs` `sets_too_long_a_value` (source `ConfigurationControlManager`); tests in `configs/tests.rs` |
| H11 | Second audit, minor 1: a transactional producer whose data DeleteRecords or retention took, but whose marker stayed, was refused (OUT_OF_ORDER_SEQUENCE_NUMBER) after a reopen and accepted before it | done: a marker found in the log at open, with nothing else known of its producer, leaves the sequences unknown (TX-4); `producers_after_trim`, `compaction_model` (fails at seed 24 on the old code) |
| H12 | Second audit, minors 2 to 5 and its test gaps `m11`, `m13`, `m18`, `m20`, `m21`, `m23` | done or decided: `segment.ms` ADM-D7; the docs that contradicted the code corrected (ADM-1, S20, phase7.md, CMP-D1, which wrongly said Kafka deletes an empty segment: Kafka merges them by size); N8 and N9 as H7 and H8. The six gaps are killed. `m07` (no `dir_dirty` flag after a removal) is equivalent, because the checkpoint write flushes the directory and a lost removal is redone by the next round; `m22` (aborted ranges forgotten one round late) is a race-only property, decided |

## Carried into later phases, by name
- **Phase 7 (compaction), done:** the log walks advance by the last offset delta and a stored batch may hold fewer records than it
  spans (`compaction.md` CMP-6); `segment.bytes` and the other topic configs act at runtime (`admin.md` ADM-6); batches are
  rewritten into the topic's `compression.type` at Produce (D4).
- **Phase 8 (security):** `idempotent-write-from-topic-write`; ACLs on the transaction APIs (TRANSACTIONAL_ID_AUTHORIZATION_FAILED,
  GROUP_AUTHORIZATION_FAILED on TxnOffsetCommit, TOPIC_AUTHORIZATION_FAILED on AddPartitionsToTxn).
