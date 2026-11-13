# Behaviours: the Lab (fault injection at the protocol edge)

What the console's Lab does to the broker, and the rules it keeps. The engine is `kafka-server::fault` (rules, matchers, the table, per-API
answers) and the hook in `kafka-server/src/connection/exchange.rs`; the three forced rehearsals are `kafka-broker::lab` and the group machines;
the console side is `kafka-console::lab` and `gather::lab`. Design and the Go edition's Lab: `docs/port/phase9/lab.md`. The pages that draw the
twelve Lab boards come with the React rebuild of the console (the Rust server answers JSON over the view-models below); this file is the
contract those pages stand on. Console-side rules are CON-930 to CON-939 in `console.md`.

### LAB-1. A rule is bounded in count, time and size
At most 50 rules at once; a rule lasts 1 second to 24 hours; a delay is 1 to 60,000 ms (and jitter the same); a share is every request, every
Nth (2 to 1,000) or a random 1 to 99 %; at most 16 topics and 64 partitions; a name is at most 80 characters with no control character. A rule
past a bound is refused with a typed reason (`AddError`) and never made. The 51st rule is refused in words.
Test: `a_rule_is_checked_against_every_bound`, `rules_are_bounded_at_fifty_and_a_replacement_does_not_count_twice`, `hostile_topic_and_client_patterns_are_kept_as_text_and_bad_fields_are_said`

### LAB-2. The history is bounded and rules never outlive the process
Rules that are over are kept (the last 200); the table is in memory and a restart clears it. No Kafka API, file or flag makes a rule.
Test: `the_history_is_bounded`

### LAB-3. A matcher fits when every field that is set fits
Client id (exact or a glob of `*` and `?`), principal (exact), APIs, topics (exact or glob) and partitions (a partition belongs to the topic it
is named under), a group for the group APIs, and connections numbered up to a limit. A field left empty fits anything; a topic rule does not
fit a request that names no topic. A Fetch or Produce from v13 names its topics by id: the broker's name for the id is used.
Test: `each_field_must_fit_and_a_field_left_empty_fits_anything`, `a_partition_belongs_to_the_topic_it_is_named_under`, `a_topic_rule_does_not_fit_a_request_that_names_none`, `a_group_rule_fits_group_apis_by_their_group`, `a_pattern_fits_by_its_stars_and_question_marks`

### LAB-4. A pattern cannot make a match slow
The glob walk is linear in the two lengths, with one backtrack point.
Test: `a_hostile_pattern_is_linear_not_exponential`

### LAB-5. The console is never bent unless a rule names it
The console's own calls do not pass through the connection loop at all. A connection whose principal is the console's is left alone by every
rule that does not name that principal.
Test: `the_console_is_never_bent_by_a_rule_that_does_not_name_it`, `the_console_is_touched_only_by_a_rule_that_names_it`

### LAB-6. Logins are never touched
SaslHandshake and SaslAuthenticate are answered by the server, not the broker, and the plan is not asked about them. A rule that matches
everything leaves a login alone.
Test: `a_rule_that_matches_everything_does_not_touch_the_login`

### LAB-7. Shares are deterministic or seeded
Every Nth fires on the Nth matching request. A random share draws from a seeded generator: the same seed, the same requests.
Test: `every_nth_fires_on_the_nth_matching_request`, `a_percent_share_is_the_seeds_and_close_to_the_share`, `jitter_is_added_within_its_bound_and_is_the_seeds`

### LAB-8. Rules end by themselves
A rule ends when its time is up (noticed on the next look at the table, but dated when it was due), or when it has fired its allowed number of
times. A rule switched off does nothing and still runs out. Ended rules are listed with why.
Test: `a_rule_ends_when_its_time_is_up_and_is_kept_as_ended`, `a_rule_ends_when_it_has_fired_as_often_as_allowed`, `a_rule_that_is_switched_off_does_nothing_and_still_runs_out`

### LAB-9. Two rules on the same requests: one wins
A rule with the same matcher and kind of effect replaces the old one. When several fit and fire, a drop wins over a fail, a fail over a delay,
and the longer delay over the shorter; only the winner counts a hit.
Test: `the_worse_effect_wins_and_a_longer_delay_beats_a_shorter`, `rules_are_bounded_at_fifty_and_a_replacement_does_not_count_twice`

### LAB-10. A rule remembers little
Its hits, the requests that fitted it, the first request it fired on and the latest, and at most 8 client names (the rest counted).
Test: `what_a_rule_remembers_of_clients_is_bounded`, `a_rule_acts_on_what_it_fits_and_counts_matches_and_hits`

### LAB-11. A failed request is answered in the shape of its API and never reaches the broker
The answer carries the code in every place a client of the version reads one (Produce and Fetch per partition, Metadata per topic and from v13
on top, ListOffsets, OffsetCommit and OffsetFetch per partition and group, FindCoordinator, JoinGroup, SyncGroup, Heartbeat,
ConsumerGroupHeartbeat, InitProducerId). Every such answer encodes at every version served. A Produce that asked for no acknowledgement is
answered nothing. A rule is not made for an API the Lab cannot answer, nor for an error code Kafka has no name for.
Test: `the_code_is_in_every_place_a_client_reads_one`, `every_failable_answer_encodes_at_every_version_the_broker_serves`, `an_api_the_lab_cannot_answer_is_refused_not_guessed`, `a_fail_rule_answers_with_its_code_and_the_broker_never_sees_the_request`, `a_fail_rule_answers_produce_with_its_code_and_writes_nothing`

