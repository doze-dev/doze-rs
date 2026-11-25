# Behaviours: the AWS Lab's fault rules (fault injection at the gateway)

What the console's Lab does to the calls apps send to doze-aws, and the rules it keeps. The hook is `aws-gateway::fault` and the four
protocol paths (`pipeline.rs` for JSON, `query.rs`, `cbor.rs`, `rest.rs`) with the drop in `server.rs`; the table, the matcher, the error
codes and the lessons are `aws-console::lab`; the API is `aws-console::api::lab_rules` (`lab_build.rs` reads a rule's words against this
build's services); the tag on a Live row is `aws-console::ring` and `model::LabTag`; the switch is `doze-aws/src/lab.rs`. Design and what
was decided where the design was silent: `docs/port/aws/lab.md`. The Kafka Lab's rules (`behaviours/lab.md`, LAB-1 to LAB-22) are the model;
these are numbered AWS-LAB-1 and up. Forced events (`lab_once`) are another part and are not here.
Where the tests are: `aws-gateway/tests/fault_verdicts.rs` and `fault_slow_drop.rs` (the hook with a scripted table and a fake SQS, shared in `tests/fault/mod.rs`); `aws-console/src/lab/**/tests*.rs` (the table,
rules, glob, codes, words, lessons); `aws-console/tests/lab_wire_json.rs`, `lab_wire_query.rs`, `lab_wire_others.rs` and `lab_wire_lambda.rs` (a real gateway with real SQS,
SNS, S3, CloudWatch and Lambda behind it, shared in `tests/lab_wire/mod.rs`: the four protocol paths and the REST JSON one);
`aws-console/tests/lab_api_rules.rs` and `lab_api_more.rs` (the console API, shared in `tests/lab_api/mod.rs`); `doze-aws/tests/lab.rs` (the binary);
`console/apps/aws/e2e/tests/live-lab.spec.ts` (Live in a browser with the real AWS CLI). A name below with no file before it is in the file the last
`tests/...::` named.

### AWS-LAB-1. A rule is bounded in count, time and size
At most 50 rules at once; a rule lasts 1 second to 24 hours; a delay is 1 to 60,000 ms (and jitter 0 to 60,000); a share is every call, every
Nth (2 to 1,000) or a random 1 to 99 %; a rule names at most 16 services, 16 operations, 16 resource patterns, 8 regions and 8 access keys;
a name is at most 80 characters with no control character; a pattern at most 256 bytes; a custom error code is 1 to 64 letters, digits and
dots and its status 400 to 599. A rule past a bound is refused with a typed reason (`AddError`: its field and a sentence), never made and
never cut. The 51st rule is refused in words.
Test: `lab::rule::tests::a_rule_is_checked_against_every_bound`, `a_delay_and_its_jitter_are_bounded`, `a_custom_error_has_an_error_status_and_a_plain_code`, `a_matcher_is_bounded_in_every_list`, `hostile_words_are_said_and_left_out`, `every_refusal_says_its_field_and_what_to_do`, `lab::table::tests::rules_are_bounded_at_fifty_and_a_replacement_does_not_count_twice`, `tests/lab_api_*::the_51st_rule_is_refused_in_words_and_a_replacement_is_not_counted_twice`, `tests/lab_api_*::what_is_wrong_with_a_rule_is_said_field_by_field`

### AWS-LAB-2. The history is bounded, rules never outlive the process, and only the console makes one
Rules that are over are kept (the last 200); the table is in memory and a restart clears it. No AWS API, file, flag or environment variable
makes a rule: the only way is the console's API, behind its same-origin proof like every write. History is forgotten on request.
Test: `lab::table::tests::the_history_is_bounded`, `tests/lab_api_*::a_rule_is_made_listed_switched_ended_and_forgotten`, `tests/lab_api_*::a_write_asks_for_the_same_origin_proof_like_every_write`

### AWS-LAB-3. A matcher fits when every list that is set fits
Services (by signing name or SDK id, in any case; the console names such as `stepfunctions` are read when the rule is made), operations
(exact), regions, access keys, and resource patterns against what the call names. A list left empty fits anything. A resource pattern
fits the whole text a call names (a queue URL, a table, an ARN, an S3 `bucket/key`) or its last name (`orders` of a queue URL, the file of
a key); a call that names nothing does not fit a rule that has patterns. `dynamodb` fits DynamoDB and its Streams, which sign alike; the
SDK id `DynamoDB Streams` fits only the Streams. Each list is sorted and without repeats, so two rules for the same calls are equal.
Test: `lab::rule::tests::each_field_must_fit_and_a_field_left_empty_fits_anything`, `a_resource_pattern_fits_the_whole_text_or_its_last_name`, `a_resource_rule_does_not_fit_a_call_that_names_no_resource`, `two_services_that_sign_alike_are_told_apart_by_their_sdk_id`, `a_matcher_is_the_same_whatever_order_and_case_its_words_came_in`, `tests/lab_wire_*::a_rule_that_names_a_resource_touches_only_what_it_names`

### AWS-LAB-4. A pattern cannot make a match slow
The glob (`*` any run, `?` any one character, nothing escaped) is walked with one backtrack point, over characters, in at most
pattern x name steps, with no recursion. A differential test checks it against a plain table on every pattern and name of up to five and
six characters over a small alphabet; `lab_glob` is a fuzz target with the same oracle and `lab_body` fuzzes a rule's body.
Test: `lab::glob::tests::a_hostile_pattern_is_linear_not_exponential`, `the_walk_agrees_with_a_plain_table_on_every_small_pattern_and_name`, `a_pattern_fits_by_its_stars_and_question_marks`, `a_question_mark_fits_a_character_not_a_byte`

### AWS-LAB-5. What is not judged, and the console is never bent
The hook sits after a request is decoded and before the service is asked. Not judged: `/_doze/health`, the `/_doze/` local hooks, function
URLs and API Gateway stages (not AWS API calls), the console's own calls, a Lab "Send a call", and calls between services (they go over
the bus, not through the gateway). A request refused before it was decoded (a body the model refuses) is the service's validation error,
as AWS answers it: no rule is asked about it.
Test: `aws-gateway/tests/fault_*::a_request_the_pipeline_refused_before_it_was_decoded_is_not_judged`, `a_request_is_judged_with_what_the_pipeline_learnt_of_it`

