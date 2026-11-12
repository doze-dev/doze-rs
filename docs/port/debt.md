# Debt register (Phases 0 to 8)

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

## I. Phase 8 (security: SASL, ACLs, TLS, the allowed-hosts guard), built 2026-11-12, audits pending
Statuses as above. The catalogue is `behaviours/security.md` (index at its top; `tools/check-security-catalogue.ts` checks its numbers and test names). The first table is every deviation
the catalogue lists, one row each; the second is what the slices reported left unfinished, and the audits.

### I.1 Deviations (all `decided`; the number is the catalogue's)
| # | Deviation | Status |
|---|---|---|
| SEC-D1 | A request in the middle of a login other than ApiVersions and SaslHandshake closes unanswered (Kafka answers 34 in the request's shape, then closes) | decided: the Go edition does the same; recorded `request-before-token-who`; answering needs the body of a stranger's request, which is never decoded |
| SEC-D2 | A 30 s login budget from the accept (Kafka has none) | decided: a dev broker should not sit open to a silent socket (D10); the Go edition's rule |
| SEC-D3 | SaslHandshake v0 (raw tokens) is not served | decided: no client of the four libraries uses it; Kafka 1.x tools only (`I14`) |
| SEC-D4 | No 100 ms `connection.failed.authentication.delay.ms` before a failure is answered | decided: the delay slows a local attacker nobody has; see `I15` |
| SEC-D5 | Passwords are not zeroed on drop | decided: no `zeroize` dependency (weight rule); they are never printed; see `I16` |
| SEC-D6 | `Session::Unauthenticated` carries a `versions_answered` flag the plan's enum did not | decided: SEC-12 needs it; an implementation detail |
| SEC-D7 | The ACL APIs and the authorizer work with SASL off, enforced for `User:ANONYMOUS` (Kafka without an authorizer answers 54) | decided: owner's D4; it is what lets `--acl-default deny` be used without SASL |
| SEC-D8 | A CLUSTER_AUTHORIZATION_FAILED carries no long message | decided: Kafka's embeds a request dump that cannot be reproduced; the code is Kafka's; see `I10` |
| SEC-D9 | A super user must be written `Type:name`, both parts non-empty | decided: the Go edition's rule; Kafka only needs a colon |
| SEC-D10 | DescribeAcls and DeleteAcls list sorted | decided: Kafka's order is a hash map's; clients sort |
| SEC-D11 | No limit of 10,000 new ACLs; DescribeConfigs does not report `allow.everyone.if.no.acl.found` or `super.users` | decided for now: Kafka's outcome for the first is unspecified, real Kafka lists neither setting; see `I8`, `I9` |
| SEC-D12 | The server nonce is 18 random bytes in base64 (Kafka: base-36 of 130 bits) | decided: both unpredictable and printable; no client depends on the alphabet |
| SEC-D13 | Several SCRAM extensions are signed in the order sent; `tokenauth=true` is an unknown user | decided: Kafka's order is a random `Map.copyOf` order; delegation tokens are out of scope |
| SEC-D14 | Bootstrap users have 4096 iterations and a random 16-byte salt | decided: a bootstrap user has no admin client to choose them; users made over the API keep theirs (4096 to 16384, SEC-163) |
| SEC-D15 | Derived SCRAM keys and proofs are not zeroed | decided: with SEC-D5 |
| SEC-D16 | Results in the order asked where Kafka's order is a hash map's, a merge or a shuffle (DeleteTopics, Produce, DeleteRecords, Fetch, CreateTopics duplicates) | decided: clients match by name; the stated orders are kept |
| SEC-D17 | CreateTopics refused for want of CREATE answers empty `configs`; `__cluster_metadata` and DeleteTopics' request-shape errors are not special-cased | decided: the "never null" rule; not authorization, so not added here |
| SEC-D18 | An unsupported resource type is checked for the right, then answered 42; broker configs are validated by this broker's table | decided: the order Kafka asks in is kept; the table is the Phase 7 one |
| SEC-D19 | A follower Fetch holding CLUSTER_ACTION is read like a consumer's; a nameless Metadata entry has no authorized-operations field | decided: no replication to serve; Kafka throws on the second |
| SEC-D20 | The topic-API rules are written from the source and not recorded | decided: D9; recording is `I1` |
| SEC-D30 | DeleteGroups with the same allowed name twice answers it twice | decided: pinned by `group_admin`; see `I11` |
| SEC-D31 | SyncGroup v5+ with no protocol type or name is 23 up front only for a caller that is also refused | decided: an allowed caller's request goes to the group, which judges the names it is given |
| SEC-D32 | AddPartitionsToTxn on the internal topics is 3 (or 29), not 29 | decided: those topics are not topics here |
| SEC-D33 | InitProducerId's two-phase-commit check is unreachable | decided: v6 is not served |
| SEC-D34 | A DescribeTransactions entry refused with 53 carries schema defaults | decided, unsure: from the source alone; settled by `I1` |
| SEC-D35 | ConsumerGroupDescribe judges the topic names it has; a deleted topic's name is empty | decided, unsure: whether Kafka's coordinator gives `""` was not confirmed; `I1` |
| SEC-D36 | FindCoordinator before v4 puts the error's own message in the answer | decided, unsure: read from `prepareOldResponse`, not recorded; `I1` |
| SEC-D37 | Share groups, streams groups and AddPartitionsToTxn v4+ are not served, so not checked | decided: not served at all |
| SEC-D40 | Client certificates and mutual TLS are out of scope | decided: the Go edition's limit; see `I12` |
| SEC-D41 | `ssl.principal.mapping.rules` has nothing to map | decided: follows SEC-D40 |
| SEC-D42 | Protocol versions and cipher suites are rustls's with ring (TLS 1.2 and 1.3, AEAD suites) | decided: no Kafka `ssl.*` counterpart; a TLS 1.1 or CBC-only client fails its handshake |
| SEC-D43 | No certificate generation and no reload (a renewal needs a restart) | decided: `rcgen` would add 9 packages; the docs give the `openssl` recipe |
| SEC-D44 | PEM only, one certificate, no SNI, ALPN or session tickets | decided: the dev case; weight |
| SEC-D45 | A failed handshake may send a TLS alert record | decided: rustls writes it; the JVM does the same; no Kafka bytes are sent |
| SEC-D46 | The pre-auth budget covers the handshake even with SASL off | decided: extends SEC-D2 |
| SEC-D47 | The advertised host is not checked against the certificate | decided: Kafka's broker does not either |
| SEC-D48 | A `tls` build needs a C compiler (ring) | decided: the feature is off by default; the default build needs none |
| SEC-D50 | Two layers of users (bootstrap and stored); the API refuses a bootstrap user with 93 | decided: the alternative makes login depend on the order of events |
| SEC-D51 | The credential APIs work with SASL off, so an open broker lets anyone manage users | decided: the same posture as SEC-D7; `--acl-default deny` with a super user closes it |
| SEC-D52 | Credential results in the request's order, or by name | decided: Kafka's order is a hash map's |
| SEC-D53 | `metadata.version` is not modelled | decided: a broker of 4.x always supports SCRAM |
| SEC-D54 | Describe with a null user list from a denied caller answers 31 where Kafka throws | decided, unsure: not verified against a running Kafka; `I1` |
| SEC-D55 | A changed credential applies to new logins; no re-authentication | decided: Kafka's default `connections.max.reauth.ms` is 0; see `I13` |
| SEC-D56 | Credentials are a table in `meta.redb`, not records in a metadata log | decided: there is no replicated log; keys are not encrypted at rest |
| SEC-D57 | An unusable stored credential row is left and named at warn | decided: Kafka would fail to replay its log |
| SEC-D58 | The environment variable of users is read once and not cleared | decided: `set_var` is unsafe on edition 2024; the file with mode 600 is the answer |
| SEC-D59 | File and variable passwords are ASCII without NUL and held for the life of the process | decided: no SASLprep (not in this phase); as SEC-D5 |
| SEC-D60 | The guard has no `.doze` special case | decided: PLAN.md line 19; a name the operator needs is listed with `--console-allow-host` |
| SEC-D61 | The guard refuses five shapes the Go edition's allows: a bare IPv6 literal, brackets around anything but an IPv6 literal, an IP literal with a trailing dot, a name with characters outside letters, digits, `-` and `_` (or a non-ASCII one), a name over 253 bytes or a label over 63 | decided: a refusal costs a flag, an acceptance costs the defence; no browser sends any of them |
| SEC-D62 | Every configuration that cannot start exits 2 (it exited 1) | decided: the Go edition's code, and clap's own for a bad flag; no test or script read 1 |
| SEC-D63 | `--console-allow-host` and `--console-unprotected` are parsed and kept and nothing acts on them; a console address is "on" unless `--console=false` | decided: there is no console listener before Phase 9 (`I21`); the start-up rule was built now because its inputs exist |

### I.2 Left unfinished, and the audits
| # | Item | Status |
|---|---|---|
| I1 | The authorization matrix is source-derived with no recording for every API except Metadata and InitProducerId. Families, each named so a recording can be checked off: **topic APIs** (CreateTopics, DeleteTopics, Produce, Fetch, ListOffsets, DeleteRecords, CreatePartitions, DescribeConfigs, AlterConfigs, IncrementalAlterConfigs, DescribeProducers, Metadata's authorized-operations fields: SEC-60 to SEC-72); **group APIs** (FindCoordinator, JoinGroup, SyncGroup, Heartbeat, LeaveGroup, ConsumerGroupHeartbeat, DescribeGroups, ConsumerGroupDescribe, ListGroups, OffsetCommit, OffsetFetch, DeleteGroups, OffsetDelete: SEC-101 to SEC-113); **transaction APIs** (AddPartitionsToTxn, AddOffsetsToTxn, TxnOffsetCommit, EndTxn, DescribeTransactions, ListTransactions, InitProducerId's transactional half: SEC-114 to SEC-120); **credential APIs** 50 and 51 (SEC-160 to SEC-169); **TLS** (SEC-140 to SEC-154: real clients in `interop/run.sh -tls` are the evidence). The proposal: record an `authz-matrix` scenario against `apache/kafka:4.3.1` (decision D9; details and the state of Docker here in `oracle.md`) | moved: owner to approve the recording; the author runs it, diffs, and corrects the rows and SEC-D20, D34 to D36 and D54 |
| I2 | ACL APIs are not offered at `--kafka-version 1` (the vendored protocol crate cannot decode v0, which is all Kafka 1.x admin tools speak) | moved: decoding v0 ourselves is small and waits for someone who asks (D7); `wire.md` W10 |
| I3 | Linux weight of the `tls` build is unmeasured (the `weight-tls` limits 6.1 MB and 137 packages come from the darwin delta and the spike) | moved: the CI's first run after the merge measures it; `weight.md` Phase 8 close |
| I4 | The CI package gate rose in Phase 8 (115 to 126 at 8a for the RustCrypto crates; the default build is 123 by the CI method on darwin) and the binary limit has about 0.25% headroom on Linux if the darwin steps carry over | owner: after the first CI run, move the size limit to the CI's own measurement plus 5% with the reason in `weight.md`; the package gate has 3 spare |
| I5 | The first independent audit of Phase 8 (HEAD 909d147) | pending: the audit is running; the author fills this row |
| I6 | The second independent audit of Phase 8 (HEAD 909d147) | pending: the audit is running; the author fills this row |
| I7 | Fixes and the re-audit | pending: after I5 and I6 |
| I8 | A CreateAcls of more than 10,000 new ACLs is not limited (Kafka's outcome is unspecified, `kafka-semantics.md` U3) | owner: needs Kafka's real answer, which only a recording gives; a broker of one developer rarely has 10,000 ACLs |
| I9 | DescribeConfigs on the broker does not report `allow.everyone.if.no.acl.found` or `super.users` (the Go edition does; real Kafka lists neither) | moved: with `authz-matrix` (I1); the console's Access page (Phase 9) shows both from the flags regardless |
| I10 | The long text of CLUSTER_AUTHORIZATION_FAILED (`Request(processor=N, connectionId=...) needs ALTER permission.`) is not reproduced | moved: with I1; the code is right and clients read the code |
| I11 | DeleteGroups with the same allowed name twice answers it twice; Kafka de-duplicates first (KA:1431) | owner: a small fix that changes a pinned Phase 5 behaviour (`group_admin`); decide whether to follow Kafka |
| I12 | Client certificates and mTLS principals (`ssl.client.auth`, `ssl.principal.mapping.rules`) | owner: out of scope in the plan ("Not in this phase"); the Go edition has the same limit; build if asked |
| I13 | Re-authentication (KIP-368, `connections.max.reauth.ms`) | owner: out of scope; a logged-in connection stays logged in, and a changed credential applies to new logins |
| I14 | SaslHandshake v0 (raw SASL tokens) | owner: build if a Kafka 1.x client must log in |
| I15 | The 100 ms failed-login delay (`connection.failed.authentication.delay.ms`) | owner: decide if a brute-force slowdown is wanted on a dev broker; a few lines |
| I16 | Zeroing secrets on drop (`zeroize`) | owner: weight versus hygiene; `zeroize` is already in the `tls` build as ring's dependency, so it costs nothing there |
| I17 | The APIs the broker does not serve have no authorization row: `DescribeLogDirs` and the rest of the admin surface outside the Phase 8 matrix, and `DescribeCluster` and the telemetry pair (the rows `version-matrix` still differs on) | moved: Phase 9 for DescribeCluster and the telemetry APIs; each other API joins the matrix, with its test, in the phase that serves it |
| I18 | A build with the `tls` feature on Windows needs the MSVC build tools (ring); not verified, the `windows` CI job compiles the default build only | moved: a Windows developer, or the CI when a Windows job for the feature is wanted |
| I19 | `phase8.md` D1 said "107 to about 121"; the real figure is 122 (8a) and 123 (since 8e, darwin count) | done: corrected in `phase8.md` |
| I20 | The `Access` facts view of the console (principals, mechanisms, the rule that applies) listed under 8g in the plan | moved: Phase 9, with the console |
| I21 | The allowed-hosts guard is a pure function with flags and the start-up rule; nothing is mounted on it | moved: Phase 9 mounts it on the console and the registry; the mount must call `kafka_auth::hostguard::allowed` and answer 403 |
| I22 | Catalogue stale cite found by `tools/check-security-catalogue.ts`: the header of `kafka-server/tests/tls_redaction.rs` cited a rule number past the end of the TLS range (the private-key rule is SEC-152) | done: corrected in the test file's header |

## Carried into later phases, by name
- **Phase 7 (compaction), done:** the log walks advance by the last offset delta and a stored batch may hold fewer records than it
  spans (`compaction.md` CMP-6); `segment.bytes` and the other topic configs act at runtime (`admin.md` ADM-6); batches are
  rewritten into the topic's `compression.type` at Produce (D4).
- **Phase 8 (security), built:** `idempotent-write-from-topic-write` and the ACLs on the transaction APIs are done; what is left is section I.
- **Phase 9 (console, registry):** mounts the allowed-hosts guard (I21), the Access facts view (I20), DescribeCluster and the telemetry APIs (I17).
