# Debt register (Phases 0 to 9a)

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
| SEC-D4 | (withdrawn) the 100 ms `connection.failed.authentication.delay.ms` is built | done: the first audit measured 18.6k PLAIN guesses a second; SEC-192, `--sasl-failed-login-delay`; the narrower SEC-D73 stands |
| SEC-D5 | Passwords are not zeroed on drop (and a SCRAM-only broker keeps none) | decided: no `zeroize` dependency (weight rule); they are never printed; SEC-198 drops them when PLAIN is off; see `I16` |
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
| SEC-D51 | The credential APIs work with SASL off, and with `--acl-default allow` any login may manage users | decided: the same posture as SEC-D7; the audit found it a takeover of the super users, closed by SEC-D70 and said at start (SEC-191); `--acl-default deny` with ACLs closes the rest |
| SEC-D52 | Credential results in the request's order, or by name | decided: Kafka's order is a hash map's |
| SEC-D53 | `metadata.version` is not modelled | decided: a broker of 4.x always supports SCRAM |
| SEC-D54 | Describe with a null user list from a denied caller answers 31 where Kafka throws | decided, unsure: not verified against a running Kafka; `I1` |
| SEC-D55 | A changed credential applies to new logins; no re-authentication | decided: Kafka's default `connections.max.reauth.ms` is 0; see `I13` |
| SEC-D56 | Credentials are a table in `meta.redb`, not records in a metadata log | decided: there is no replicated log; keys are not encrypted at rest, and the file is owner-only (SEC-193) |
| SEC-D57 | An unusable stored credential row is left and named at warn | decided: Kafka would fail to replay its log |
| SEC-D58 | The environment variable of users is read once and not cleared | decided: `set_var` is unsafe on edition 2024; the file with mode 600 is the answer |
| SEC-D59 | File and variable passwords are ASCII without NUL and held for the life of the process when PLAIN is offered | decided: no SASLprep (not in this phase); as SEC-D5 |
| SEC-D60 | The guard has no `.doze` special case | decided: PLAN.md line 19; a name the operator needs is listed with `--console-allow-host` |
| SEC-D61 | The guard refuses five shapes the Go edition's allows: a bare IPv6 literal, brackets around anything but an IPv6 literal, an IP literal with a trailing dot, a name with characters outside letters, digits, `-` and `_` (or a non-ASCII one), a name over 253 bytes or a label over 63 | decided: a refusal costs a flag, an acceptance costs the defence; no browser sends any of them |
| SEC-D62 | Every configuration that cannot start exits 2 (it exited 1) | decided: the Go edition's code, and clap's own for a bad flag; no test or script read 1 |
| SEC-D63 | `--console-allow-host` and `--console-unprotected` are parsed and kept and nothing acts on them; a console address is "on" unless `--console=false` | decided: there is no console listener before Phase 9 (`I21`); the start-up rule was built now because its inputs exist |
| SEC-D70 | Only a super user may change a super user's SCRAM credential (Kafka has no such rule) | decided: the default `--acl-default allow` gives ALTER on the cluster to every login, so without it a login took over every super user (SEC-190); the first audit's major finding |
| SEC-D71 | A stranger's frame is at most 128 KiB (Kafka's default is 512 KiB) | decided: 1000 strangers at 300 KiB each were 325 MB (SEC-194); a token is under 1 KiB but the recorded Kafka takes a 64 KiB one, so not lower; replaces the 512 KiB of D10 |
| SEC-D72 | No `--sasl-allow-empty-users` (the Go edition has it) | decided: a start nobody could log in to was a dead start (SEC-176); the first user comes from a flag, a file, the variable, or a broker started without SASL |
| SEC-D73 | Only a failed login (58) waits for the failed-login delay; Kafka delays every authentication failure | decided: the other two (33, 34) give a guesser nothing (SEC-192) |
| SEC-D80 | A regex subscription is resolved per member, with its own rights, at each heartbeat that changes them (Kafka: per expression, with the rights of the heartbeat that triggered a refresh) | decided: stricter than Kafka, which could hand a member a topic another member of the same text was allowed |
| SEC-D81 | Kafka's `findAclRule` throws or never ends on some names; this broker answers by the rules | decided: nothing can depend on a throw or a hang |
| SEC-D82 | The PREFIXED ACLs of a type are scanned on every check | open at admin scale (thousands of prefixes); fine for tens; a sorted index is the fix, covered by the two model tests |
| SEC-D83 | Metadata by id serves names beside ids and answers a repeated unknown id each time | decided: no client sends either |
| SEC-D84 | DeleteTopics by an unknown name is 3 with no message; neither name nor id is not special-cased | decided: with SEC-D17 |
| SEC-D85 | `--acl-super-user` splits on commas and semicolons | decided: the Go edition's separator and Kafka's |
| SEC-D90 | A member's names and what it inherits (a static member's place, a classic subscription) are held to its own principal's DESCRIBE right at every heartbeat | decided: stricter than Kafka, which checks names once and copies a departed static member's state whole (SEC-230 to SEC-233) |
| SEC-D91 | A classic member of a consumer group is given only topics its principal may DESCRIBE | decided: Kafka's coordinator assigns what the opaque subscription names (SEC-231) |
| SEC-D92 | `--sasl-failed-login-delay` is at most 10 s | decided: Kafka has no limit; a wait of minutes holds a connection and a task per guess (SEC-236) |
| SEC-D93 | A classic group adopted by a consumer group drops, with no handover, the partitions its members held of topics they may not DESCRIBE | recorded by the final audit of Phase 8, not decided: Kafka keeps them until the member rejoins; another member may be given them while the old client still consumes (SEC-239) |

