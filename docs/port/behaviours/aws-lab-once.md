# Behaviours: the AWS Lab's forced events

Rehearsals that are not rules: things production does on a timer, asked for now (`docs/port/aws/lab.md`, section 3). The mechanism is
`aws_wire::forced::Forced` (a service's own step, reached through `Service::forced()`), `doze_runtime::Rests` (resting a lazy service) and
`aws-console::lab_once` (forms, carrying out, history, the guide); the endpoints are `GET/POST /api/v1/lab/once...`. The Kafka Lab's LAB-15 to
LAB-18 are the model: the same function the timer runs, behind a right, refused in words when there is nothing to do.

### ONCE-1. A forced event runs the function the service's own timer or request path runs
Never a transition made for the Lab. The survey in `lab.md` names the function for each kind (`Lazy`'s idle close, Scheduler's `fire_now` and `attempt`,
`Queue::change_visibility`, DynamoDB's `housekeeping`, Lambda's `Runtime::retire`); what has no such function (Secrets rotation, a recovery window, a retention
sweep) is not built.
Test: `it_is_what_a_consumer_does_with_a_visibility_timeout_of_zero`, `it_is_what_the_timer_does`, `a_recurring_schedule_keeps_its_plan_and_its_timer_still_fires_it_when_due`, `a_service_told_to_rest_closes_as_it_does_when_idle_and_opens_again_on_the_next_call`

### ONCE-2. The index says which kinds this run can carry out, and why not
Five kinds in the order of the cards. A kind whose service is not served (`--services`), not on the bus, or without forced events, is `available: false` with the
reason in words, and a POST for it is refused in the same words without doing anything.
Test: `the_index_lists_the_five_kinds_and_says_why_one_cannot_be_carried_out`

### ONCE-3. A resting service is described as resting and is not called by a form
A form that would list a service's resources asks the register whether it runs in the region; if it rests, the answer is `resting: true`, no choices and a note, and no
call is made. `look=yes` reads it anyway. The rest form lists every served service as running or resting without calling any.
Test: `a_resting_service_is_described_as_resting_and_read_only_when_asked_to_be`, `the_rest_form_says_which_services_run_without_calling_any`

### ONCE-4. Resting closes the actor the way the idle timer does, per region
The actor stops taking requests, answers what is queued, closes the service and ends; the next call opens it again with its data. Resting what rests, or what was never
opened, is refused in words. One region's rest leaves the others running; an unknown region is refused.
Test: `a_service_told_to_rest_closes_as_it_does_when_idle_and_opens_again_on_the_next_call`, `what_is_queued_when_a_service_is_told_to_rest_is_still_answered`, `a_service_that_is_resting_or_was_never_made_is_said_so`, `a_region_is_rested_alone_and_the_others_keep_running`, `a_rested_service_answers_its_next_call_keeps_its_data_and_is_shown_as_resting`, `a_region_is_rested_alone`

### ONCE-5. The cost of the wake can be timed, and Home calls a rested service resting
With the option, the first read of the service after the rest and the same read again are timed and told, and both are on Live as the console's calls. Home and the
Resources index count a service as running only if a call reached it after the Lab rested it.
Test: `the_wake_is_timed_when_asked_and_the_calls_are_on_live_as_the_consoles`, `a_rested_service_answers_its_next_call_keeps_its_data_and_is_shown_as_resting`

### ONCE-6. A schedule fired now invokes its target once by the timer's path and keeps its plan
The invocation, the first attempt as the execution role, the retry policy and the dead-letter queue are the timer's. A recurring schedule's next fire is unchanged; a
one-time schedule's fire is its one fire (deleted afterwards when `ActionAfterCompletion` says so). A disabled schedule, and one with no fire left (a one-time schedule already
fired, an `EndDate` passed), are refused in words and nothing is invoked. A target that refuses is told with its code and what the retry policy does.
Test: `a_schedule_due_in_a_day_fires_now_and_its_target_gets_the_message_exactly_once`, `a_one_time_schedule_has_one_fire_and_it_is_the_forced_one`, `a_disabled_schedule_has_nothing_to_fire_and_an_ended_one_says_so`, `a_target_that_refuses_is_said_with_its_code_and_what_the_retry_policy_does`, `a_schedule_due_in_a_day_fires_now_its_target_gets_the_message_once_and_the_plan_stays`

### ONCE-7. An in-flight message comes back with the receive count it had
Every in-flight message of the queue is released by `ChangeMessageVisibility` with a timeout of 0 and its latest receipt handle: receivable at once, the count kept, older handles
still naming it, a long poll that waits woken, a message past `maxReceiveCount` moved on by the next receive. Delayed messages are not touched; a queue with none in flight is refused in
words with what it holds; a missing queue is the call's own refusal. Other queues keep their timeouts.
Test: `a_forced_timeout_brings_in_flight_messages_back_with_the_receive_count_they_had`, `with_nothing_in_flight_it_says_what_the_queue_holds_instead`, `only_the_queue_asked_for_comes_back_and_a_timeout_that_was_not_forced_still_holds`, `a_message_received_as_often_as_the_dead_letter_policy_allows_is_said_to_move_on_the_next_receive`, `a_long_poll_that_waits_is_woken_by_the_messages_that_come_back`, `an_in_flight_message_comes_back_with_its_receive_count_and_a_second_try_says_there_is_none`