### LAB-12. A slow rule delays the answer after the broker has made it
The request is served; the answer waits. It is the total the wire shows. A stop of the server does not wait for the delay.
Test: `a_slow_rule_delays_the_answer_by_its_delay_and_tags_the_request`, `stopping_the_server_does_not_wait_for_a_delayed_answer`, `a_slow_rule_delays_a_fetch_and_the_wire_says_so`

### LAB-13. A drop closes the connection with no answer
Also at ApiVersions, so a rule by client closes each new connection at its first request. "Drop once" closes only the connections that were
open when it was made (a limit on the connection number, and a number of hits equal to how many were open), at their next request.
Test: `a_drop_rule_closes_the_connection_and_the_next_one_is_served_when_the_rule_is_gone`, `a_drop_rule_by_client_closes_a_new_connection_at_its_first_request`, `a_drop_once_closes_only_the_connections_that_were_open_when_it_was_made`, `drop_once_closes_the_connections_that_were_open_and_not_the_next_one`

### LAB-14. The wire marks what a rule touched
The observer is told which rule touched a request and what it did; the recorder keeps the rule's number and name on the row, never folds such a
row, and says "includes +N ms" under a delayed row's time.
Test: `a_slow_rule_delays_a_fetch_and_the_wire_says_so`, `a_fail_rule_answers_produce_with_its_code_and_writes_nothing`

### LAB-15. A forced expiry is the session timer's step
`Event::Force(Expire)` runs the function the session timer runs, without the check that spares a member with a request waiting; a parked
JoinGroup or SyncGroup of the member is answered UNKNOWN_MEMBER_ID. For the new protocol it is the silent-member removal. The result is what
the timer would have left: members, state, the note naming the cause SessionExpired.
Test: `a_forced_expiry_leaves_the_group_as_the_session_timer_does`, `a_member_with_a_join_waiting_is_answered_when_it_is_expired`, `a_forced_expiry_removes_the_member_as_a_missed_session_does`, `an_expired_member_and_a_forced_rebalance_go_through_the_groups_own_machine`

### LAB-16. A forced rebalance is the membership change's step
Classic: the group begins a rebalance as when a member joins or leaves, with the cause Forced; a group with no member or already rebalancing
is refused with a sentence. New protocol: the epoch moves by one through the path a member's own change takes. No other transition exists.
Test: `a_forced_rebalance_makes_the_members_join_again_and_says_it_was_forced`, `a_forced_rebalance_of_an_empty_group_is_refused`, `a_forced_rebalance_moves_the_epoch_by_one_and_says_it_was_forced`, `a_forced_rebalance_moves_a_consumer_group_to_the_next_epoch`

### LAB-17. Compact now is the cleaner's real round, behind the planner
`clean_now` runs the cleaner's own round on one partition. Refused with a sentence when the topic does not compact, nothing has been written, the
cleaner is off, or the planner finds nothing due (the planner's verdict and the ratio against the threshold are shown first).
Test: `compact_now_runs_the_real_round_and_refuses_with_a_sentence_otherwise`

### LAB-18. The broker checks a right before each forced step
Expire and rebalance need DELETE on the group, Compact now ALTER on the topic; the console is a super user, and a caller of the test seam without
the right meets Denied.
Test: `a_caller_without_the_right_is_refused_by_the_broker`

### LAB-19. Faults never reach storage
A failed Produce appends nothing; a delayed one is applied; a dropped one is a closed socket. No rule is applied below the connection loop.
Test: `a_fail_rule_answers_with_its_code_and_the_broker_never_sees_the_request`, `a_fail_rule_answers_produce_with_its_code_and_writes_nothing`

### LAB-20. With no plan, a request costs one Option test; with no rule, one relaxed load
`--lab=false` installs no plan. Measured with `cargo run --release -p kafka-console --example produce_bench`: Lab on with no rule, and with one
rule that fits nothing, are within the noise of no observer (see `docs/port/phase9/lab.md`).
Test: `a_table_with_no_rule_lets_everything_through_and_takes_no_lock`

### LAB-21. `--lab` is on by default and off by default wherever the console is open
On unless `--lab=false`; off with the console; off by default whenever the console is served beyond loopback, whatever guards the broker (since the
2026-11-13 console security audit; before, only with SASL or TLS on). `--lab` (or `--lab=true`) switches it on there, and the start then says anyone
who can reach the console can inject faults; left off, the start says `the Lab is off because the console is served beyond loopback (--lab=true switches
it on)`. The Access page says it too.
Test: `the_lab_is_on_by_default_and_off_with_its_flag_or_with_the_console`, `a_console_open_to_other_machines_has_the_lab_off_unless_asked_and_then_says_it_can_inject_faults`, `a_protected_broker_with_an_open_console_has_the_lab_off_unless_asked_and_never_without_unprotected`, `tests/process console::an_open_broker_on_the_network_serves_its_console_there_with_the_lab_off_unless_asked`

### LAB-22. The Lab teaches what it rehearses
Every rehearsal has its lesson (what it stands for in production, what to expect, what a well-behaved client does, the settings
with Kafka's defaults, where to see it) and the page has recipes that name the broker's own topics, apps and groups; Stop all
ends every rule in force at once. The words are one module (`crate::lab::teach`), the recipes another (`crate::lab::recipes`),
served by `GET /api/v1/lab/guide`; the console's rules are CON-1370 to CON-1375.
Test: `every_rehearsal_is_taught_once_in_the_order_of_the_tabs`, `recipes_name_the_brokers_own_topics_apps_and_groups`, `stop_all_ends_every_rule_and_live_filters_by_the_lab_tag`
