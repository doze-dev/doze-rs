# The console (Phase 9)

What the web console (`crates/kafka-console`) does, rule by rule, and the test that holds each. Slice 9a is built (the crate, the shell, Home and Connect, the advertised-listener check, the stream skeleton, the feature in `doze-kafka`); later slices add their rules here. The security rules of the console (its reserved principal, the guard, the same-origin proof, the headers, escaping, the address) are SEC-240 to SEC-249 in `security.md` and are not repeated; their tests are named there.

The design is the canvas "doze-kafka console direction" (`docs/port/console/`), the plan `docs/port/phase9.md`. Where a board shows something the broker cannot yet say, the page says what it can and the place in the code is tagged with the REALITY or GROUNDING reference; the list is in "What the boards show that the broker cannot supply yet" below.

The numbers are checked by `tools/check-console-catalogue.ts` (`bun tools/check-console-catalogue.ts`; add `--lean-tree` to also check that the lean build has no HTTP in its tree): no CON-n is defined twice, none is referenced without a definition, and every test name quoted in this file is a `fn` in the Rust sources.

## The routes and the shell

## CON-1. The route table is the router
`routes.rs` is the one list (path, method, mutates, the Kafka API behind it, the screens it serves); the router is built from it and from nothing else; `Handler` is matched without a wildcard, so a row without a handler does not compile. Every row answers; a path that is no row is a 404. Home's three boards (`home`, `home-quiet`, `home-attention`) are states of the one route `/`, chosen by data. Test: `tests/routes::every_row_of_the_table_has_a_handler_and_every_handler_a_row`, `routes::tests::no_screen_is_served_by_two_routes_and_no_route_is_listed_twice`, `routes::tests::the_five_boards_of_slice_9a_are_served_by_two_routes_and_the_advertised_page`.

## CON-2. An unknown path is a 404 page in the shell, a wrong method a 405
Both are drawn in the shell, the path is shown escaped. Screens a later slice builds (`/live`, `/topics`, `/access`, `/settings`, `/lab`, `/topics/new`, `/topics/{topic}/producers`) are rows of the table that answer a page saying they are not built yet. Test: `tests/routes::an_unknown_route_is_a_404_page_in_the_shell_and_a_wrong_method_a_405`.

## CON-3. Every link is built from a base (`--console-prefix`)
`Base` turns a path into a URL under the prefix (`/` is the root; `kafka`, `/kafka/` and `/kafka` are one prefix; a prefix is always a path, never `//host`); the root is registered as both `/k` and `/k/`. Test: `base::tests::a_prefix_is_one_path_whatever_slashes_it_was_written_with`, `base::tests::a_url_is_the_prefix_then_the_path`, `base::tests::a_request_path_is_stripped_of_the_prefix_and_only_of_whole_segments`, `routes::tests::a_prefix_registers_the_root_both_ways_and_prefixes_the_rest`, `ui::shell::tests::the_rail_marks_the_active_space_and_every_link_is_under_the_base`, `tests/routes::every_href_and_src_of_every_page_resolves_under_the_base` (run with no prefix and with `/kafka`), `tests/process console::a_prefix_moves_every_page_under_it`, `cli::tests::console::the_console_is_on_by_default_and_off_only_for_false_and_the_prefix_is_a_path`.

## CON-4. Every link and asset of every page resolves
The crawl starts at every page, follows every `href` and `src` that starts with a slash and fails on a 404 or a link outside the base: no dangling link, no missing asset. Test: `tests/routes::every_href_and_src_of_every_page_resolves_under_the_base`.

## CON-5. The shell
A `<!DOCTYPE html>` document with `lang`, one `<main>`, a skip link, the top bar (the address with a copy chip, the version profile, the security chip, which says what the broker has on and links to Access), the rail of the six spaces with the active one marked, the breadcrumb, a banner hidden until the stream is lost, and a banner across the top when the console is served beyond loopback. Left out and tagged: the pinned items and the key hints of the rail (no store, no keyboard layer yet), the search (drawn, disabled), the "Reached from" line (canvas scaffolding). Test: `ui::shell::tests::the_document_is_html_with_a_language_a_title_and_one_main`, `ui::shell::tests::the_top_bar_says_the_address_the_profile_and_what_is_on`, `ui::shell::tests::a_page_that_watches_opens_a_stream_and_one_that_does_not_has_none`, `ui::shell::tests::the_stream_banner_is_hidden_until_the_script_shows_it`, `ui::shell::tests::the_search_is_drawn_as_the_board_draws_it_and_not_offered`, `facts::tests::the_chip_says_what_is_on_in_the_boards_vocabulary`, `facts::tests::the_chrome_locks_the_chip_when_anything_protects_the_broker`.