### I.2 Left unfinished, and the audits
| # | Item | Status |
|---|---|---|
| I1 | The authorization matrix is source-derived with no recording for every API except Metadata and InitProducerId. Families, each named so a recording can be checked off: **topic APIs** (CreateTopics, DeleteTopics, Produce, Fetch, ListOffsets, DeleteRecords, CreatePartitions, DescribeConfigs, AlterConfigs, IncrementalAlterConfigs, DescribeProducers, Metadata's authorized-operations fields: SEC-60 to SEC-72); **group APIs** (FindCoordinator, JoinGroup, SyncGroup, Heartbeat, LeaveGroup, ConsumerGroupHeartbeat, DescribeGroups, ConsumerGroupDescribe, ListGroups, OffsetCommit, OffsetFetch, DeleteGroups, OffsetDelete: SEC-101 to SEC-113); **transaction APIs** (AddPartitionsToTxn, AddOffsetsToTxn, TxnOffsetCommit, EndTxn, DescribeTransactions, ListTransactions, InitProducerId's transactional half: SEC-114 to SEC-120); **credential APIs** 50 and 51 (SEC-160 to SEC-169); **TLS** (SEC-140 to SEC-154: real clients in `interop/run.sh -tls` are the evidence). The proposal: record an `authz-matrix` scenario against `apache/kafka:4.3.1` (decision D9; details and the state of Docker here in `oracle.md`) | moved: owner to approve the recording; the author runs it, diffs, and corrects the rows and SEC-D20, D34 to D36 and D54 |
| I2 | ACL APIs are not offered at `--kafka-version 1` (the vendored protocol crate cannot decode v0, which is all Kafka 1.x admin tools speak) | moved: decoding v0 ourselves is small and waits for someone who asks (D7); `wire.md` W10 |
| I3 | Linux weight of the `tls` build is unmeasured (the `weight-tls` limits 6.1 MB and 137 packages come from the darwin delta and the spike) | moved: the CI's first run after the merge measures it; `weight.md` Phase 8 close |
| I4 | The CI package gate rose in Phase 8 (115 to 126 at 8a for the RustCrypto crates; the default build is 123 by the CI method on darwin) and the binary limit has about 0.25% headroom on Linux if the darwin steps carry over | owner: after the first CI run, move the size limit to the CI's own measurement plus 5% with the reason in `weight.md`; the package gate has 3 spare |
| I5 | The first independent audit of Phase 8, authentication half (HEAD 909d147) | done: verdict DONE with no blocker. One MAJOR design gap: with SASL on and the default `--acl-default allow`, any logged-in user could replace a stored super user's SCRAM credential and escalate; minors: `meta.redb` mode 644, pre-login memory (1000 strangers took 12 to 333 MB), `--sasl-allow-empty-users` a dead start, no failed-login delay, plaintext passwords kept with SCRAM only, the flush-before-close mutant surviving on real sockets (and ECONNRESET on every failed login on macOS). All fixed (branch p8h, SEC-190 to SEC-198, SEC-D70 to D73); about 100 adversarial tests and 1,400 independent crypto vectors held; 25 of 25 mutants verified |
| I6 | The second independent audit of Phase 8, authorization half (HEAD 909d147) | done: verdict NOT DONE for one MAJOR: a KIP-848 regex subscription skipped Kafka's DESCRIBE filter, so a member with READ on the group was assigned the partitions of a topic it could not see; minors: Metadata authorized-operations bits on refused entries (SEC-71 refuted by the source), a surviving by-type mutant, Kafka's own `findAclRule` defects unrecorded, `--acl-super-user` separators (Kafka uses `;`), DeleteTopics unknown id. All fixed (branch p8i, SEC-210 to SEC-222, SEC-D80 to D85). An independent transcription of `StandardAuthorizerData` ran 2.13 million checks with 0 disagreements |
| I7 | The re-audit of the fixes (HEAD 693da51), and the rounds after it | done: the re-audit said DONE, both majors verified closed, 33 of 38 mutants killed, and found one minor of the same CLASS as the major (a static member's replacement inherited the departed member's topic state without re-checking the new principal's right). Fixed by sweeping every place member state is produced or inherited (branch p8j, SEC-230 to SEC-238, SEC-D90 to D92; the classic protocol path had the same hole). A final focused audit of that round (HEAD e6d986b) said DONE: a 630,000-step differential of the group machines with no ACLs against the previous commit (0 differences), about 12,000 liveness scenarios with flapping ACLs (all converge, no stranded partition), an independent invariant generator (about 10 million member checks, 0 violations); it found one narrow minor (a classic-only group did not tell a member to rejoin inside the assignment interval) and four test gaps, closed in branch p8k |
| I8 | A CreateAcls of more than 10,000 new ACLs is not limited (Kafka's outcome is unspecified, `kafka-semantics.md` U3) | owner: needs Kafka's real answer, which only a recording gives; a broker of one developer rarely has 10,000 ACLs |
| I9 | DescribeConfigs on the broker does not report `allow.everyone.if.no.acl.found` or `super.users` (the Go edition does; real Kafka lists neither) | moved: with `authz-matrix` (I1); the console's Access page (Phase 9) shows both from the flags regardless |
| I10 | The long text of CLUSTER_AUTHORIZATION_FAILED (`Request(processor=N, connectionId=...) needs ALTER permission.`) is not reproduced | moved: with I1; the code is right and clients read the code |
| I11 | DeleteGroups with the same allowed name twice answers it twice; Kafka de-duplicates first (KA:1431) | owner: a small fix that changes a pinned Phase 5 behaviour (`group_admin`); decide whether to follow Kafka |
| I12 | Client certificates and mTLS principals (`ssl.client.auth`, `ssl.principal.mapping.rules`) | owner: out of scope in the plan ("Not in this phase"); the Go edition has the same limit; build if asked |
| I13 | Re-authentication (KIP-368, `connections.max.reauth.ms`) | owner: out of scope; a logged-in connection stays logged in, and a changed credential applies to new logins |
| I14 | SaslHandshake v0 (raw SASL tokens) | owner: build if a Kafka 1.x client must log in |
| I15 | The 100 ms failed-login delay (`connection.failed.authentication.delay.ms`) | done: built after the first audit, SEC-192 |
| I16 | Zeroing secrets on drop (`zeroize`) | owner: weight versus hygiene; `zeroize` is already in the `tls` build as ring's dependency, so it costs nothing there |
| I17 | The APIs the broker does not serve have no authorization row: `DescribeLogDirs` and the rest of the admin surface outside the Phase 8 matrix, and `DescribeCluster` and the telemetry pair (the rows `version-matrix` still differs on) | partly done (slice 9i, 2026-11-12): DescribeCluster, GetTelemetrySubscriptions and PushTelemetry are served and have their rows (none; DESCRIBE on the cluster only for the operations bits: SEC-290, SEC-291), `version-matrix` and the ten `telemetry-*` scenarios pass, the replay is 58 of 58. Still open and moved: each other API (`DescribeLogDirs` and the rest) joins the matrix, with its test, in the phase that serves it |
| I18 | A build with the `tls` feature on Windows needs the MSVC build tools (ring); not verified, the `windows` CI job compiles the default build only | moved: a Windows developer, or the CI when a Windows job for the feature is wanted |
| I19 | `phase8.md` D1 said "107 to about 121"; the real figure is 122 (8a) and 123 (since 8e, darwin count) | done: corrected in `phase8.md` |
| I20 | The `Access` facts view of the console (principals, mechanisms, the rule that applies) listed under 8g in the plan | moved: Phase 9, with the console |
| I21 | The allowed-hosts guard is a pure function with flags and the start-up rule; nothing is mounted on it | moved: Phase 9 mounts it on the console and the registry; the mount must call `kafka_auth::hostguard::allowed` and answer 403 |
| I22 | Catalogue stale cite found by `tools/check-security-catalogue.ts`: the header of `kafka-server/tests/tls_redaction.rs` cited a rule number past the end of the TLS range (the private-key rule is SEC-152) | done: corrected in the test file's header |
| I23 | Fixes of the first audit of the authentication half (branch `p8h`): SEC-190 to SEC-198, SEC-D70 to D73 (a super user's credential, the open-ACL note, the failed-login delay, file modes, the stranger cap and pool, the clean close, the crypto vectors, no `--sasl-allow-empty-users`, no plaintext kept for SCRAM alone) | done on `p8h`, merge and the re-audit pending |
| I24 | With `--acl-default allow` and SASL on every logged-in user may rewrite the ACLs and manage every user who is not a super user (SEC-D51, SEC-191); the broker says it at start and does not forbid it | decided: the default is the broker's promise to a developer; `--acl-default deny` with ACLs and a super user is the shared-broker posture |
| I25 | A stored credential, the TLS key and the users file are protected by file mode only; the partition logs and the data directory are as the umask makes them; the plaintext of a `--sasl-user` flag stays in the process arguments (`ps`) | owner: the flag's value cannot be hidden from `ps` by the process; the file and variable avoid it (SEC-D58) |
| I26 | Fixes of the re-audit of the authorization half (branch `p8j`): SEC-230 to SEC-238, SEC-D90 to D92 (a static member's inherited topics, names revoked, classic members of a consumer group and a group changing kind, the failed-login cap and the lingering close at a stop, every file mode pinned, the super-user name documented) | done on `p8j`, merge and the next audit pending |
| I27 | Found by the author's replay, not by any audit: the audit fix that capped a stranger's frame at 64 KiB turned the recorded Apache Kafka step `token-of-64k-authenticate` of `sasl-plain` (a 64 KiB token, answered 58) into a closed connection | done: the cap is 128 KiB (a242d28), a connection test pins the 64 KiB token, SEC-194 and SEC-D71 say why. Lesson: every hardening change is re-run against the replay; the agents were told not to run it, so only the author's gate could catch this |
| I28 | Closing findings of the final focused audit of Phase 8 (branch `p8k`): a classic heartbeat in a consumer group answers REBALANCE_IN_PROGRESS when a right is taken, inside the assignment interval too (SEC-231); the audit's liveness and invariant simulations are repo tests (`groups::group::tests::liveness`, `invariants`); the catalogue records four observations (SEC-230 table) and SEC-239, SEC-D93 | done on `p8k`, merge pending |

## Carried into later phases, by name
- **Phase 7 (compaction), done:** the log walks advance by the last offset delta and a stored batch may hold fewer records than it
  spans (`compaction.md` CMP-6); `segment.bytes` and the other topic configs act at runtime (`admin.md` ADM-6); batches are
  rewritten into the topic's `compression.type` at Produce (D4).
- **Phase 8 (security), built:** `idempotent-write-from-topic-write` and the ACLs on the transaction APIs are done; what is left is section I.
- **Phase 9 (console, registry):** the guard is mounted on the console (I21, done in 9a); the Access facts view (I20, slice 9h), DescribeCluster and the telemetry APIs (I17, slice 9i); section J is slice 9a.

## J. Phase 9, slice 9a (the console: the crate, the shell, Home and Connect), built 2026-11-12, audits pending
Statuses as above. The catalogue is `behaviours/console.md` (and SEC-240 to SEC-249 in `security.md`). I21 is done here (the guard is mounted on every console route, SEC-243); I20 moves to slice 9h.

| # | What | Status |
|---|---|---|
| J1 | Home's sentence has no "N apps appear on the wire ... the last request landed N s ago" (REALITY 1): the recorder does not exist. The page says what the broker holds | moved: slice 9e |
| J2 | Home's all-clear does not say "no partition is offline and the data directory is writable" (GROUNDING 2.2): only the transaction rule is checked | moved: slice 9b (`partition_state()`); the writability check is not planned |
| J3 | Home's all-clear has no link to the attention state (a state chosen by data cannot be a link); the example is text | decided |
| J4 | The first-request card ("Waiting for a client id this broker has not seen", the link to `connect-first-request`) and the quiet state's "no clients": no first-request detector, no connection table | moved: slice 9e |
| J5 | The instance card's descriptors are all open files and sockets (the board says "Segments and indexes"); idle memory and threads are read on Linux only, "Not measured" elsewhere | decided (no safe way without `unsafe` or a spawn) |
| J6 | Times are UTC and say so; the boards' times have no zone | decided (no safe, portable way to the local zone) |
| J7 | The rail has no pinned items and no key hints; the search box is drawn disabled (no jump, no palette yet) | moved: slice 9b for the jump to a topic or group; pins are not planned (REALITY 5.5) |
| J8 | Planned routes (`/live`, `/topics`, `/topics/new`, `/topics/{topic}/producers`, `/access`, `/lab`, `/settings`) answer a page that says it is not built yet | moved: the slice that builds each |
| J9 | `connect-advertised` is its own route (`/connect/advertised`) and not a state of `/connect`: its board is a page with its own breadcrumb and a button from Connect | decided |
| J10 | The banner "served beyond loopback, no login" shows when the console is bound beyond loopback, whether or not `--console-unprotected` is given (the banner says what is true; the flag only allows SASL or TLS to start with it) | decided |
| J11 | The stream's ticks name every watched fragment and the fragments answer 304 when unchanged; the ticker is a 2 s floor timer. The recorder's and the group ring's own bumps do not exist | moved: slice 9e |
| J12 | `console.js` is covered by string tests in Rust and a manual Chrome pass; no DOM test of its behaviour (no dependency-free DOM shim) | open: a bun DOM test if a dependency-free shim is found |
| J13 | The advertised-listener dial resolves a name with the system resolver on a blocking thread: a "Check again" pressed repeatedly against a name that hangs holds a blocking-pool thread each until the resolver gives up | decided (dev tool; bounded by the pool) |
| J14 | `tools/boards-snapshot.ts`, `tools/board-diff.ts` and `design/boards/**` are another agent's; this slice writes `target/board-diff/<id>.html` and `design/routes.json` for them | moved: the tooling agent |
| J15 | No CI step runs the `origin` fuzz target (the target exists and `fuzz-check` passes; `ci.yml` is the lead's) | owner |
| J16 | Chrome fidelity pass: remaining differences per board are in the 9a report (the "Reached from" and "Other moments" scaffolding, the dropped rail extras, "MiB" for the board's "MB", the UTC second line) | decided |
| J17 | `weight-console` limits and `weight.md` are not set: the lead sets them from a Linux measurement; the darwin numbers are in the 9a report | owner |
| J18 | A mutation run on `origin.rs`, `format.rs`, `words.rs` and `probe/judge.rs` was not done in this slice | open: before the phase audit |
| J19 | `fixtures` and `boards` are in the library behind the `examples` feature; a release build without the feature has neither | decided |
| J20 | `doze-kafka/src/cli.rs` is 361 lines (soft limit 300; it was 344): the console flags added to it | open: split the flag groups into files when the next flags land |
| J41 | The wire table under a refused produce ("What the wire recorded", board `topic-produce-keyless`) needs the request recorder; the page leaves it out (allow-list entries, `expires: 9e`) | moved: slice 9e |
| J42 | "Size on disk" of the topics list after a delete and the "(7.9 MiB)" figures of the Produced and Delete pages need `partition_files()` (the read builder's `ConsoleView` method): the list cell shows a dash and the sentences give records only | moved: after the merge of p9b-read |
| J43 | The delete dialog's "group reads it with 3 members" and "expire in 3 d 2 h": the protocol gives groups with an offset committed on the topic and their member counts, not who subscribes (that is in member metadata) nor the offsets' expiry (9c); the callout says what is read | decided; 9c for expiry |
| J44 | The Produced page's table lists the record just sent, not the newest five across partitions: the other rows need the record decoder of the read builder (allow-list entry, live render only) | moved: after the merge of p9b-read |
| J45 | `acks` is three radio inputs drawn as the segmented control (the board draws three buttons, which do nothing without a script); allow-list entries, `close` | decided |
| J46 | The Create page's call is the call for what was submitted last, not a live preview "as you type" (a script could do it; the sentence under the title no longer says it does) | open: a few lines of `console.js` |
| J47 | The boards' teaching links to refusal states ("see that refusal") are not offered: a state needs a request, and a GET never changes anything (allow-list entries, `close`) | decided |
| J48 | The console is a super user of every broker (SEC-241), so the authorization refusal pages are exercised through `Backend::acting_as` (a test seam), not by the real console; the pages and wording are tested | decided (D5) |
| J49 | A keyless record sent with the partition left to the key goes to partition 0 (Kafka's client uses a sticky partitioner for it); a keyed one goes where `murmur2` puts it, tested on Kafka's own vectors | decided |
| J50 | Rows for `/groups`, `/topics/{topic}/log` and `/topics/{topic}/config` are Planned pages so the Produced page's tabs resolve; `topic-producers` joins the producers row. The read builder adds the same rows: dedupe at the merge | open: merge |
| J51 | `Backend::acting_as` and `Backend::recording` are `doc(hidden)` seams for the tests (a caller that is not the console; a log of the APIs called); a real console never sets either | decided |
| J52 | The created page says "Nothing has written to it yet" (checked with ListOffsets) and does not claim "no group reads it" | decided |
| J53 | "Check only" has no board of its own: its answer is the review step with a banner ("Checked."); a refusal of the check is the refused state | decided |
| J54 | Chrome side-by-sides were looked at for `topic-create`, `topic-produce` (dialog over a blank page: the board draws it over its parent) and `topic-deleted` only; the other eleven were checked by structure (board-diff, both renders) | open: the independent look of the slice gate |
| J55 | A form body is read for 64 fields (`form::FIELD_LIMIT`); the Produce dialog offers 8 header rows | decided |
| J56 | The dialog opening in `console.js` has string tests and a manual look only (as J12) | open |

## J (continued). Phase 9, slice 9b, the reading half (`p9b-read`: the topics list, a topic with its Messages, a record, partitions, offline), built 2026-11-12, audits pending
Statuses as above. Rules CON-100 and up in `behaviours/console.md`. The writing half has its own rows.

| # | What | Status |
|---|---|---|
| J21 | The "On the wire" dock of the topic page, "What the wire recorded" of the offline page, and the button and link to `topic-wire` are not drawn (REALITY 1, the recorder); allow-list entries expire with 9e | moved: slice 9e |
| J22 | The record page's "Copy as kcat" is "Copy value" (and "Copy as hex" for bytes): a command line built from a hostile value is a shell-injection risk, so the page copies the text and builds no command | decided |
| J23 | The list rows and the topic page say "read by <groups that committed offsets>" and never "written by <client>" (REALITY 5.4, the recorder); "read by" is groups with a committed offset, so a group that reads without committing is not listed | moved: slice 9e for "written by" |
| J24 | The list rows have no record counts: ListOffsets opens every partition it is asked about, so counting for a list would open (and flush, on eviction) every partition to draw a table. The topic page has the count (ListOffsets on one topic) | decided |
| J25 | The offline page says which partition and why (the writer's recorded cause) and not when: the broker keeps the first failed flush's text and not its time. The live test cannot seed an offline partition (the console holds `Broker<RealFs>`); the facts are tested in `kafka-storage` and `kafka-broker` and the page is rendered from the fixture | decided |
| J26 | The tab counts are "Consumers N" = groups with a committed offset on the topic and "Producers N" = producers active on its partitions (DescribeProducers: they expire); neither is a member count | decided |
| J27 | "Older" shows twice as many records (up to 200) and is hidden at the cap; there is no cursor into the past | open: a cursor (`before`) if a person needs more than 200 |
| J28 | A key search reads the partition backwards from its end in windows of 1,000 offsets until enough records match or 8 MiB (16 requests a window) were looked at, and says how far it looked; the board's help text ("reads from the start of the partition") is reworded to say that. A time goes to ListOffsets, which the broker answers by walking the log | decided |
| J29 | An ACL refusal cannot be shown as the console, which is a super user (SEC-241); the test acts as another principal through `Backend::acting_as`, compiled only for tests and the `examples` feature | decided |
| J30 | `__consumer_offsets` is a constant row (Metadata never lists it; "50 partitions" is the broker's group-slot count): drawn on "Show internal" and in the callout, said to be no log | decided |
| J31 | The boards' navigation between states (the help line with links to the filtered and empty list, "What an offline partition looks like", "Its dead letter topic, by name") is canvas scaffolding and is not drawn; the allow-list entries say so | decided |
| J32 | A record is a page of its own at `/topics/{topic}/messages/{partition}/{offset}`, not a drawer over its parent: a direct address works with no script and the board's "Close" is a link back; the drawer's behaviour over a dimmed parent is not built | decided |
| J33 | An empty state's title is an h2 (the canvas has an h3 straight under the h1: `tools/test/a11y-known.json`); `design/console.css` styles both alike and the allow-list carries the heading difference | decided |
| J34 | Values over 64 KiB are shown to 64 KiB (hex to 16 KiB) with the true size; a value that is cut is not recognised as JSON; Avro and Protobuf are not decoded (Phase 10) | decided |
| J35 | The read_committed page's "Who this holds back" has no group lag (the board names the group payments-workers and its lag: slice 9c); it says where a read_committed consumer stops and which transaction holds it | moved: slice 9c |
| J36 | "Size on disk" is the bytes of the segments' log files; the index files are not counted | decided |
| J37 | A mutation check by hand on `gather::walk::Aborts::reach`, `gather::decode::text_of` and `model::query::whole` was made at the slice close (the changes and what failed are in the report); a mutation run over the whole of `gather/`, `words/topics.rs` and `format/read.rs` was not made | open: before the phase audit |
| J38 | The live board-diff seeds an `orders` topic whose rows are not the boards' (the live render is compared on structure only); the fixture render carries the boards' numbers | decided |
| J39 | The stream's ticker for the topic pages is the 2 s floor timer until the recorder bumps it per request (slice 9e); the pages are right without it and a fragment is a 304 when nothing changed | moved: slice 9e |
| J40 | `Source` gains one file of methods (`source/topics.rs`) and the stream vocabulary three lists (`HOME_IDS`, `TOPICS_IDS`, `TOPIC_IDS`); the four Planned rows after `/topics/new` in `routes.rs` (produce, partitions/add, delete-records, delete) stand in for `p9b-write`'s handlers until the branches are merged | open: the lead's merge |