### AWS-LAB-6. Shares are deterministic or seeded
Every Nth fires on the Nth call that fits the rule. A random share draws from a seeded splitmix64: the same seed and the same calls give
the same hits, and so does the jitter, which stays within its bound.
Test: `lab::table::tests::every_nth_fires_on_the_nth_matching_request`, `a_percent_share_is_the_seeds_and_close_to_the_share`, `jitter_is_added_within_its_bound_and_is_the_seeds`

### AWS-LAB-7. Rules end by themselves
A rule ends when its time is up (noticed on the next look at the table, no timer anywhere, but dated when it was due), or when it has fired
its allowed number of times. A rule switched off does nothing, counts nothing, and still runs out; an ended rule cannot be switched on or
removed again. Ended rules are listed with why (expired, spent, cleared, replaced).
Test: `lab::table::tests::a_rule_ends_when_its_time_is_up_and_is_kept_as_ended`, `a_rule_ends_when_it_has_fired_as_often_as_allowed`, `a_rule_that_is_switched_off_does_nothing_and_still_runs_out`, `stop_all_ends_every_rule_at_once`, `tests/lab_wire_*::a_client_that_retries_as_the_sdks_do_gets_through_when_the_rule_has_run_out`

### AWS-LAB-8. Two rules on the same calls: one wins
A rule with the same matcher and the same kind of effect (slow, fail, drop; a throttle replaces an unavailable) replaces the old one, which
is kept as replaced and does not count twice against the limit. When several fit and fire, a drop wins over a fail, a fail over a delay and
the longer delay over the shorter; only the winner counts a hit (every rule that fitted counts the match).
Test: `lab::table::tests::the_worse_effect_wins_and_a_longer_delay_beats_a_shorter`, `rules_are_bounded_at_fifty_and_a_replacement_does_not_count_twice`, `a_rule_acts_on_what_it_fits_and_counts_matches_and_hits`