### ONCE-8. The TTL sweep removes the expired items and not the live ones, by the timer's function
It counts what is expired in the table, then runs the region's `housekeeping` with the once-a-second limit lifted, as the timer's tick does; every table with time to live in the region is
swept, a table with a stream gets its REMOVE records, a batch is 500 and what is left is said. With nothing expired it is refused in words with when the next item expires, and what AWS
documents (typically two days). A table without time to live and a missing table are said.
Test: `the_sweep_removes_the_expired_items_and_not_the_live_ones`, `it_is_what_the_timer_does`, `with_nothing_expired_it_says_when_the_next_item_expires`, `a_sweep_removes_one_batch_and_says_how_many_are_still_expired`, `a_table_without_time_to_live_and_a_table_that_is_not_there_are_said`, `the_sweep_removes_what_has_expired_and_not_what_has_not`

### ONCE-9. A cold start ends the warm processes by the call that code or configuration changes make
`Runtime::retire`, after finding the function (a missing one is the call's refusal); an invocation in progress finishes; nothing about the function changes; the next invocation starts a new
process. With no process running it is refused in words (the next call is already cold).
Test: `a_forced_cold_start_ends_the_warm_process_and_the_next_invocation_starts_another`, `a_function_with_no_process_is_already_cold_and_a_missing_one_is_refused`, `a_function_with_no_process_is_already_cold_and_a_missing_one_is_said`

### ONCE-10. The service checks a right before each forced step
`sqs:ChangeMessageVisibility`, `scheduler:UpdateSchedule`, `dynamodb:DeleteItem`, `lambda:UpdateFunctionConfiguration`, each on the resource, with the resource's policy where it has one. The console is the
account's root; a caller without the right meets the call's own refusal and nothing changes. Resting asks no right of a service (it closes no data); the console's endpoints are behind the same-origin proof.
Test: `a_real_service_brings_a_message_back_and_a_caller_without_the_right_is_refused`, `a_caller_without_the_right_is_refused_and_nothing_is_fired`, `a_real_service_sweeps_and_a_caller_without_the_right_is_refused`, `a_caller_without_the_right_is_refused_and_the_process_stays`, `under_enforce_the_console_is_the_accounts_root_and_may_force_what_the_call_would_allow`, `a_write_without_the_same_origin_proof_is_refused`

### ONCE-11. Every attempt is kept, the last 200, newest first
A refusal in words is an attempt and is kept with `ok: false`; a request that is not one (an unknown kind, no choice, a region AWS does not have) is not. The history clears on request and a restart
clears it.
Test: `the_history_keeps_two_hundred_and_is_cleared_on_request`, `a_rested_service_answers_its_next_call_keeps_its_data_and_is_shown_as_resting`, `an_unknown_kind_a_missing_choice_and_a_region_that_is_not_aws_s_are_refused`

### ONCE-12. Every region the console can select works
The form takes `?region=`, a POST takes `region` in its body, and both fall back to `X-Console-Region`, then `--region`; a service's actor of that region is the one asked.
Test: `a_region_is_rested_alone`, `a_resting_service_is_described_as_resting_and_read_only_when_asked_to_be`, `the_rest_form_says_which_services_run_without_calling_any`

### ONCE-13. A rehearsal changes nothing about what the same call does when called normally
A service without forced events has `Service::forced()` of `None`; the services' own suites (SQS 49, Scheduler 37, DynamoDB 111, Lambda 79, doze-runtime 22) pass unchanged beside the new ones. A queue that
was not named keeps its visibility timeout; a recurring schedule's timer still fires it when due; the sweep leaves the live items.
Test: `only_the_queue_asked_for_comes_back_and_a_timeout_that_was_not_forced_still_holds`, `a_recurring_schedule_keeps_its_plan_and_its_timer_still_fires_it_when_due`, `the_sweep_removes_the_expired_items_and_not_the_live_ones`

### ONCE-14. The Lab teaches what it rehearses
One lesson per kind, in the order of the cards: what it stands for in production, what to expect here, what a well-behaved client does, the settings with the defaults AWS documents (read from the vendored
models), where to see it. No invented number. Words are one module (`crate::lab_once::teach`), served by `GET /api/v1/lab/once/guide`.
Test: `every_kind_is_taught_once_in_the_order_of_the_cards`, `the_words_say_never_asleep_and_use_no_marketing`, `the_guide_teaches_each_kind_once_in_the_order_of_the_cards`