## Home

## CON-6. Home is in the state its data calls for
Attention when a transaction is close to its timeout, quiet when the broker holds no user topic, steady otherwise; attention wins. The state is chosen from Metadata, ListTransactions and DescribeTransactions through `Broker::handle` as `User:__console`. Test: `gather::home::tests::the_state_follows_the_data_and_attention_wins`, `tests/live::a_broker_with_no_topics_is_the_quiet_home_and_with_one_it_is_the_steady_one`, `tests/live::the_console_reads_a_broker_that_refuses_everyone_else_because_it_is_a_super_user`.

## CON-7. "Close to its timeout" is two thirds of it
The one condition Home draws as needing someone: an open transaction whose elapsed time is two thirds of its timeout or more (the board's own example, 41 s of 60 s, meets it). The page names the transaction, producer and epoch, the first partition, when it opened, the bar, and what a `read_committed` consumer is held back by (the last stable offset against the high watermark, from ListOffsets). Test: `gather::attention::tests::two_thirds_of_the_timeout_is_close`, `gather::attention::tests::a_timeout_of_nothing_is_never_close_and_nothing_overflows`, `tests/live::an_open_transaction_close_to_its_timeout_is_what_needs_someone`, `tests/boards::the_attention_home_carries_the_boards_transaction`.

## CON-8. Home says what the broker holds, never what it cannot know
The sentence under the title is the running time, the last stop and the counts of topics and partitions; the quiet state claims no client; the all-clear says only that no transaction is near its timeout. Test: `words::tests::the_steady_sentence_says_how_long_and_how_it_stopped_and_what_it_holds`, `words::tests::the_steady_sentence_has_a_form_for_each_last_stop`, `words::tests::the_quiet_sentence_names_the_listener_and_claims_no_client`, `words::tests::the_attention_headline_counts`.

## CON-9. The instance card says what it measured and says when it did not
Started (UTC, said), up for, last stop (clean, not clean, or none yet on a new data directory), open partitions of the bound, open files, memory and threads where the platform lets a safe process read them (Linux `/proc`; elsewhere "Not measured" and "Measured on Linux only"), the version profile, access in words. The facts that the protocol hides come from `ConsoleView::instance()` (feature `inspect`), which checks DESCRIBE. Test: `words::tests::the_instance_lede_follows_the_last_stop`, `words::tests::open_partitions_are_of_the_bound_when_there_is_one`, `gather::home::tests::an_instance_card_counts_the_run_from_its_start`, `facts::tests::the_access_row_of_home_reads_as_the_board_does_when_open`, `kafka-broker view::tests::a_new_data_directory_has_no_last_stop_and_a_clean_stop_is_remembered`, `kafka-broker view::tests::a_run_after_a_stop_that_vouched_for_nothing_says_unclean`, `kafka-broker view::process::tests::the_resident_set_is_the_second_number_in_pages`.

## The advertised-listener check

## CON-10. Three rules, from real dials
The check does what a client does: (1) dial the listener and ask ApiVersions v0; (2) ask Metadata, in process, what the broker says clients should reconnect to; (3) dial that address back and ask ApiVersions. A TLS listener is only dialled. Each rule is ok or bad with the board's words, and the failure says why in a few words (`connection refused`, `no such host`, `timed out after 700 ms`, `connected, but it did not answer ApiVersions`), never the system's own text. Test: `probe::judge::tests::an_address_that_answers_is_reachable_in_the_boards_words`, `probe::judge::tests::a_wrong_advertised_address_is_the_boards_bad_case`, `probe::judge::tests::a_listener_that_does_not_answer_fails_the_first_rule`, `probe::judge::tests::metadata_that_names_something_else_or_nothing_fails_the_second`, `probe::judge::tests::tls_is_only_dialled`, `probe::tests::a_failed_dial_says_why_in_a_few_words`, `probe::tests::something_that_is_not_kafka_is_said_not_to_be`, `probe::tests::the_request_is_a_framed_api_versions_v0_with_the_client_id`, `probe::tests::an_answer_must_echo_the_correlation_id_and_carry_no_error`, `tests/live::the_check_passes_against_a_real_listener_and_the_page_says_so`, `tests/live::a_deliberately_wrong_advertised_address_fails_the_third_rule_in_words`.

## CON-11. The check never holds a page for long
Every dial has its own deadline (700 ms, 1 s), the whole check 1.5 s; one that does not finish claims nothing ("Not checked"). It runs on demand and once at page load (a result under 30 s old is shown without dialling), never on a timer. Test: `probe::tests::a_closed_port_is_refused_and_a_silent_one_times_out_and_neither_waits_longer_than_its_deadline`, `probe::judge::tests::a_dial_that_did_not_finish_claims_nothing`, `probe::judge::tests::a_rule_is_ok_exactly_when_the_verdict_can_be_reachable`, `tests/live::a_name_that_cannot_be_resolved_is_not_reachable_and_the_check_stays_inside_its_budget`, `tests/live::a_listener_that_is_gone_fails_the_first_rule_and_a_result_is_reused_for_thirty_seconds`.

## CON-12. "Check again" is a plain form
A POST to `/connect/advertised/check` with `back=home` or `back=advertised`: without a script it answers 303 back to the page, with the script it answers the fragment to swap in; anything else is 400. It needs the same-origin proof (SEC-244). Test: `tests/routes::the_check_form_comes_back_to_its_page_without_a_script_and_swaps_a_fragment_with_one`.

## Connect

## CON-13. The snippets are the board's, with the real address
Five clients (franz-go, sarama, librdkafka and kcat, kafkajs, the Java client) with the bootstrap address substituted; the address is the listener's, or for a wildcard bind the advertised name. Test: `snippets::tests::the_address_is_in_every_snippet_and_the_clients_are_the_boards`, `snippets::tests::the_code_is_the_boards_to_the_character`, `snippets::tests::another_address_is_substituted_everywhere`, `facts::tests::a_specific_bind_is_the_bootstrap_and_a_wildcard_is_the_advertised_name`, `facts::tests::an_advertised_ipv6_literal_is_bracketed_once`, `tests/boards::connect_has_the_boards_cards_and_the_five_clients`.

## CON-14. The boards, rendered from the larder fixtures
Each of the five boards is rendered with the fixtures of `kit/world.md` (the "larder") to `target/board-diff/<id>.html` for `tools/board-diff.ts`, and checked against what the board says in order: headings, labels, buttons, numbers. `design/routes.json` is generated from the route table by a test. Test: `tests/boards::every_board_of_the_slice_renders_and_is_written_for_the_tool`, `tests/boards::home_has_the_parts_of_its_board_in_its_order`, `tests/boards::the_quiet_home_is_the_boards_first_run`, `tests/boards::the_advertised_page_has_the_check_the_bad_case_and_what_to_set`, `tests/boards::the_fixtures_carry_the_worlds_headline_numbers`, `routes::tests::routes_json_is_what_the_table_says`.

## The client layer

## CON-15. Every page is complete with JavaScript off
Fragments are plain GETs (`/fragments/{id}`), forms are plain forms, the copy buttons and the "Copy as" control are hidden until a script reveals them, and the address is always text that can be selected. Test: `ui::client::tests::nothing_the_script_needs_is_shown_without_it`, `ui::client::tests::a_form_posts_with_no_script_and_swaps_its_target_with_one`, `tests/routes::the_check_form_comes_back_to_its_page_without_a_script_and_swaps_a_fragment_with_one`.

## CON-16. One file holds every name the script reads (decision D4a)
The client layer is `assets/console.js`, our own, no library (the spike, `phase9/client-spike.md`). Every attribute, id, header and event name it reads is a constant of `ui/client.rs` and a test checks the script uses each. The rules the spike found are each a line of the script and a test that the line is there: the script sends `If-None-Match` itself and skips the swap on a 304, the newest request wins per fragment, the stream is closed on `pagehide` and in a hidden tab and reopened on `pageshow` and when visible, the reconnect waits longer and never gives up, a focused input keeps its value. The script uses no eval and no inline script (the CSP, SEC-245). Test: `ui::client::tests::the_script_uses_every_name_this_file_defines`, `ui::client::tests::the_markup_uses_the_attribute_names_the_constants_say`, `ui::client::tests::the_script_follows_the_rules_the_spike_found_and_runs_under_the_strictest_policy`. The script's behaviour in a browser is covered by the manual Chrome pass of the slice (it cannot be unit-tested in Rust).

## CON-17. Fragments revalidate
A fragment answers `Cache-Control: no-cache` and an ETag over its text; a request with the same ETag is a 304 with no body. Pages are `no-store`; files are `public, max-age=31536000, immutable` under a content-hashed name. Test: `tests/routes::pages_are_not_stored_fragments_revalidate_and_files_are_kept_for_a_year`.

## The stream

## CON-18. `GET /events?watch=a,b`
A closed vocabulary of fragment ids (`home-sentence`, `home-callout`, `home-instance`); anything else is a 400 before a stream opens. The first event, `tick`, arrives at once and names every watched fragment, so a page that has just loaded or reconnected reconciles itself; each later `tick` names them again; a comment keeps the connection alive every 15 s. Test: `stream::tests::only_the_closed_vocabulary_may_be_watched`, `tests/stream::the_first_event_arrives_at_once_and_names_what_is_watched`, `tests/stream::a_tick_arrives_when_the_signal_is_bumped_and_by_itself_on_the_floor_timer`, `tests/stream::an_unknown_fragment_is_refused_before_a_stream_is_opened`, `tests/routes::the_stream_takes_only_the_closed_vocabulary_and_an_unknown_fragment_is_a_404`.

## CON-19. The signal's task exists only while a stream is connected
One `watch` channel fed by one task (a 2 s floor timer until the recorder of slice 9e bumps it); the first stream starts the task and the last to go stops it, so an idle console costs nothing. Test: `tick::tests::nobody_listening_means_no_task`, `tick::tests::the_first_viewer_starts_the_task_and_the_last_to_leave_stops_it`, `tick::tests::a_viewer_hears_a_tick_every_period`, `tick::tests::a_bump_reaches_every_viewer_at_once`, `tests/stream::the_task_exists_only_while_a_stream_is_connected`.

## CON-20. Shutdown ends every stream
Every stream selects on the server's cancellation token, so a graceful shutdown never waits on one. Test: `tests/stream::shutdown_while_ten_streams_are_open_completes_in_under_a_second` (with 10 open streams, under a second), `tests/process console::a_stopping_broker_does_not_wait_for_an_open_console_stream`.

## Assets and components

## CON-21. The assets are a manifest, under hashed names
`assets.rs` lists every file of `assets/` with `include_bytes!`; a test fails on a file that is not listed and on an entry with no file. Served names carry a content hash; the stylesheet names the fonts' hashed names. `console.css` is the kit's `paper.css` and `kafka.css` unchanged plus `design/console.css`, joined; a test says so. Inter and JetBrains Mono (Latin subset, OFL) are embedded with their licences kept beside them; no third-party script ships. The total is about 140 KB. Test: `assets::tests::every_file_of_assets_is_in_the_manifest_and_every_entry_has_its_file`, `assets::tests::a_licence_is_kept_for_every_third_party_file`, `assets::tests::the_stylesheet_is_the_kit_and_the_consoles_rules_joined`, `assets::tests::a_name_changes_with_its_content_and_only_then`, `assets::tests::every_served_file_is_found_by_its_hashed_name_and_the_stylesheet_names_its_fonts`, `assets::tests::the_shipped_files_weigh_what_the_plan_says`.

## CON-22. The components write what their golden files say
The shell, the buttons and states, the notices, the layout, and the table, form, filter and code components each have a golden file under `tests/golden/`; a change to the HTML is a diff in review. Variants are enums, so a misspelt class does not compile. Test: `tests/components::buttons_states_and_pills`, `tests/components::notices`, `tests/components::layout`, `tests/components::table_form_filter_and_code`, `ui::button::tests::a_class_is_built_from_its_variant_and_size`.

## CON-23. Numbers, sizes and times are written by pure functions
`format.rs`: spans of time in two units, numbers with separators, binary sizes with one decimal, milliseconds, civil UTC dates (checked over two hundred years of days). Test: `format::tests::a_span_is_its_two_largest_units`, `format::tests::a_longer_wait_never_reads_as_a_shorter_one`, `format::tests::a_number_has_thousands_separators_and_a_sign`, `format::tests::a_number_without_its_commas_is_the_number`, `format::tests::a_size_is_binary_with_one_decimal`, `format::tests::civil_time_matches_known_instants`, `format::tests::every_day_for_two_hundred_years_has_a_valid_date_and_follows_the_one_before`, `form::tests::a_component_is_decoded_as_a_browser_wrote_it`, `form::tests::decoding_never_panics_on_any_byte_string`.

## The binary

## CON-24. The console is in the default build and the lean build has no HTTP
`doze-kafka` has the cargo feature `console`, on by default (decision D1); `--no-default-features` is the broker with no HTTP, no axum and no maud in its tree (`bun tools/check-console-catalogue.ts --lean-tree`). `--console=false` leaves the broker as it was before there was a console. Test: `tests/process console::console_false_leaves_the_broker_as_it_was`, `tests/process console::the_console_serves_its_pages_beside_the_broker_and_refuses_what_it_should`, `cli::tests::the_flags_the_conformance_harness_passes_are_all_accepted`.

## What the boards show that the broker cannot supply yet

Each is tagged in the code with its reference and is a debt row of section J in `debt.md`.
- Home, "N apps appear on the wire, which reaches back to HH:MM, and the last request landed N s ago": the recorder (REALITY 1, slice 9e). The page says what the broker holds.
- Home, "no partition is offline and the data directory is writable": `partition_state()` (slice 9b) and no writability check. The all-clear says only that no transaction is near its timeout.
- Home, the link from the all-clear to the attention state: the state is chosen by data, so the example is plain text.
- Home, "Waiting for a client id this broker has not seen" and the link to `connect-first-request`: the first-request detector (slice 9e).
- Home quiet, "no clients": there is no connection table yet (slice 9e).
- The instance card, "Segments and indexes" for descriptors: the count is of all open files and sockets, and says so. "Idle memory" and "Threads" are measured on Linux only.
- Times are UTC and say so (no safe way to read the local zone).
- The rail's pinned items and key hints, the search box, and `connect-first-request`.


## The topic writes (slice 9b, p9b-write)

## CON-150. The writes are one list
`actions.rs` lists the Kafka APIs the console writes with (CreateTopics, Produce, CreatePartitions, DeleteRecords, DeleteTopics) and a test compares it with the `data-op` names the committed boards draw, both ways. Test: `actions::tests::actions_match_boards`.

## CON-151. A write is a POST with a same-origin proof, and a GET never writes
Every write is a plain `<form method=post>`; without the proof every POST row is 403 and changes nothing; every GET of every route calls only reads (a log of the APIs called, and the broker's state before and after). Test: `tests/guard::every_post_row_needs_the_same_origin_proof_and_changes_nothing_without_it`, `tests/live_write_admin::a_get_never_asks_the_broker_to_change_anything`.

## CON-152. Create topic is three steps of one route
`/topics/new?step=name|config|review`; each step is a GET form carrying every value typed, so Back and a reload keep them; moving on checks the steps before it and lands on the first with something to fix; the POST makes the topic (or, "Check only", runs the checks with `validate_only`). Test: `gather::create::tests::moving_on_checks_the_steps_before_the_one_asked_for`, `tests/live_write::a_topic_made_through_the_pages_exists_for_the_protocol_with_its_partitions_and_configs`.

## CON-153. The topic-name rule is the broker's own
The form judges a name with `TopicName::parse` and says why in a sentence (a character by name or code point, 249, `.` and `..`); the collision with `.` against `_` and with letter case is advised and decided by the broker. Test: `validate::tests::every_name_the_broker_would_parse_passes_and_no_other_does`, `validate::tests::a_topic_name_is_judged_by_the_brokers_own_rule_and_told_in_a_sentence`.

## CON-154. A refusal is Kafka's code and the broker's own sentence
The page shows the code, its name and the handler's `error_message` (Kafka's description where the answer has none, as DeleteRecords), then the console's rule and what would pass where the table has one. Test: `tests/live_write::every_refusal_of_a_create_is_kafkas_code_and_the_brokers_own_sentence`, `refusal::tests::kafkas_names_are_made_from_the_variants`.

## CON-155. The teaching under a field comes from the broker's table
Kafka's documentation string and default for `cleanup.policy` and `retention.ms` are read through `ConsoleView::topic_config_defaults` (checked against CREATE on the cluster), with tags removed; the limits are `ConsoleView::limits`. Test: `view::tests::the_create_form_is_taught_from_the_brokers_own_table_and_only_to_who_may_create`, `teach::tests::kafkas_documentation_becomes_plain_text_of_a_few_sentences`.

## CON-156. A record is framed as a client frames it
One v2 batch, CRC and all, written by `kafka_wire::batch::single_record` (checked byte for byte against the test builder), sent through Produce at the topic's own limits; a key left to the key goes where Kafka's partitioner puts it. Test: `batch::compose::tests::a_batch_is_what_the_format_says_byte_for_byte`, `partitioner::tests::the_hash_is_kafkas_on_its_own_test_vectors`, `tests/live_write::a_keyed_and_a_keyless_record_are_written_and_fetched_back_through_the_protocol`.

## CON-157. A produce that Kafka refuses shows Kafka's words
A keyless record on a compacted topic is INVALID_RECORD with the broker's "Compacted topic cannot accept message without key in topic partition ..." and uses no offset; a record over the topic's `max.message.bytes` is MESSAGE_TOO_LARGE with the limit read from the topic. Test: `tests/live_write::kafkas_own_words_for_a_keyless_record_on_a_compacted_topic_and_for_a_record_that_is_too_large`.

## CON-158. Partitions go up, never down
The Add-partitions dialog takes the new total; a count not above the present one is refused INVALID_PARTITIONS in the broker's words, nothing changes. Test: `tests/live_write_admin::partitions_can_be_added_and_never_taken_away`.

## CON-159. Delete records moves the log start
The earliest offset after the call is the one asked (-1 is the high watermark); below the start changes nothing; past the end is OFFSET_OUT_OF_RANGE; a compact-only topic is POLICY_VIOLATION. Test: `tests/live_write_admin::records_are_deleted_up_to_an_offset_and_the_log_start_is_that_offset`.

## CON-160. A topic is deleted when its name is typed
Wrong confirmation is 422 and sends nothing; the delete shows what uses the topic (groups with offsets on it, open transactions) as read; afterwards the topic is gone from Metadata. Test: `tests/live_write_admin::a_topic_is_deleted_only_when_its_name_is_typed_and_is_then_gone`.

## CON-161. A dialog is a route
With a script a link marked `data-dialog` opens the body (answered alone to `X-Fragment`) in a `<dialog>`; without one the same address is a full page with the dialog in it; either way it is one POST form. Test: `tests/routes_write::a_dialog_is_a_full_page_to_a_browser_and_its_body_alone_to_a_script`, `ui::client::tests::the_script_opens_dialogs_by_the_names_this_file_defines`.

## CON-162. Without super rights each write is refused as a client would be
A caller that may only DESCRIBE gets TOPIC_AUTHORIZATION_FAILED on every write, with the console's reading, and a topic it may not describe is "not there". Test: `tests/live_write_admin::on_a_broker_with_acls_a_caller_that_may_only_describe_is_refused_as_a_client_would_be`.

## CON-163. Hostile input is text
Names with markup, NUL and newlines, 249 and 250 bytes, invalid UTF-8 escapes, duplicate fields and bodies at the limit, through every form and every query: no panic, no 5xx, inert echo, no header injection. Test: `tests/security::hostile_input_in_every_topic_form_is_answered_and_echoed_as_text`, `form::body_tests::fields_past_the_limit_are_dropped_not_errors`, `form::body_tests::a_field_as_large_as_the_body_limit_is_read_whole`.

## CON-164. The fourteen boards, fixtures and a real broker
Each write board is rendered from the larder and from a real broker and compared by `tools/board-diff.ts`. Test: `tests/boards_write::every_board_of_the_slice_renders_and_is_written_for_the_tool`, `tests/live_write_boards::the_fourteen_boards_are_rendered_from_a_real_broker`.

## CON-165. Every link of the write pages resolves
Under any prefix, except links to the read builder's pages until the merge. Test: `tests/routes_write::every_link_of_the_write_pages_resolves_or_is_the_read_builders`.
