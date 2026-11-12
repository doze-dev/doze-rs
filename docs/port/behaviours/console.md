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
Attention when a transaction is close to its timeout, quiet when the broker holds no user topic, steady otherwise; attention wins. The state is chosen from Metadata, ListTransactions and DescribeTransactions through `Broker::handle` as `User:__console`. Test: `gather::home::tests::the_state_follows_the_data_and_attention_wins`, `tests/live::a_broker_with_no_topics_is_the_quiet_home_and_with_one_it_is_the_steady_one`, `tests/live::the_console_reads_a_broker_that_refuses_everyone_else_because_it_is_a_super_user`. The state is judged on every open transaction, CON-28.

## CON-7. "Close to its timeout" is two thirds of it
The one condition Home draws as needing someone: an open transaction whose elapsed time is two thirds of its timeout or more (the board's own example, 41 s of 60 s, meets it). The page names the transaction, producer and epoch, the first partition, when it opened, the bar, and what a `read_committed` consumer is held back by (the last stable offset against the high watermark, from ListOffsets). Test: `gather::attention::tests::two_thirds_of_the_timeout_is_close`, `gather::attention::tests::a_timeout_of_nothing_is_never_close_and_nothing_overflows`, `tests/live::an_open_transaction_close_to_its_timeout_is_what_needs_someone`, `tests/boards::the_attention_home_carries_the_boards_transaction`.

## CON-8. Home says what the broker holds, never what it cannot know
The sentence under the title is the running time, the last stop and the counts of topics and partitions; the quiet state claims no client; the all-clear says only that no transaction is near its timeout. Test: `words::tests::the_steady_sentence_says_how_long_and_how_it_stopped_and_what_it_holds`, `words::tests::the_steady_sentence_has_a_form_for_each_last_stop`, `words::tests::the_quiet_sentence_names_the_listener_and_claims_no_client`, `words::tests::the_attention_headline_counts`.

## CON-9. The instance card says what it measured and says when it did not
Started (UTC, said), up for, last stop (clean, not clean, or none yet on a new data directory), open partitions of the bound, open files, memory and threads where the platform lets a safe process read them (Linux `/proc`; elsewhere "Not measured" and "Measured on Linux only"). The memory row keeps the board's label "Idle memory" and says under the figure that it is what is resident now, read from `VmRSS` (kB, whatever the page size), not a measured idle (J72); the descriptors are the open files without the listing's own handle, the version profile, access in words. The facts that the protocol hides come from `ConsoleView::instance()` (feature `inspect`), which checks DESCRIBE. Test: `words::tests::the_instance_lede_follows_the_last_stop`, `words::tests::open_partitions_are_of_the_bound_when_there_is_one`, `gather::home::tests::an_instance_card_counts_the_run_from_its_start`, `facts::tests::the_access_row_of_home_reads_as_the_board_does_when_open`, `kafka-broker view::tests::a_new_data_directory_has_no_last_stop_and_a_clean_stop_is_remembered`, `kafka-broker view::tests::a_run_after_a_stop_that_vouched_for_nothing_says_unclean`, `kafka-broker view::process::tests::the_resident_set_is_the_vmrss_line_in_kb_whatever_the_page_size`, `kafka-broker view::process::tests::the_listing_of_the_open_files_does_not_count_its_own_handle`, `tests/home_live::the_memory_figure_says_it_is_what_is_resident_now_under_the_boards_label`.

## The advertised-listener check

## CON-10. Three rules, from real dials
The check does what a client does: (1) dial the listener and ask ApiVersions v0; (2) ask Metadata, in process, what the broker says clients should reconnect to; (3) dial that address back and ask ApiVersions. A TLS listener is only dialled. Each rule is ok or bad with the board's words, and the failure says why in a few words (`connection refused`, `no such host`, `timed out after 700 ms`, `connected, but it did not answer ApiVersions`), never the system's own text. The sentences around the rules say only what the check did: no "can always reach it again", and no "a client that reached" an address whose rule failed (`words::tests::the_instance_lede_claims_only_what_the_check_found`). Test: `probe::judge::tests::an_address_that_answers_is_reachable_in_the_boards_words`, `probe::judge::tests::a_wrong_advertised_address_is_the_boards_bad_case`, `probe::judge::tests::a_listener_that_does_not_answer_fails_the_first_rule`, `probe::judge::tests::metadata_that_names_something_else_or_nothing_fails_the_second`, `probe::judge::tests::tls_is_only_dialled`, `probe::tests::a_failed_dial_says_why_in_a_few_words`, `probe::tests::something_that_is_not_kafka_is_said_not_to_be`, `probe::tests::the_request_is_a_framed_api_versions_v0_with_the_client_id`, `probe::tests::an_answer_must_echo_the_correlation_id_and_carry_no_error`, `tests/live::the_check_passes_against_a_real_listener_and_the_page_says_so`, `tests/live::a_deliberately_wrong_advertised_address_fails_the_third_rule_in_words`.

## CON-11. The check never holds a page for long
Every dial has its own deadline (700 ms, 1 s), the whole check 1.5 s; one that does not finish claims nothing ("Not checked"), and a result that claims nothing is not reused. It runs on demand ("Check again") and at a page load (a result under 30 s old that says something is shown without dialling), never on a timer and never for a live fragment: the fragments the stream refetches show the last result (`Prober::peek`) and dial nothing, so a page left open dials once, not every 30 s. Callers that arrive while a check is running share its result (single-flight), so many at once are one set of dials. A dead advertised address therefore cannot slow a fragment. (The first text of this rule said "once at page load, never on a timer" and the code dialled from every fragment fetch: audit A2.) Test: `probe::tests::a_closed_port_is_refused_and_a_silent_one_times_out_and_neither_waits_longer_than_its_deadline`, `probe::judge::tests::a_dial_that_did_not_finish_claims_nothing`, `probe::judge::tests::a_rule_is_ok_exactly_when_the_verdict_can_be_reachable`, `tests/live::a_name_that_cannot_be_resolved_is_not_reachable_and_the_check_stays_inside_its_budget`, `tests/live::a_listener_that_is_gone_fails_the_first_rule_and_a_result_is_reused_for_thirty_seconds`, `tests/home_live::the_live_fragments_never_dial_and_a_page_load_dials_once_in_thirty_seconds`, `tests/home_live::a_dead_advertised_address_does_not_slow_a_fragment`, `tests/home_live::a_result_is_reused_until_it_is_old_and_callers_at_once_share_one_set_of_dials`, `tests/home_live::a_probe_against_a_black_hole_finishes_in_its_deadline_and_a_result_that_claims_nothing_is_not_reused` (the dial deadlines against a listener that accepts and never answers; the whole-check deadline is an outer bound that no single dial can reach, so that test pins the documented numbers).

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
The client layer is `assets/console.js`, our own, no library (the spike, `phase9/client-spike.md`). Every attribute, id, header and event name it reads is a constant of `ui/client.rs` and a test checks the script uses each. The rules the spike found are each a line of the script and a test that the line is there: the script sends `If-None-Match` itself and skips the swap on a 304, the newest request wins per fragment, the stream is closed on `pagehide` and in a hidden tab and reopened on `pageshow` and when visible, the reconnect waits longer and never gives up, a focused input keeps its value. The script uses no eval and no inline script (the CSP, SEC-245). Test: `ui::client::tests::the_script_uses_every_name_this_file_defines`, `ui::client::tests::the_markup_uses_the_attribute_names_the_constants_say`, `ui::client::tests::the_script_follows_the_rules_the_spike_found_and_runs_under_the_strictest_policy`. The script's behaviour is tested by running the shipped file in a DOM (`tools/dom`, happy-dom with a scripted fetch and EventSource; `cd tools/dom && bun install && bun test`, in CI): the ETag sent and kept per fragment and a 304 skipping the swap, an AbortController per fragment and a stale answer never overwriting a newer one, the stream closed on `pagehide` and in a hidden tab and reopened, a reconnect that never gives up, the banner shown by a failed fetch and hidden by the next answer, the focused input kept, "Copy as" applying to the client snippets only (the "When SASL is on" card copies its own lines), and a copy that works without `navigator.clipboard` (plain http on a LAN address). The Rust string tests above are no longer the guard. A mutation run over the script (15 mutants) is caught; the cases are in `tools/dom/console.test.ts`.

## CON-17. Fragments revalidate
A fragment answers `Cache-Control: no-cache` and an ETag over its text; a request with the same ETag is a 304 with no body. Pages are `no-store`; files are `public, max-age=31536000, immutable` under a content-hashed name. Test: `tests/routes::pages_are_not_stored_fragments_revalidate_and_files_are_kept_for_a_year`.

## The stream

## CON-18. `GET /events?watch=a,b`
A closed vocabulary of fragment ids (`home-live`); anything else is a 400 before a stream opens. The first event, `tick`, arrives at once and names every watched fragment, so a page that has just loaded or reconnected reconciles itself; each later `tick` names them again; a comment keeps the connection alive every 15 s. Test: `stream::tests::only_the_closed_vocabulary_may_be_watched`, `tests/stream::the_first_event_arrives_at_once_and_names_what_is_watched`, `tests/stream::a_tick_arrives_when_the_signal_is_bumped_and_by_itself_on_the_floor_timer`, `tests/stream::an_unknown_fragment_is_refused_before_a_stream_is_opened`, `tests/routes::the_stream_takes_only_the_closed_vocabulary_and_an_unknown_fragment_is_a_404`.

## CON-19. The signal's task exists only while a stream is connected
One `watch` channel fed by one task (a 2 s floor timer until the recorder of slice 9e bumps it); the first stream starts the task and the last to go stops it, so an idle console costs nothing. Test: `tick::tests::nobody_listening_means_no_task`, `tick::tests::the_first_viewer_starts_the_task_and_the_last_to_leave_stops_it`, `tick::tests::a_viewer_hears_a_tick_every_period`, `tick::tests::a_bump_reaches_every_viewer_at_once`, `tick::tests::the_task_really_stops_when_the_last_viewer_goes_and_does_not_tick_on_its_own_after`, `tick::tests::a_task_that_has_finished_is_not_running`, `tests/stream::the_task_exists_only_while_a_stream_is_connected`.

## CON-20. Shutdown ends every stream
Every stream selects on the server's cancellation token, so a graceful shutdown never waits on one. Test: `tests/stream::shutdown_while_ten_streams_are_open_completes_in_under_a_second` (with 10 open streams, under a second), `tests/process console::a_stopping_broker_does_not_wait_for_an_open_console_stream`.

## Assets and components

## CON-21. The assets are a manifest, under hashed names
`assets.rs` lists every file of `assets/` with `include_bytes!`; a test fails on a file that is not listed and on an entry with no file. Served names carry a content hash; the stylesheet names the fonts' hashed names. `console.css` is the kit's `paper.css` and `kafka.css` unchanged plus `design/console.css`, joined; a test says so. Inter and JetBrains Mono (Latin subset, OFL) are embedded with their licences kept beside them; no third-party script ships. The total is about 140 KB. Test: `assets::tests::every_file_of_assets_is_in_the_manifest_and_every_entry_has_its_file`, `assets::tests::a_licence_is_kept_for_every_third_party_file`, `assets::tests::the_stylesheet_is_the_kit_and_the_consoles_rules_joined`, `assets::tests::a_name_changes_with_its_content_and_only_then`, `assets::tests::every_served_file_is_found_by_its_hashed_name_and_the_stylesheet_names_its_fonts`, `assets::tests::the_shipped_files_weigh_what_the_plan_says`.

## CON-22. The components write what their golden files say
The shell, the buttons and states, the notices, the layout, and the table, form, filter and code components each have a golden file under `tests/golden/`; a change to the HTML is a diff in review. Variants are enums, so a misspelt class does not compile. Test: `tests/components::buttons_states_and_pills`, `tests/components::notices`, `tests/components::layout`, `tests/components::table_form_filter_and_code`, `ui::button::tests::a_class_is_built_from_its_variant_and_size`.

## CON-23. Numbers, sizes and times are written by pure functions
`format.rs`: spans of time in two units, numbers with separators, binary sizes with one decimal, milliseconds, civil UTC dates (checked over two hundred years of days). Test: `format::tests::a_span_is_its_two_largest_units`, `format::tests::a_longer_wait_never_reads_as_a_shorter_one`, `format::tests::a_number_has_thousands_separators_and_a_sign`, `format::tests::a_number_without_its_commas_is_the_number`, `format::tests::a_size_is_binary_with_one_decimal`, `format::tests::civil_time_matches_known_instants`, `format::tests::every_day_for_two_hundred_years_has_a_valid_date_and_follows_the_one_before`, `form::tests::a_component_is_decoded_as_a_browser_wrote_it`, `form::tests::decoding_never_panics_on_any_byte_string`. The form decoder has a fuzz target, `form` (`crates/kafka-console/fuzz`, 30 s in CI beside `origin`).

## The binary

## CON-24. The console is in the default build and the lean build has no HTTP
`doze-kafka` has the cargo feature `console`, on by default (decision D1); `--no-default-features` is the broker with no HTTP, no axum and no maud in its tree (`bun tools/check-console-catalogue.ts --lean-tree`). `--console=false` leaves the broker as it was before there was a console. Test: `tests/process console::console_false_leaves_the_broker_as_it_was`, `tests/process console::the_console_serves_its_pages_beside_the_broker_and_refuses_what_it_should`, `cli::tests::the_flags_the_conformance_harness_passes_are_all_accepted`. `--console` takes a strict boolean (`true`, `false`, `1`, `0`, `yes`, `no`, `on`, `off`, any case; anything else exits 2) and a console that is off says so in the start log. That the default build contains the console and the lean build does not is checked by `bun tools/check-console-catalogue.ts --lean-tree --default-tree` (`cargo tree` for both) and, in CI, by the `weight` and `weight-lean` jobs. Test: `cli::tests::console::the_console_flag_takes_a_strict_boolean_in_any_case_and_anything_else_stops_the_start`, `tests/process console::a_console_that_is_off_says_so_in_the_start_banner`.

## Limits, live updates and honesty (audit fixes)

## CON-25. A connection that does not say anything is closed
hyper's header-read timeout is armed before every request head, so a partial head, a connection that sends nothing and an idle keep-alive connection are each closed after 10 s (`HEADER_TIMEOUT`); a request that is not answered in 30 s (a body that stops arriving) is answered 408 with the security headers (`REQUEST_TIMEOUT`). An event stream is answered at once and then lives until either side ends it. `Limits` carries the numbers, so a test makes them small. Test: `tests/limits::the_documented_numbers_are_the_defaults`, `tests/limits::a_partial_head_a_silent_connection_and_an_idle_one_are_closed_after_the_timeout`, `tests/limits::a_body_that_stops_arriving_is_answered_408_and_the_connection_closed`.

## CON-26. At most 256 connections, counted in the broker's descriptor budget
A 257th connection is answered 503 with `Retry-After: 1` and the security headers, and closed; closing a connection gives its slot back. Each connection holds a descriptor, so `doze-kafka` subtracts the console's listener and its 256 connections from what is left for the partitions' files, as it does the Kafka connections (`descriptors::connections`). Test: `tests/limits::the_connection_over_the_cap_is_told_to_come_back_and_closing_connections_frees_the_slots`, `descriptors::tests::the_consoles_connections_and_listener_are_in_the_budget_when_there_is_a_console`, `tests/process console::the_consoles_connections_come_out_of_the_descriptor_budget_of_the_partitions`.

## CON-27. At most 64 event streams
The 65th `/events` is answered 503 with `Retry-After: 1`; a stream that ends gives its permit back (it is held by the stream's own state). Pages and fragments are not streams and are still served. Test: `tests/limits::the_stream_over_the_cap_is_refused_and_a_stream_that_ends_gives_its_slot_back`.

## CON-28. Home looks at every open transaction, or says how many it did not
ListTransactions answers every open id; DescribeTransactions takes any number, and the console asks in chunks of 100, up to 10,000 ids (`MAX_DESCRIBED`: a cost bound of a hundred calls, not a page size). Past that, Home says how many it did not look at ("N open transactions were not checked") and never "Nothing needs you" about them; a count found beside unchecked ones is a floor ("At least N transactions are close to their timeout"). The first version described 20 and said "Nothing needs you" with three near-timeout transactions at the end of 47 (audit A1). Test: `tests/txns::forty_four_open_transactions_do_not_hide_three_that_are_near_their_timeout`, `tests/txns::twenty_five_open_transactions_are_all_looked_at`, `tests/txns::where_it_has_to_stop_looking_it_says_so_and_never_claims_the_all_clear`, `words::tests::a_count_that_did_not_look_at_everything_is_a_floor_and_says_so`.

## CON-29. Home's state-dependent region is one fragment
The title, the sentence, the callout and both columns (which card sits where) follow the data together in the fragment `home-live`, one fetch and one look at the broker per tick; creating a topic turns "Your broker is up" and the first-request card into "Home" and the next-step card without a reload. Its advertised-listener part is the last result (CON-11). Test: `tests/home_live::creating_a_topic_flips_title_sentence_and_cards_in_the_one_fragment_without_a_reload`, and the swap itself in `tools/dom` (a whole region that changes shape is swapped in place).

## CON-30. Static files revalidate and are not ranged
A file's hashed name is its ETag; `If-None-Match` with it (a list, a weak tag or `*`) is a 304 with the cache rule and the security headers. A `Range` is not honoured: the whole file is the answer and `Accept-Ranges: none` says so. Test: `tests/headers::a_static_file_answers_304_to_its_own_etag_and_ignores_a_range`.

## What the boards show that the broker cannot supply yet

Each is tagged in the code with its reference and is a debt row of section J in `debt.md`.
- Home, "N apps appear on the wire, which reaches back to HH:MM, and the last request landed N s ago": the recorder (REALITY 1, slice 9e). The page says what the broker holds.
- Home, "no partition is offline and the data directory is writable": `partition_state()` (slice 9b) and no writability check. The all-clear says only that no transaction is near its timeout.
- Home, the link from the all-clear to the attention state: the state is chosen by data, so the example is plain text.
- Home, "Waiting for a client id this broker has not seen" and the link to `connect-first-request`: the first-request detector (slice 9e).
- Home quiet, "no clients": there is no connection table yet (slice 9e).
- The instance card, "Segments and indexes" for descriptors: the count is of all open files and sockets, and says so. "Idle memory" and "Threads" are measured on Linux only.
- Times are UTC and say so (no safe way to read the local zone).
- The instance card's "Idle memory" keeps the board's label and shows the resident memory now (J81); the sentence of `connect-advertised` that says a client "can always reach it again" says what the check did instead (J84).
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
## Topics, the reading half (slice 9b, `p9b-read`; the rules from CON-100 up)

The boards `topics-list`, `topics-none`, `topics-filtered`, `topic-detail`, `topic-messages-empty`, `topic-messages-filtered`, `topic-readcommitted`, `topic-record`, `topic-record-binary`, `topic-partitions` and `topic-offline`. The writing half (create, produce, add partitions, delete records, delete) is `p9b-write` and has its own rules.

## CON-100. Five routes, and the data chooses the state
`/topics` (list, none, filtered), `/topics/{topic}` and `/topics/{topic}/messages` (the Messages tab: detail, filtered, read_committed, empty, offline), `/topics/{topic}/messages/{partition}/{offset}` (a record, text or binary) and `/topics/{topic}/partitions`. The state is chosen by the data and the filters, never by the address: no topic is none, a name filter that matches nothing is filtered, a topic nothing was written to is empty, a topic with an offline partition is offline, `isolation=read_committed` is the read_committed state, any other filter is the filtered state. Test: `tests/boards_topics::every_board_of_the_reading_half_renders_and_is_written_for_the_tool`, `tests/live_topics::every_route_answers_as_the_console_and_has_the_boards_parts`, `tests/live_topics::an_empty_broker_is_the_none_state`, `tests/routes::every_row_of_the_table_has_a_handler_and_every_handler_a_row`.

## CON-101. Every fact is a protocol request or `ConsoleView`, as the console principal
Names, partitions and ids come from Metadata; offsets from ListOffsets; records from Fetch; policy and retention from DescribeConfigs; open transactions from ListTransactions and DescribeTransactions; producers and where each transaction began from DescribeProducers; the groups that read a topic from ListGroups and OffsetFetch. Only size on disk, last write and the offline cause come from `ConsoleView::partition_files`. Nothing is kept between requests. Test: `tests/live_topics::every_route_answers_as_the_console_and_has_the_boards_parts`, `tests/live_topics::the_reads_the_records_and_the_filters_answer_as_the_console_and_have_the_boards_parts`, `backend::requests::fetch::tests::a_fetch_never_waits_and_is_bounded_by_the_limit_it_was_given`, `backend::requests::offsets::tests::each_ask_is_one_entry_in_order_and_the_isolation_is_the_requests`.

## CON-102. A read is bounded
A Fetch has `max_wait 0`; a table reads 256 KiB a request, one record 1 MiB and 12 bytes (a Fetch returns its first batch whole whatever its size, so a larger record still arrives); one read makes at most 16 requests and takes at most 4 MiB of records; a batch inflates to at most 64 MiB (the broker's own limit when it accepted the batch); a page shows 25 records and at most 200; a key search looks at the newest 8 MiB. Test: `backend::requests::fetch::tests::the_isolation_levels_are_the_protocols_numbers`, `backend::requests::offsets::tests::a_request_is_bounded`, `model::query::tests::a_huge_limit_is_cut_and_a_long_key_is_refused_with_a_sentence`, `batch::look::tests::a_gzip_batch_is_read_through_its_codec_and_held_to_the_limit`.

## CON-103. A value is shown as the bytes say, with its true size
Absent is null (a deletion's value), zero bytes is empty. Valid UTF-8 is text, and text that is one whole JSON object or array is labelled JSON. Anything else is binary: a hex dump, eight bytes to a line with the text beside it, never a guess at text. Text is shown in full to 64 KiB and then cut at a character with the true size beside it; hex to 16 KiB. A table cell is the first 47 characters on one line, or `binary` and the first eight bytes. Control characters, line separators and bidirectional controls are written out as `⟨U+XXXX⟩`, so a value cannot move text around the page or hide in it. Test: `gather::decode::tests::nothing_and_empty_are_told_apart`, `gather::decode::tests::text_is_text_and_a_complete_json_object_or_array_is_json`, `gather::decode::tests::json_nested_past_the_depth_is_not_recognised_and_does_not_overflow_the_stack`, `gather::decode::tests::bytes_that_are_not_utf8_are_binary_with_a_hex_dump_and_a_preview_of_eight_bytes`, `gather::decode::tests::a_value_is_shown_in_full_to_64_kib_and_then_cut_with_its_true_size`, `gather::decode::tests::a_prefix_that_stopped_in_the_middle_of_a_character_is_still_text`, `gather::decode::tests::hex_is_cut_at_16_kib`, `gather::decode::tests::what_could_act_on_the_page_is_written_out`, `gather::decode::tests::a_cell_is_one_line_of_47_characters`, `gather::decode::tests::no_bytes_make_it_panic_and_the_size_is_always_the_true_one`, `tests/security::what_could_act_on_a_page_is_written_out_in_a_value_and_the_size_stays_true`.

## CON-104. Where a record stands for a read_committed consumer
Each record is Committed (below the last stable offset and not aborted), Above LSO (at or above it: held back) or Aborted (its transaction was aborted and the abort marker is past it). The walk does what a consumer does: an aborted transaction's records are skipped from its first offset until its abort marker, a producer's records after its marker are not, a window that begins inside a transaction still skips it, and markers are not rows. A read_uncommitted answer does not list the aborted transactions, so a page that meets a transactional batch asks for the list a read_committed consumer is given. Test: `gather::walk::tests::records_at_or_above_the_last_stable_offset_are_held_and_the_rest_are_committed`, `gather::walk::tests::an_aborted_transaction_is_skipped_from_its_first_offset_to_its_marker`, `gather::walk::tests::a_producer_after_its_marker_is_not_aborted_again`, `gather::walk::tests::a_window_that_begins_inside_the_transaction_still_skips_it`, `gather::walk::tests::the_producer_and_the_sequence_of_each_record_are_reported`, `gather::walk::tests::bytes_that_are_not_batches_end_the_walk_without_a_panic`.

## CON-105. read_committed and read_uncommitted show different rows around an open transaction
With a transaction open on a partition, `isolation=read_committed` stops at the last stable offset and lists none of the transaction's records; read_uncommitted lists them, each marked Above LSO. The read_committed page names the records held back, the transaction that holds them (where it began, from DescribeProducers), and what each isolation level gets. Test: `tests/live_topics::read_committed_stops_at_the_last_stable_offset_and_read_uncommitted_does_not`, `tests/boards_topics::the_empty_topic_the_filtered_read_and_the_committed_read_are_their_boards`.

## CON-106. A filter is validated and a bad value is a sentence
Partition, offset, time, key, isolation and limit come from the address (and a form of the same fields), are validated and bounded, and a value that cannot be used (negative, not a number, overflowing, a time that is not one, a key over 512 bytes, a limit over 200) is a sentence on the page and the rest of the query still works: never an error and never a 5xx. One box takes an offset when it is digits and a key otherwise. A time is milliseconds or a UTC civil time. Test: `model::query::tests::each_filter_is_read_and_written_back`, `model::query::tests::the_one_box_is_an_offset_when_it_is_digits_and_a_key_otherwise`, `model::query::tests::a_value_that_cannot_be_used_is_a_sentence_and_the_rest_still_works`, `model::query::tests::a_hostile_value_reaches_a_sentence_only_clipped_and_escaped_by_debug`, `format::read::tests::a_time_is_milliseconds_or_a_utc_civil_time`, `tests/live_topics::the_filters_find_by_key_by_offset_and_by_time_and_a_bad_value_is_a_sentence`, `tests/security::a_hostile_query_on_a_topic_page_is_a_sentence_never_markup_never_a_5xx`.

## CON-107. What a read did is said, and a search says how far it looked
Under a filtered table a sentence says how many records came, their offsets, whether the read reached the end of the partition, how many are at or above the last stable offset, and for a key search how many records it looked at and whether that was all of them. Test: `words::topics::tests::what_a_read_did_is_said_in_the_boards_words`, `words::topics::tests::a_search_that_found_nothing_says_how_much_it_looked_at`, `tests/live_topics::the_filters_find_by_key_by_offset_and_by_time_and_a_bad_value_is_a_sentence`.

## CON-108. The list says what the broker holds
Name, partitions, cleanup policy, size on disk and last write (from `partition_files`), whether a transaction is open, retention where the topic sets its own, "N segments" where a topic has more segments than partitions, and the groups that have committed offsets on it ("read by"). Internal topics are hidden; `__consumer_offsets`, which Metadata never lists, is shown as a constant row when asked for, said to be no log. The filter matches names as typed. "Written by" is not drawn: it needs the recorder. Test: `tests/boards_topics::the_list_has_the_boards_nine_topics_and_the_hidden_internal_one`, `tests/live_topics::every_route_answers_as_the_console_and_has_the_boards_parts`, `words::topics::tests::the_list_sentence_is_the_boards`, `gather::configs::tests::a_policy_is_delete_compact_or_both`.

## CON-109. `ConsoleView::partition_files`: size, last write and the offline cause, lazily
Per partition: bytes of its segments, how many segments, when the newest was last written, and why writes are refused after a failed flush. An open partition answers from its reader's snapshot; a closed one is read from its directory; nothing is opened or kept, so looking at a list does not change which partitions are open. The topic is checked for Describe with the console's caller, and an unknown topic and a refused one are not told apart. Test: `tests/view_partition_files::each_partition_reports_its_size_and_a_partition_never_written_reports_none`, `tests/view_partition_files::a_partition_that_was_closed_to_make_room_reports_the_same_from_its_directory`, `tests/view_partition_files::a_partition_whose_flush_failed_is_offline_with_the_cause_and_the_others_are_not`, `tests/view_partition_files::a_topic_the_caller_may_not_describe_or_that_does_not_exist_is_nothing`, `log::files::tests::a_log_of_several_segments_lists_each_with_its_base_and_size`, `log::files::tests::what_the_directory_says_is_what_the_open_log_says`, `log::files::tests::a_failed_flush_is_the_offline_cause_a_reader_can_read`, `log::files::tests::a_directory_that_is_not_there_has_no_segments_and_foreign_files_are_not_segments`.

## CON-110. An offline partition is named with its cause, not a time
A topic with a partition a failed flush took offline says which, what still works (reads), what is refused (KAFKA_STORAGE_ERROR, 56) and what restarting does. The broker keeps the cause and not the time of the failure, so the page does not say when. Test: `words::topics::tests::an_offline_partition_is_named_and_the_others_are_said_to_work`, `tests/boards_topics::the_partitions_and_the_offline_pages_are_their_boards`, `tests/security::hostile_names_and_values_are_inert_on_the_offline_empty_record_and_unknown_pages`.

## CON-111. A record is one page, complete
Its key, value, headers, time (and whose clock), codec, producer id, epoch and its own sequence number, the transaction it belongs to (from the coordinator while it is open) and where it stands for a read_committed consumer. A record that is not there (an offset outside the log, a partition that does not exist, a place compaction or a marker emptied) is a 404 page in the shell. Test: `tests/boards_topics::the_record_pages_show_the_text_and_the_bytes`, `tests/live_topics::a_compacted_topic_a_gzip_batch_headers_and_a_binary_value_are_shown_as_they_are`, `tests/live_topics::a_record_past_the_end_is_a_page_that_says_so_and_not_an_error`.

## CON-112. The page follows the stream, and the stream's vocabulary is closed
The list and the Messages page watch `topics-sentence`, `topics-table`, `topic-sentence` and `topic-messages`; each is also a plain GET (`/fragments/{id}` with the page's query) that answers 304 to an unchanged ETag. An id that is not in the vocabulary is refused. Until the recorder (slice 9e) the ticker is the 2 s floor timer, and the page is right without it. Test: `stream::tests::only_the_closed_vocabulary_may_be_watched`, `tests/security::the_stream_vocabulary_is_closed_and_has_the_topics_ids`, `tests/security::the_stream_takes_the_topics_ids_and_refuses_one_that_is_not_in_the_vocabulary`, `tests/live_topics_more::a_fragment_is_what_the_page_holds_and_an_unchanged_one_is_a_304`.

## CON-113. Hostile keys, values, headers and names are text
A record's key, value and header names and values, a topic name (every legal character, 249 bytes), a transaction id, an offline cause and every value of the query are shown escaped on every page and fragment: no script, no attribute break-out, no bidirectional control, no NUL, and a very long line is cut. Test: `tests/security::hostile_keys_values_headers_and_names_are_inert_on_every_topic_page`, `tests/security::hostile_topic_names_in_the_list_come_out_inert`, `tests/live_topics_more::hostile_records_headers_and_names_from_a_real_broker_are_inert_on_every_page`, `pages::topics::table::tests::a_hostile_key_and_value_are_text_in_their_cells_and_the_offset_is_the_link`.

## CON-114. A refusal is Kafka's, as a client would see it
On an ACL-on broker a principal that may describe a topic but not read it sees the topic and then "The broker refused the read: TOPIC_AUTHORIZATION_FAILED (29)."; one that may not describe it sees no such topic; one with no right at all sees an empty broker. The console itself is a super user and reads everything; the refusal is shown by acting as another caller (`Backend::acting_as`, tests and examples only). Test: `tests/live_topics_more::on_an_acl_broker_a_principal_that_may_describe_but_not_read_is_refused_in_kafkas_words`, `words::topics::tests::a_timeout_is_in_seconds_up_to_two_minutes_as_the_board_writes_it`.

## CON-115. A topic name of 249 bytes fits
The longest legal name is listed, opened and linked, and one byte more is no topic (a 404 page, no request to the broker). Test: `tests/live_topics::a_topic_name_of_249_bytes_fits_and_a_longer_one_is_no_topic`.

## CON-116. The boards pass the structure gate in both renders
The eleven boards pass `bun tools/board-diff.ts` against the fixture render and, for the ten that a real broker draws, against the live render (`target/board-diff-live/`); `topic-offline` cannot be seeded over the real file system and is fixture-rendered (the offline cause is held by `CON-109`). Every allowed difference has a reason and an `expires`. Test: `tests/boards_topics::the_list_has_the_boards_nine_topics_and_the_hidden_internal_one`, `tests/boards_topics::the_topic_page_has_the_open_transaction_the_toolbar_and_the_ten_newest`, `tests/live_topics::every_route_answers_as_the_console_and_has_the_boards_parts`.

### What these boards show that the broker cannot supply yet

Tagged in the code and in `debt.md` J21 to J40.
- "On the wire" on the topic page and "What the wire recorded" on the offline page, and "written by" on the list: the recorder (slice 9e).
- Record counts on the list rows: ListOffsets would open every partition just to draw a list; the topic page has the counts.
- "Copy as kcat" on a record: a hostile value would be put in a command line; the page copies the value.
- When a partition went offline: the broker keeps the cause, not the time.
- The "Who this holds back" lede names a group and its lag: lag is slice 9c; the page says where a read_committed consumer stops.

## Slice 9c: Groups and lag (CON-200 to CON-215)

Sixteen boards: `groups-list`, `groups-none`, `group-detail`, `group-detail-848`, `group-rebalancing`, `group-members`, `member-detail`, `group-lag`, `lag-no-commits`, `lag-ahead`, `lag-expiring`, `group-reset-offsets`, `group-reset-refused`, `group-delete`, `group-delete-refused`, `group-offset-delete`.

## CON-200. Lag is a subtraction only when it is one
Lag is the log end (ListOffsets) minus the committed offset (OffsetFetch). A partition never committed (OffsetFetch answers -1) is `NoCommit`, never lag `end`; a commit beyond the log end (a topic recreated under an app that kept its position) is `PastEnd(by)`; a partition with no log end is `Gone`. A negative lag is never produced and never shown. Tests: `lag::tests::the_table_of_cases`, `lag::tests::lag_is_never_negative_and_a_behind_group_is_exactly_the_difference`, `lag::tests::a_bar_is_zero_when_not_behind_two_at_least_when_behind_and_never_over_a_hundred`.

## CON-201. A commit past the end of a recreated topic is said, not subtracted
Over a real broker: commits made after a topic was deleted and made again, shorter, show "3 past the end" on the Lag page and on the list ("orders-0 is 3 beyond its log"), with the callouts that say what Kafka accepted and what a consumer would do. Test: `live_groups::a_commit_beyond_the_end_of_a_recreated_topic_is_past_the_end_never_negative_lag`.

## CON-202. A group with no commit shows "No commits yet", not the log end
A group whose members are subscribed to a topic but have committed nothing shows its partitions as "No commits yet" and the tab says "lag not known". Test: `live_groups::a_group_with_no_commit_shows_no_commits_and_not_the_log_end_as_lag`.

## CON-203. The expiry date is the moment the sweep takes the commit
`groups::expiry::first_expiry` applies the rules of `groups::expired` (empty since, own age, the presumed members after a restart, subscribed topics kept, a group in a rebalance not looked at) and is held to `expired` itself at the moment before, at and after each date over a grid of groups. The date on the Lag page is that date, and a sweep at that moment takes the commit. Tests: `groups::expiry::tests::the_date_is_the_one_the_sweep_acts_on_over_a_grid_of_groups`, `live_groups::the_expiry_date_shown_is_the_moment_the_sweep_takes_the_commit`.

## CON-204. What the protocol hides comes from `ConsoleView::group_detail`
The classic leader, the generation, the session and rebalance timeouts of each member, how long ago each was heard, which member a rebalance waits for and how long is left, a consumer group's epochs, the date a group became empty and the retention: read in the group's own task (one consistent look), Describe-checked on the group, and the offsets filtered by Describe on their topic. No transition changed: a member's `heard` and a rebalance's `began` are written where the timers were already armed. Tests: `live_groups::a_classic_group_with_a_member_and_commits_shows_its_members_lag_and_what_the_protocol_hides`, `live_groups::a_rebalance_in_progress_says_who_it_waits_for`.

## CON-205. A KIP-848 group is described by its own API
A consumer group of the new protocol is Dead to DescribeGroups; the page asks ConsumerGroupDescribe and shows epoch, target epoch, assignor, and each member's target and assigned partitions. Test: `live_groups::a_consumer_group_of_the_new_protocol_is_described_by_its_own_api`.

## CON-206. The sentences say only what the group machine says now
No sentence gives the cause of a rebalance (a member left, timed out, a topic changed): that needs the event ring of slice 9f. Tests: `groups_words::tests::a_rebalance_names_who_is_waited_for_and_the_time_left_and_never_a_cause`, `groups_words::tests::a_consumer_group_in_step_says_nothing_is_moving_and_gives_no_cause`, `groups_words::tests::a_stable_classic_group_is_told_by_its_member_its_topics_and_its_lag`, `groups_words::tests::the_lag_sentences_per_state`.

## CON-207. A reset is a commit with no generation, and the broker's refusal is shown in its words
Reset offsets sends OffsetCommit with generation -1 and no member (what `kafka-consumer-groups --reset-offsets` sends); the preview is the same computation as the write (earliest, latest, an offset, a time, a shift, current; each kept within the log). On a group with a member Kafka answers UNKNOWN_MEMBER_ID (25) and the page shows that code, its words ("The coordinator is not aware of this member."), the console's reading of the rule and what would pass. Tests: `live_groups::a_reset_of_an_empty_group_moves_its_offsets_and_a_group_with_a_member_is_refused_in_kafkas_words`, `groups_words::tests::the_reading_of_a_refused_reset_counts_the_members_and_a_delete_names_the_rule`, `backend::requests::groups_read::tests::a_reset_is_a_commit_that_names_no_generation_and_no_member`.

## CON-208. Delete group is refused with NON_EMPTY_GROUP and done when empty
DeleteGroups on a group with a member shows NON_EMPTY_GROUP (68) and "The group is not empty."; on an empty group it deletes and returns to the list. Test: `live_groups::deleting_a_group_is_refused_with_members_and_done_when_empty`.

## CON-209. Delete offsets forgets them, and a subscribed topic is refused
OffsetDelete on an empty group forgets the commits and lands on the Lag page; on a topic a member is subscribed to Kafka answers GROUP_SUBSCRIBED_TO_TOPIC (86) and the dialog shows it. Test: `live_groups::deleting_offsets_forgets_them_and_a_subscribed_topic_is_refused`.

## CON-210. An ACL-on broker refuses as a client would be refused
With ACLs on, a caller that may only DESCRIBE a group is refused a reset and a delete with GROUP_AUTHORIZATION_FAILED (30), and a group it may not describe is not told apart from no group. Test: `live_groups::on_a_broker_with_acls_a_caller_without_the_right_is_refused_as_a_client_would_be`.

## CON-211. A group id is hostile input
A group id with `/`, `?`, `%`, spaces, unicode, a `<script>`, an attribute break, or 255 bytes is escaped in every page and percent-encoded in every link. Test: `live_groups::a_hostile_group_id_is_escaped_and_encoded_on_every_page`.

## CON-212. The group requests are bounded and name what they do
DescribeGroups and ConsumerGroupDescribe name at most 100 groups; an OffsetFetch asks for every partition of one group and waits for a stable answer; a commit or a delete names at most 4,096 partitions. Tests: `backend::requests::groups_read::tests::a_describe_names_at_most_a_hundred_groups`, `backend::requests::groups_read::tests::an_offset_fetch_asks_for_every_partition_of_one_group_and_waits_for_a_stable_answer`, `backend::requests::groups_read::tests::the_deletes_name_what_they_delete`.

## CON-213. The assignment bytes are read without trusting their counts
A member's assignment and subscription (DescribeGroups hands the bytes over) are decoded with every count checked against the bytes left. Test: `gather::protocol::tests::bytes_that_are_not_an_assignment_or_a_subscription_are_not_read`.

## CON-214. The sixteen boards pass the structure gate in both renders
The fixture render (the larder: analytics, fulfilment, payments-workers, search-indexer, run through the same assembly as a live group) and the live render over a real broker both pass `bun tools/board-diff.ts --require` the sixteen ids, with every allowed difference carrying a reason and an `expires` (the wire dock: 9e; the Story page: 9f; the rest: close). Tests: `boards_groups::every_board_of_the_slice_renders_and_is_written_for_the_tool`, `boards_groups::the_list_has_the_boards_four_groups_in_order_with_their_lag_words`, `boards_groups::the_group_pages_say_what_the_boards_say`, `boards_groups::the_consumer_group_shows_target_and_assigned_and_the_rebalance_shows_who_is_waited_for`, `boards_groups::the_lag_pages_never_show_a_negative_number_and_say_where_a_commit_is`, `boards_groups::the_refusals_carry_kafkas_code_and_words`.

## CON-215. The stream vocabulary of the Groups pages is registered once
Five fragment ids (`groups-live`, `groups-table`, `group-sentence`, `group-lag-sentence`, `group-lag-table`) live in `stream_groups.rs` and are accepted by the stream through one line. Test: `stream_groups::tests::every_id_a_page_watches_is_in_the_vocabulary_and_the_pages_use_these_constants`.

### What these boards show that the broker cannot supply yet

Tagged in the code and in `debt.md` J91 to J120.
- "What just happened" wire docks: the request recorder (slice 9e).
- The Story tab and its count, "left 3 s ago", the last rebalance's age: the group event ring (slice 9f).
- Members that left a group: the machine forgets them; only members present are drawn.
## Config, the topic's configs (slice 9d, `p9d`; the rules from CON-250 up)

## CON-250. The eight Config boards, fixtures and a real broker
Each board is rendered from the larder and from a real broker and compared by `tools/board-diff.ts`; the edit dialogs and the reset dialog are drawn as overlays on their parent, the Config page. Test: `tests/boards_config::every_board_of_the_slice_renders_and_is_written_for_the_tool`, `tests/live_config::the_eight_boards_are_rendered_from_a_real_broker`.

## CON-251. The page lists what the broker lists, with source, Kafka's text and when a change counts
Every key DescribeConfigs lists for the topic is on the page (the replication group opens on request), with its value, its source, Kafka's first sentence, and a sentence for when a change counts whose seconds are the broker's own flags. Test: `tests/live_config::the_page_lists_what_the_broker_lists_with_its_source_and_when_a_change_counts`.

## CON-252. Every key the broker lists has an effect sentence
`config_effects::moment` has an entry for each of the 33 keys, and says "this broker keeps it and does nothing with it" for the ones it does not act on; a name that is not listed has none. Test: `tests/live_config::every_key_the_broker_lists_has_an_effect_sentence`, `config_effects::tests::every_listed_key_has_a_moment_and_a_sentence_and_a_name_that_is_not_listed_has_none`.

## CON-253. A value is set with IncrementalAlterConfigs SET and read back
Test: `tests/live_config::a_value_is_set_with_the_call_a_client_sends_and_read_back_from_the_broker`.

## CON-254. A list is appended to and subtracted from
Test: `tests/live_config::a_list_is_appended_to_and_subtracted_from`, `config_pass::tests::a_list_after_an_operation_is_what_the_boards_draw`.

## CON-255. The two compaction lags are one request
A crossed pair is refused whole, in Kafka's words, and nothing changes. Test: `tests/live_config::the_two_lags_are_saved_together_and_a_crossed_pair_is_refused_whole`.

## CON-256. What would pass is what the broker accepted
A candidate named by a refusal is shown only after `validate_only` took it; the other ranges are the broker's answers now. Test: `tests/live_config::a_refused_value_shows_kafkas_words_and_only_what_the_broker_accepted_as_what_would_pass`, `config_pass::tests::the_values_kafka_names_in_a_refusal_are_the_ones_to_try`.

## CON-257. A reset is IncrementalAlterConfigs DELETE
Test: `tests/live_config::a_reset_removes_the_topics_value_and_the_broker_takes_over`.

## CON-258. Check is the same call with validate_only
Test: `tests/live_config::a_check_is_the_same_call_with_validate_only_and_changes_nothing`.

## CON-259. An empty form is the dialog again and no call
Test: `tests/live_config::an_empty_value_is_the_dialog_again_with_the_error_and_no_call`.

## CON-260. Without the right to alter, the save is refused as a client would be
Test: `tests/live_config::on_a_broker_with_acls_the_page_reads_and_the_save_is_refused_as_a_client_would_be`.

## CON-261. Hostile text is escaped and cannot forge a line of the call
Test: `tests/live_config::hostile_text_is_escaped_and_cannot_forge_a_line_of_the_call`, `config_calls::tests::what_was_typed_cannot_start_a_line_of_its_own`.

## CON-262. A GET of the Config pages never changes the broker
Test: `tests/live_config::a_get_of_the_config_pages_never_changes_the_broker`.

## CON-263. Every word under a value is made from the key and the value
Durations, sizes, "no limit", the group a key is in, Kafka's first sentence. Test: `config_words::tests::a_value_is_said_in_words_where_it_helps_and_left_alone_where_it_does_not`.

## CON-264. The dialogs teach from the row, the table and the typed values
Test: `config_teach::tests::the_lags_are_taught_together_and_here_says_what_the_typed_pair_means`.

### What these boards show that the broker cannot supply yet

Tagged in the code and in `debt.md` J121 to J130.
- The cleaner's state ("61 % of the log is dirty", "not due until 11.2 MiB more"): the cleaner keeps none a console may read.
- The name of the flag behind a Static flag (`--segment-bytes`): the table knows the broker config, not the flag.
- Segment sizes ("Segments 1 to 7 keep their size", "the active segment (0.6 MiB)"): the dialog does not read the log's files.

## Settings and Access (slice 9h, CON-550 to CON-581)

The Settings page (`/settings`, its edit dialog `/settings/config/edit` and the saved and refused answers of its POST) and the Access page (`/access`, the Create ACL and Delete ACL dialogs and their POSTs). Boards: `settings-home`, `broker-config-edit`, `broker-config-refused`, `broker-config-saved`, `access-open`, `access-secured`, `access-acl-create`, `access-acl-delete`. Debt rows J261 to J278.

## CON-550. The Settings boards render, whole, and are written for the board-diff
Test: `tests/boards_settings::every_board_of_the_slice_renders_and_is_written_for_the_tool`.

## CON-551. The Settings page has the sections of its board, in order, with the board's numbers
Broker configs, Version profile, Intervals, Weight, Data, Flags. Test: `tests/boards_settings::the_settings_page_has_the_parts_of_its_board_in_order`.

## CON-552. The edit dialog is the board's, over the Settings page
Config, Applies to, New value with the teaching (Now, Allowed, Takes effect, Who sees it), the call. Test: `tests/boards_settings::the_edit_dialog_is_the_boards_over_its_parent`.

## CON-553. The saved and refused states carry their boards' words
Test: `tests/boards_settings::the_saved_and_refused_states_carry_their_boards_words`.

## CON-554. The Access boards render, whole, and are written for the board-diff
Test: `tests/boards_access::every_board_of_the_slice_renders_and_is_written_for_the_tool`.

## CON-555. The open Access page has the parts of its board, in order
Test: `tests/boards_access::the_open_page_has_the_parts_of_its_board_in_order`.

## CON-556. The secured Access page has the parts of its board, in order, with eight ACLs and no iteration count
Test: `tests/boards_access::the_secured_page_has_the_parts_of_its_board_in_order`.

## CON-557. The two ACL dialogs are the boards', over the Access page
Test: `tests/boards_access::the_two_dialogs_are_the_boards_over_their_parent`.

## CON-558. The page says what a secured broker holds, by name only
Mechanisms, the user from flags and the stored one (`Flags` and `Stored`), the super users without the console's own, the default, the failed-login delay. Test: `tests/live_access::the_page_says_what_a_secured_broker_holds_by_name_only`.

## CON-559. An ACL made and deleted through the pages is seen through DescribeAcls
Create is CreateAcls; Delete is DeleteAcls with every field of the one ACL, so no other can match; a second delete says the ACL is gone. Test: `tests/live_access::an_acl_made_and_deleted_through_the_pages_is_seen_through_describeacls`.

## CON-560. Each refusal is Kafka's own, and a missing field asks the broker nothing
INVALID_REQUEST with the broker's sentence (a principal with no colon, the CLUSTER resource's one name), CLUSTER_AUTHORIZATION_FAILED (31) for a caller that may not alter the cluster, for each write and for the page. Test: `tests/live_access::each_refusal_is_kafkas_own_and_a_missing_field_asks_the_broker_nothing`.

## CON-561. Hostile ACL names are escaped and inject neither a header nor a line of the call
Test: `tests/live_access::hostile_names_are_escaped_and_inject_nothing`.

## CON-562. A GET of either page or dialog changes nothing
Test: `tests/live_access::a_get_of_either_page_or_dialog_changes_nothing`.

## CON-563. The console is a super user: a broker that denies by default still shows its page
Test: `tests/live_access_secrets::the_console_is_a_super_user_so_a_denying_broker_still_shows_its_page`.

## CON-564. No planted password, salt or key is in any page, fragment, stream or Debug
A real SCRAM user from flags (planted password) and a stored one (planted salt and salted password), each secret in its text, hex and base64, and the stored and server keys read from the broker's own verifiers. Test: `tests/live_access_secrets::no_planted_password_salt_or_key_is_in_any_page_fragment_stream_or_debug`.

## CON-565. The stream of a secured console carries ticks and nothing else
Test: `tests/live_access_secrets::the_stream_of_a_secured_console_carries_ticks_and_nothing_else`.

## CON-566. The Settings page lists the five, their sources and the intervals of this run, from the broker's own answers
Test: `tests/live_settings::the_page_lists_the_five_with_their_sources_and_the_intervals_of_this_run`.

## CON-567. A save is the call the dialog drew, and the broker keeps it for the scope chosen
All brokers is the resource `""` (source 3), this broker its node id (source 2). Test: `tests/live_settings::a_save_is_the_call_the_dialog_drew_and_the_broker_keeps_it_for_the_scope_chosen`.

## CON-568. A check changes nothing, and each refusal is Kafka's own
Too small, not a number, a config that cannot change (a dry run, `validate_only`, asked by the link of the page), a forged key (always a dry run), an empty value (no call), the select moved (no call). Test: `tests/live_settings::a_check_changes_nothing_and_each_refusal_is_kafkas_own`.

## CON-569. The rule the dialog teaches is the rule the broker holds
The least value of each config is taken and one less is refused, by the broker. Test: `tests/live_settings_more::the_rule_the_dialog_teaches_is_the_rule_the_broker_holds`.

## CON-570. Hostile text on Settings is escaped and cannot start a line of the call
Test: `tests/live_settings_more::hostile_text_is_escaped_and_cannot_start_a_line_of_the_call`.

## CON-571. A caller without rights is refused in Kafka's words, and the page still draws
Test: `tests/live_settings_more::a_caller_without_rights_is_refused_in_kafkas_words_and_the_page_still_draws`.

## CON-572. The Settings boards and the open Access board render from a real broker
Written to `target/board-diff-live/` for the board-diff. Test: `tests/live_boards_settings_access::the_settings_boards_and_the_open_access_board_render_from_a_real_broker`.

## CON-573. The secured Access board and the two ACL dialogs render from a real broker
Test: `tests/live_boards_settings_access::the_secured_access_board_and_the_acl_dialogs_render_from_a_real_broker`.

## CON-574. The flags table is clap's own list of flags
A flag added to the command line is in the table with no change to it, and the test fails until the flag has a group (or is named as accepted and ignored). Test: `doze-kafka/src/facts/tests::the_table_is_exactly_the_list_of_flags_clap_has_less_the_ones_that_are_accepted_and_ignored`.

## CON-575. A password and a key file are never in the table
`--sasl-user` says how many were given; `--tls-key` says one was; a flag whose value is named like a secret that the table does not redact fails the test. Test: `a_password_and_a_key_file_are_never_in_the_table`, `every_flag_whose_value_is_named_like_a_secret_is_one_the_table_redacts`.

## CON-576. What a delete comes to is decided by the broker's own rules
The ACLs that would be left, judged by `kafka_auth::authz::Rules` (a DENY wins, an ALLOW implies Describe, an ACL for anybody else turns the default off): the board's case reads "User:orders is then refused: its next Produce is answered TOPIC_AUTHORIZATION_FAILED (29)". Test: `acl_words::tests::the_boards_delete_is_judged_as_the_board_says_it`.

## CON-577. Every API a Kafka profile offers has a family on the version profile
Test: `settings_words::tests::every_api_a_kafka_profile_offers_has_a_family_and_a_place_and_no_two_share_one`.

## CON-578. The weight limits the page names are the ones weight.md writes
Test: `settings_words::tests::the_limits_are_the_ones_weight_md_writes`.

## CON-579. A start-up warning holds exactly when the broker would say it
SEC-188 (console beyond loopback with SASL or TLS on), SEC-189 (SASL without TLS beyond loopback), SEC-191 (SASL with `--acl-default allow`). Test: `access_words::tests::each_warning_holds_exactly_when_the_broker_would_say_it_at_start`.

## CON-580. The broker's view names users and super users and holds no secret
`ConsoleView::access` (DESCRIBE on the cluster), `intervals` (DESCRIBE_CONFIGS) and `data_size`. Test: `view::tests::access_names_the_flag_users_and_the_super_users_and_never_what_they_log_in_with`.

## CON-581. The writes the console issues are the ones the boards draw
CreateAcls and DeleteAcls join the list. Test: `actions::tests::actions_match_boards`.

### What these boards show that the broker cannot supply yet

Tagged in the code and in `debt.md` J261 to J278.
- "Today fulfilment speaks Fetch v12 and orders Produce v9" (the wire recorder, 9e), "its last round was on inventory.levels at 21:31" (the cleaner's rounds, 9g) and "analytics' offsets expire in 3 d 2 h" (a group's own clock, Groups): the sentences say what the broker holds now and name none of them.
- The iteration count column of the users table: not read by any code of the console (J264).
- Idle memory and threads on a system that does not let a process read them (macOS): the tile says "Measured on Linux only".