### AWS-LAB-9. A rule remembers little
Its matches and hits, the first call it fired on (when, key, service, operation, what it named, the client), when the latest was, and at most
8 access keys (the hits of the rest are counted). Numbers are never reused.
Test: `lab::table::tests::what_a_rule_remembers_of_callers_is_bounded`, `a_rule_remembers_the_first_and_the_last_call_it_fired_on`, `a_rule_gets_a_name_when_it_is_given_none_and_numbers_are_never_reused`

### AWS-LAB-10. A failed call is answered in the shape of its protocol and never reaches the service
The path's own error renderer words it, so it has the protocol's shape and carries its code where an SDK reads it: JSON (a `__type`, and
`x-amzn-query-error` for SQS's JSON with its Query-compatible codes), Query XML (`<ErrorResponse>`, the Query code, `Sender` or
`Receiver`), CBOR (a CBOR map with the Smithy headers), S3's XML (`<Error>` with the request id and `Server: AmazonS3`) and REST JSON
(`x-amzn-ErrorType`). The service is never asked: a failed write writes nothing, whatever the protocol.
Test: `aws-gateway/tests/fault_*::a_fail_answers_in_the_protocols_shape_and_the_service_never_sees_the_request`, `a_fail_with_a_code_the_model_does_not_carry_states_its_own_status_and_code`, `tests/lab_wire_*::a_json_fail_answers_in_the_protocols_shape_and_writes_nothing`, `a_query_fail_answers_in_xml_with_the_models_code_and_writes_nothing`, `a_query_service_words_each_kind_of_failure_its_own_way`, `a_cbor_service_is_throttled_slowed_and_dropped_and_a_fail_writes_nothing`, `a_rest_service_is_throttled_slowed_and_dropped_and_a_fail_writes_nothing`, `a_rest_json_service_words_a_failure_with_its_error_type_header_and_its_own_status`

