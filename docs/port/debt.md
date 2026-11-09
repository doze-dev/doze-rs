# Debt register (Phases 0 to 6)

Opened 2026-11-09 at the owner's instruction: nothing known is carried past Phase 6. Every item is cleared (with a test
that fails when the behaviour is removed), or is blocked by a later phase's feature and moves into that phase's plan with
a named scenario, or is the owner's decision. Status: `open`, `done <commit>`, `moved` (blocked, see the note), `decide`.

## A. Phase 6 audit
| # | Item | Status |
|---|---|---|
| A1 | Findings of the independent audit of Phase 6 (and its re-audit) | open (audit running) |

## B. Phase 5 (groups, KIP-848)
| # | Item | Status |
|---|---|---|
| B1 | Test: cooperative `owned` retain in a classic join into a consumer group | open |
| B2 | Test: classic SyncGroup "group moved on" check | open |
| B3 | Test: `hand_over` disarms the rebalance timer and uses the previous assignment during a completing rebalance | open |
| B4 | Test: `hand_back` disarms timers | open |
| B5 | Test: registry retries a join queued behind a deletion | open |
| B6 | Test: `end_revocation_if_given_up` | open |
| B7 | OffsetFetch v9 ignores the member epoch (Kafka checks it) | open: find the recording or read the spec, then build |
| B8 | Words of a consumer heartbeat to a non-empty classic group; "RackId can't be empty." | open: unrecorded, check the recordings, else name what is guessed |
| B9 | Topic snapshot rebuilt on every heartbeat | open |
| B10 | Member cap checked after a conversion; record rewritten on every classic join into a consumer group | open |
| B11 | Rack-aware assignment (rack ids are kept, not used); heartbeat and assignment intervals only a test knob | decide |

## C. Phase 4 (groups, classic)
| # | Item | Status |
|---|---|---|
| C1 | Peer address in DescribeGroups is a fixed `/127.0.0.1` | open |
| C2 | A SyncGroup or Heartbeat queued behind a group's deletion is dropped (connection closed); OffsetCommit answered UNKNOWN_MEMBER_ID | open |
| C3 | Issued-id table has one global cap (one client can use it all); an id lasts five minutes, not the session timeout | open |
| C4 | librdkafka and FindCoordinator note owed from Phase 3 | open: interop runs librdkafka and kcat; record the finding |

## D. Phases 2 and 3 (storage, minimum broker)
| # | Item | Status |
|---|---|---|
| D1 | Sealed segments of an open partition hold two descriptors each (lazy open not done) | open |
| D2 | A read asks the file for `max_bytes` plus 16 KiB before the limit is known (7.5 MiB pinned for 8 KiB measured); Phase 6 was to clamp it by the last stable offset | open: check what Phase 6 did |
| D3 | Go behaviours not built: a closed partition's bounds without opening it, `segment.bytes` at runtime, `max_bytes` of 0 | open: decide each against the recordings |
| D4 | Rewriting a batch into the topic's `compression.type` | moved: needs topic configs (Phase 7) |

## E. Phase 6
| # | Item | Status |
|---|---|---|
| E1 | `endtxn-while-preparing` is a table test, not a replay of the recording | open |
| E2 | `full-disk-*`: a `SimFs` out-of-space test for the transaction path (marker write, record write) | open |
| E3 | The transaction interop scenarios take 18 s each; find out why | open |
| E4 | KIP-890 transaction v2 (AddPartitionsToTxn v4) is not offered | decide |
| E5 | `idempotent-write-from-topic-write` | moved: needs ACLs and SASL (Phase 8) |

## F. Process
| # | Item | Status |
|---|---|---|
| F1 | kafka-wire two-sided test diet | open |
| F2 | Private GitHub repository | decide |
| F3 | Java client | closed by the owner (Go clients only) |