### AWS-LAB-11. The error codes are one table per service, and each is verified
`lab::codes` holds, for each of the 21 APIs of the build, what it answers a throttle, an unavailable, an internal error and an access denied
with. A modelled answer names a shape of the service's vendored model, and a test checks that it is there with the status (and the wire or
Query code) the model gives; a plain answer is one of the Common Errors every service's API Reference lists, or S3's own (`SlowDown`,
`InternalError`). A test checks that every throttle is one SDKs retry as a throttle (botocore's list, or status 429), every internal error is a
500 and every unavailable a 503, and that a denial is none of those. Sources are in the module's text. A code the table cannot verify is not in
it: a custom failure is the way to ask for one.
Test: `lab::codes::tests::every_shape_a_row_names_is_in_its_model_with_the_status_and_code_the_model_gives`, `a_throttle_is_one_every_sdk_retries_as_a_throttle`, `an_internal_error_is_a_500_and_an_unavailable_a_503_which_every_sdk_retries`, `an_access_denied_is_a_code_no_sdk_retries`, `every_service_of_the_build_has_its_row_and_no_row_is_for_a_service_that_is_not_there`, `two_apis_that_sign_alike_each_have_their_own_row`, `a_service_the_table_does_not_know_answers_the_common_errors`

### AWS-LAB-12. A slow rule waits before the service is asked, and a stop does not wait
The request is decoded and judged, waits its delay (and jitter), and is then served as usual; the time Live shows includes the wait. A stop
of the server does not wait for it, and a client that gives up while the call waits is never served (the connection is gone: here the
service never did the work, where in production it might have).
Test: `aws-gateway/tests/fault_*::a_slow_request_waits_and_is_then_served_and_the_observer_is_told`, `stopping_the_server_does_not_wait_for_a_slow_request`, `a_client_that_gives_up_during_the_wait_is_never_served`, `tests/lab_wire_*::a_json_slow_call_is_served_after_its_wait_and_its_row_says_it_waited`, `a_query_drop_and_a_query_slow_call`

### AWS-LAB-13. A drop closes the connection with no answer
The connection is closed after the call was read and judged, with not a byte of answer; the service never saw the call, so nothing was
written; the next connection is served once the rule is gone. A dropped call has no HTTP status: its Live row says status 0, outcome error,
and `NO ANSWER`.
Test: `aws-gateway/tests/fault_*::a_drop_closes_the_connection_with_no_answer_and_the_service_never_sees_it`, `the_next_connection_is_served_when_the_rule_is_gone`, `tests/lab_wire_*::a_json_drop_closes_the_connection_with_no_answer_and_writes_nothing`, `tests/lab_api_*::live_tags_the_calls_a_rule_touched_and_filters_by_the_rule`, `api::types::a_dropped_call_is_an_error_and_a_denial_in_snss_words_is_a_denial`

### AWS-LAB-14. The wire marks what a rule touched
The observer is told which rule touched a call and what it did (`Finished::lab`: the rule's number, name and `Done`); the Recorder keeps the
number and what was done on the row (8 bytes a row, so the whole ring is still under a mebibyte at its worst), never folds such a row, and
the API sends `lab: {id, name, effect}` with the name read from the table (`Lab rule N` once the table has forgotten the rule).
`GET /api/v1/calls?lab=<rule>` keeps one rule's calls and `lab=any` those of the Lab, oldest first. Live draws the tag, filters by it, says a
slowed call's time includes the wait, and shows each try of a bent client as a row of its own.
Test: `ring::tests::a_call_a_rule_touched_keeps_the_rule_and_is_found_by_it`, `the_bytes_held_are_counted_and_never_pass_the_bound`, `tests/lab_api_*::live_tags_the_calls_a_rule_touched_and_filters_by_the_rule`, `tests/lab_wire_*::a_json_fail_answers_in_the_protocols_shape_and_writes_nothing`, `doze-aws/tests/lab::a_rule_made_in_the_console_bends_the_calls_to_the_port_until_stop_all`; the browser: `e2e/tests/live-lab.spec.ts`, `pages/live/search.test.ts`, `live-model.test.ts`

### AWS-LAB-15. A call goes through the rules the same on every path
JSON (every operation, event streams too), Query (the unsigned STS operations too), CBOR and REST (S3, Lambda, API Gateway's management API) each
call the same `fault::apply` at the same point: after the input is decoded and noted, before the service. The rules judge a call even when no
observer is installed.
Test: the four `tests/lab_wire` families above; `aws-gateway/tests/fault_*::the_rules_judge_a_request_with_no_observer_too`

### AWS-LAB-16. With no table a request costs one `Option` test; with no rule, one relaxed load
`--lab=false` (or `--no-console`) installs no table. With the table and no rule switched on a call asks `Faults::armed`, one relaxed load,
and is never described to the rules. Measured (`cargo run --release -p aws-console --example lab_bench`, one keep-alive connection, SQS
`ListQueues` through the gateway, 2026-11-26): about 24.5 to 25.3 microseconds a request with the recorder only, the table with no rule, one
rule that fits nothing or 50 that fit nothing alike, within the round-to-round noise of about 1 microsecond; `armed` 0.2 ns, `judge` 2 ns with
no rule, 25 ns with one rule that fits nothing, 123 ns with 50.
Test: `lab::table::tests::a_table_with_no_rule_lets_everything_through_and_takes_no_lock`, `aws-gateway/tests/fault_*::a_table_with_nothing_armed_is_never_asked`

### AWS-LAB-17. `--lab` is on by default and off by default wherever the console is open
On unless `--lab=false` (`true`, `false`, `on`, `off`, `yes`, `no`, `1`, `0`; bare `--lab` is on; the value takes an equals sign; also
`DOZE_AWS_LAB` and `lab` in `doze-aws.toml`). Off with the console (`--no-console`: asked for plainly it says so). Off by default whenever the
console is served beyond loopback, and `--lab=true` switches it on there with a start-up line that says anyone who can reach the console can
inject faults. The console is loopback-only today (`--console` refuses another address), so the rule is a tested function waiting for the day
it is not. `GET /api/v1/lab/rules` says `enabled: false` and why; a write answers 409 `LAB_OFF`.
Test: `doze-aws::lab::tests::the_lab_is_on_by_default_and_off_with_its_flag_or_with_the_console`, `a_console_open_to_other_machines_has_the_lab_off_unless_asked_and_then_says_it_can_inject_faults`, `cli_tests::the_lab_flag_is_on_alone_and_takes_its_word_with_an_equals_sign`, `tests/lab::a_rule_made_in_the_console_bends_the_calls_to_the_port_until_stop_all`, `with_lab_false_the_console_has_no_rules_to_make_and_the_port_is_never_bent`, `a_bare_lab_flag_is_on`, `tests/lab_api_*::with_the_lab_off_nothing_can_be_made_and_the_answer_says_why`

### AWS-LAB-18. The Lab teaches what it rehearses
Each of the six rehearsals (throttle, unavailable, internal error, access denied, slow, drop) has its lesson: what it stands for in production,
what to expect, what a well-behaved client does, the SDK settings with their real defaults (boto3 and the AWS CLI, JavaScript v3, Java 2.x, Go
v2: retry mode, attempts, backoff, connect and read timeouts; a default that could not be confirmed has no number), and where to see it. The
words are one module (`lab::teach`, with the sources of every number in `lab/teach/settings.rs`) and are served by `GET /api/v1/lab/guide`
whether or not the Lab is on. The guide has no recipes: the page makes them from the person's own resources.
Test: `lab::teach::tests::every_rehearsal_is_taught_once_in_the_order_of_the_tabs`, `every_lesson_says_what_it_stands_for_what_to_expect_and_what_a_client_does`, `the_retries_are_taught_with_the_defaults_the_sdks_ship_with`, `the_timeouts_are_taught_with_slow_and_a_denial_is_taught_without_retries`, `the_words_never_say_asleep_or_promise_what_is_not_true`, `tests/lab_api_*::the_guide_has_the_intro_and_the_six_lessons_in_the_order_of_the_tabs`

### AWS-LAB-19. The API checks in words, makes, lists, switches, ends and forgets
`rules/check` says the sentence (`Throttle 1 in 4 SQS SendMessage calls to orders for 10 minutes`, made once from the rule) and every problem
by field; a Fail rule also says the code each service it names answers with. Create returns the rule or the check that says why not
(outcome `made` or `refused`, status 200). A service no doze-aws has, an operation none of the services named has, and a Lab that is full are
problems in words, not silent rules that never fire. Switch toggles (or sets `enabled`), remove ends, stop-all ends every rule and counts them,
history/clear forgets the ended ones. Every shape is a `ts-rs` model and a test checks each answer has the keys of its TypeScript type.
Test: `tests/lab_api_*::a_rule_is_checked_in_words_before_it_is_made`, `a_fail_rule_says_what_each_service_it_names_answers`, `stop_all_ends_every_rule_and_says_how_many`, `at_first_there_are_no_rules_and_the_answer_has_the_keys_of_its_type`, `lab::words::tests::a_rule_is_a_sentence_made_once`

### AWS-LAB-20. A retry is possible
The answer to a throttle, an unavailable or an internal error carries a code every SDK retries, and the rule can run out between the tries: a
client that retries as the SDKs do (three attempts) gets through when a rule that fires twice has fired twice. The browser test runs the real AWS
CLI at its default retries against a throttle rule and sees the three tagged tries on Live.
Test: `tests/lab_wire_*::a_client_that_retries_as_the_sdks_do_gets_through_when_the_rule_has_run_out`, `e2e/tests/live-lab.spec.ts`
