# The console (Phase 9)

What the web console (`crates/kafka-console`) does, rule by rule, and the test that holds each. Slice 9a is built (the crate, the shell, Home and Connect, the advertised-listener check, the stream skeleton, the feature in `doze-kafka`); later slices add their rules here. The security rules of the console (its reserved principal, the guard, the same-origin proof, the headers, escaping, the address) are SEC-240 to SEC-249 in `security.md` and are not repeated; their tests are named there.

The design is the canvas "doze-kafka console direction" (`docs/port/console/`), the plan `docs/port/phase9.md`. Where a board shows something the broker cannot yet say, the page says what it can and the place in the code is tagged with the REALITY or GROUNDING reference; the list is in "What the boards show that the broker cannot supply yet" below.

The numbers are checked by `tools/check-console-catalogue.ts` (`bun tools/check-console-catalogue.ts`; add `--lean-tree` to also check that the lean build has no HTTP in its tree): no CON-n is defined twice, none is referenced without a definition, and every test name quoted in this file is a `fn` in the Rust sources.

Slice R5 (2026-11-13) removed the server-rendered console (maud, htmx, Alpine, idiomorph, `/classic/`). A rule that only described it is marked **retired in R5** in its heading and kept, with the reason; a rule about the data that lost its page test is marked **Untested since R5**; a test or file that went is named unquoted with "(removed in R5)". The React console's rules start at CON-1000.

## The routes and the shell

## CON-1. The route table is the router (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
routes.rs (removed in R5) is the one list (path, method, mutates, the Kafka API behind it, the screens it serves); the router is built from it and from nothing else; `Handler` is matched without a wildcard, so a row without a handler does not compile. Every row answers; a path that is no row is a 404. Home's three boards (`home`, `home-quiet`, `home-attention`) are states of the one route `/`, chosen by data. Test: `tests/routes::every_row_of_the_table_has_a_handler_and_every_handler_a_row`, routes::tests::no_screen_is_served_by_two_routes_and_no_route_is_listed_twice (removed in R5), routes::tests::the_five_boards_of_slice_9a_are_served_by_two_routes_and_the_advertised_page (removed in R5).

## CON-2. An unknown path is a 404 page in the shell, a wrong method a 405
Both are drawn in the shell, the path is shown escaped. Screens a later slice builds (`/live`, `/topics`, `/access`, `/settings`, `/lab`, `/topics/new`, `/topics/{topic}/producers`) are rows of the table that answer a page saying they are not built yet. Test: `tests/routes::an_unknown_route_is_a_404_page_in_the_shell_and_a_wrong_method_a_405`.

## CON-3. Every link is built from a base (`--console-prefix`)
`Base` turns a path into a URL under the prefix (`/` is the root; `kafka`, `/kafka/` and `/kafka` are one prefix; a prefix is always a path, never `//host`); the root is registered as both `/k` and `/k/`. Test: `base::tests::a_prefix_is_one_path_whatever_slashes_it_was_written_with`, `base::tests::a_url_is_the_prefix_then_the_path`, `base::tests::a_request_path_is_stripped_of_the_prefix_and_only_of_whole_segments`, routes::tests::a_prefix_registers_the_root_both_ways_and_prefixes_the_rest (removed in R5), ui::shell::tests::the_rail_marks_the_active_space_and_every_link_is_under_the_base (removed in R5), `tests/routes::every_href_and_src_of_every_page_resolves_under_the_base` (run with no prefix and with `/kafka`), `tests/process console::a_prefix_moves_every_page_under_it`, `cli::tests::console::the_console_is_on_by_default_and_off_only_for_false_and_the_prefix_is_a_path`.

## CON-4. Every link and asset of every page resolves
The crawl starts at every page, follows every `href` and `src` that starts with a slash and fails on a 404 or a link outside the base: no dangling link, no missing asset. Test: `tests/routes::every_href_and_src_of_every_page_resolves_under_the_base`.

## CON-5. The shell
A `<!DOCTYPE html>` document with `lang`, one `<main>`, a skip link, the top bar (the address with a copy chip, the version profile, the security chip, which says what the broker has on and links to Access), the rail of the six spaces with the active one marked, the breadcrumb, a banner hidden until the stream is lost, and a banner across the top when the console is served beyond loopback. Left out and tagged: the pinned items and the key hints of the rail (no store, no keyboard layer yet), the search (drawn, disabled), the "Reached from" line (canvas scaffolding). Test: ui::shell::tests::the_document_is_html_with_a_language_a_title_and_one_main (removed in R5), ui::shell::tests::the_top_bar_says_the_address_the_profile_and_what_is_on (removed in R5), ui::shell::tests::the_search_is_drawn_as_the_board_draws_it_and_not_offered (removed in R5), `facts::tests::the_chip_says_what_is_on_in_the_boards_vocabulary`, `facts::tests::the_chrome_locks_the_chip_when_anything_protects_the_broker`.

## Home

## CON-6. Home is in the state its data calls for
Attention when a transaction is close to its timeout, quiet when the broker holds no user topic, steady otherwise; attention wins. The state is chosen from Metadata, ListTransactions and DescribeTransactions through `Broker::handle` as `User:__console`. Test: `gather::home::tests::the_state_follows_the_data_and_attention_wins`, `tests/live::a_broker_with_no_topics_is_the_quiet_home_and_with_one_it_is_the_steady_one`, `tests/live::the_console_reads_a_broker_that_refuses_everyone_else_because_it_is_a_super_user`. The state is judged on every open transaction, CON-28.

## CON-7. "Close to its timeout" is two thirds of it
The one condition Home draws as needing someone: an open transaction whose elapsed time is two thirds of its timeout or more (the board's own example, 41 s of 60 s, meets it). The page names the transaction, producer and epoch, the first partition, when it opened, the bar, and what a `read_committed` consumer is held back by (the last stable offset against the high watermark, from ListOffsets). Test: `gather::attention::tests::two_thirds_of_the_timeout_is_close`, `gather::attention::tests::a_timeout_of_nothing_is_never_close_and_nothing_overflows`, `tests/live::an_open_transaction_close_to_its_timeout_is_what_needs_someone`, `tests/boards::the_attention_home_carries_the_boards_transaction`.

## CON-8. Home says what the broker holds, never what it cannot know
The sentence under the title is static (it holds no number: CON-805); the running time, the counts and the wire's facts are the metric tiles above the cards; the quiet state claims no client; the all-clear is claimed only for what was looked at. Test: `words::tests::the_steady_sentence_is_static_and_holds_no_number`, `words::tests::the_quiet_sentence_names_the_listener_and_claims_no_client`, `words::tests::the_attention_headline_counts`.

## CON-9. The instance card says what it measured and says when it did not
Started (UTC, absolute; how long ago is on the hover), last stop (clean, not clean, or none yet on a new data directory), open partitions of the bound, open files, memory and threads where the platform lets a safe process read them (Linux `/proc`; elsewhere "Not measured" and "Measured on Linux only"). The memory row keeps the board's label "Idle memory" and says under the figure that it is what is resident now, read from `VmRSS` (kB, whatever the page size), not a measured idle (J72); the descriptors are the open files without the listing's own handle, the version profile, access in words. The facts that the protocol hides come from `ConsoleView::instance()` (feature `inspect`), which checks DESCRIBE. Test: `words::tests::open_partitions_are_of_the_bound_when_there_is_one`, `gather::home::tests::an_instance_card_counts_the_run_from_its_start`, `facts::tests::the_access_row_of_home_reads_as_the_board_does_when_open`, `kafka-broker view::tests::a_new_data_directory_has_no_last_stop_and_a_clean_stop_is_remembered`, `kafka-broker view::tests::a_run_after_a_stop_that_vouched_for_nothing_says_unclean`, `kafka-broker view::process::tests::the_resident_set_is_the_vmrss_line_in_kb_whatever_the_page_size`, `kafka-broker view::process::tests::the_listing_of_the_open_files_does_not_count_its_own_handle`, `tests/home_live::the_memory_figure_says_it_is_what_is_resident_now_under_the_boards_label`.

## The advertised-listener check

## CON-10. Three rules, from real dials
The check does what a client does: (1) dial the listener and ask ApiVersions v0; (2) ask Metadata, in process, what the broker says clients should reconnect to; (3) dial that address back and ask ApiVersions. A TLS listener is only dialled. Each rule is ok or bad with the board's words, and the failure says why in a few words (`connection refused`, `no such host`, `timed out after 700 ms`, `connected, but it did not answer ApiVersions`), never the system's own text. The sentences around the rules say only what the check did: no "can always reach it again", and no "a client that reached" an address whose rule failed (words::tests::the_instance_lede_claims_only_what_the_check_found (removed in R5)). Test: `probe::judge::tests::an_address_that_answers_is_reachable_in_the_boards_words`, `probe::judge::tests::a_wrong_advertised_address_is_the_boards_bad_case`, `probe::judge::tests::a_listener_that_does_not_answer_fails_the_first_rule`, `probe::judge::tests::metadata_that_names_something_else_or_nothing_fails_the_second`, `probe::judge::tests::tls_is_only_dialled`, `probe::tests::a_failed_dial_says_why_in_a_few_words`, `probe::tests::something_that_is_not_kafka_is_said_not_to_be`, `probe::tests::the_request_is_a_framed_api_versions_v0_with_the_client_id`, `probe::tests::an_answer_must_echo_the_correlation_id_and_carry_no_error`, `tests/live::the_check_passes_against_a_real_listener_and_the_page_says_so`, `tests/live::a_deliberately_wrong_advertised_address_fails_the_third_rule_in_words`.

## CON-11. The check never holds a page for long
Every dial has its own deadline (700 ms, 1 s), the whole check 1.5 s; one that does not finish claims nothing ("Not checked"), and a result that claims nothing is not reused. It runs on demand ("Check again") and at a page load (a result under 30 s old that says something is shown without dialling), never on a timer and never for a live fragment: the fragments the stream refetches show the last result (`Prober::peek`) and dial nothing, so a page left open dials once, not every 30 s. Callers that arrive while a check is running share its result (single-flight), so many at once are one set of dials. A dead advertised address therefore cannot slow a fragment. (The first text of this rule said "once at page load, never on a timer" and the code dialled from every fragment fetch: audit A2.) Test: `probe::tests::a_closed_port_is_refused_and_a_silent_one_times_out_and_neither_waits_longer_than_its_deadline`, `probe::judge::tests::a_dial_that_did_not_finish_claims_nothing`, `probe::judge::tests::a_rule_is_ok_exactly_when_the_verdict_can_be_reachable`, `tests/live::a_name_that_cannot_be_resolved_is_not_reachable_and_the_check_stays_inside_its_budget`, `tests/live::a_listener_that_is_gone_fails_the_first_rule_and_a_result_is_reused_for_thirty_seconds`, `tests/home_live::the_live_fragments_never_dial_and_a_page_load_dials_once_in_thirty_seconds`, `tests/home_live::a_dead_advertised_address_does_not_slow_a_fragment`, `tests/home_live::a_result_is_reused_until_it_is_old_and_callers_at_once_share_one_set_of_dials`, `tests/home_live::a_probe_against_a_black_hole_finishes_in_its_deadline_and_a_result_that_claims_nothing_is_not_reused` (the dial deadlines against a listener that accepts and never answers; the whole-check deadline is an outer bound that no single dial can reach, so that test pins the documented numbers).

## CON-12. "Check again" is a plain form
A POST to `/connect/advertised/check` with `back=home` or `back=advertised`: without a script it answers 303 back to the page, with the script it answers the fragment to swap in; anything else is 400. It needs the same-origin proof (SEC-244). Test: `tests/routes::the_check_form_comes_back_to_its_page_without_a_script_and_swaps_a_fragment_with_one`.

## Connect

## CON-13. The snippets are the board's, with the real address
Five clients (franz-go, sarama, librdkafka and kcat, kafkajs, the Java client) with the bootstrap address substituted; the address is the listener's, or for a wildcard bind the advertised name. Test: `snippets::tests::the_address_is_in_every_snippet_and_the_clients_are_the_boards`, `snippets::tests::the_code_is_the_boards_to_the_character`, `snippets::tests::another_address_is_substituted_everywhere`, `facts::tests::a_specific_bind_is_the_bootstrap_and_a_wildcard_is_the_advertised_name`, `facts::tests::an_advertised_ipv6_literal_is_bracketed_once`, `tests/boards::connect_has_the_boards_cards_and_the_five_clients`.

## CON-14. The boards, rendered from the larder fixtures (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
Each of the five boards is rendered with the fixtures of `kit/world.md` (the "larder") to `target/board-diff/<id>.html` for `tools/board-diff.ts`, and checked against what the board says in order: headings, labels, buttons, numbers. `design/routes.json` is generated from the route table by a test. Test: `tests/boards::every_board_of_the_slice_renders_and_is_written_for_the_tool`, `tests/boards::home_has_the_parts_of_its_board_in_its_order`, `tests/boards::the_quiet_home_is_the_boards_first_run`, `tests/boards::the_advertised_page_has_the_check_the_bad_case_and_what_to_set`, `tests/boards::the_fixtures_carry_the_worlds_headline_numbers`, routes::tests::routes_json_is_what_the_table_says (removed in R5).

## The client layer

## CON-15. Every page is complete with JavaScript off (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
Fragments are plain GETs (`/fragments/{id}`), forms are plain forms, the copy buttons and the "Copy as" control are hidden until a script reveals them, and the address is always text that can be selected. Test: ui::client::tests::nothing_the_script_needs_is_shown_without_it (removed in R5), ui::client::tests::a_form_posts_with_no_script_and_swaps_its_target_with_one (removed in R5), `tests/routes::the_check_form_comes_back_to_its_page_without_a_script_and_swaps_a_fragment_with_one`.

## CON-16. One file holds every name the script reads (decision D4a) (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
The client layer is `assets/console.js`, our own, no library (the spike, `phase9/client-spike.md`). Every attribute, id, header and event name it reads is a constant of `ui/client.rs` and a test checks the script uses each. The rules the spike found are each a line of the script and a test that the line is there: the script sends `If-None-Match` itself and skips the swap on a 304, the newest request wins per fragment, the stream is closed on `pagehide` and in a hidden tab and reopened on `pageshow` and when visible, the reconnect waits longer and never gives up, a focused input keeps its value. The script uses no eval and no inline script (the CSP, SEC-245). Test: ui::client::tests::the_script_uses_every_name_this_file_defines (removed in R5), ui::client::tests::the_markup_uses_the_attribute_names_the_constants_say (removed in R5), ui::client::tests::the_script_follows_the_rules_the_spike_found_and_runs_under_the_strictest_policy (removed in R5). The script's behaviour is tested by running the shipped file in a DOM (`tools/dom`, happy-dom with a scripted fetch and EventSource; `cd tools/dom && bun install && bun test`, in CI): the ETag sent and kept per fragment and a 304 skipping the swap, an AbortController per fragment and a stale answer never overwriting a newer one, the stream closed on `pagehide` and in a hidden tab and reopened, a reconnect that never gives up, the banner shown by a failed fetch and hidden by the next answer, the focused input kept, "Copy as" applying to the client snippets only (the "When SASL is on" card copies its own lines), and a copy that works without `navigator.clipboard` (plain http on a LAN address). The Rust string tests above are no longer the guard. A mutation run over the script (15 mutants) is caught; the cases are in `tools/dom/console.test.ts`.

## CON-17. Fragments revalidate
A fragment answers `Cache-Control: no-cache` and an ETag over its text; a request with the same ETag is a 304 with no body. Pages are `no-store`; files are `public, max-age=31536000, immutable` under a content-hashed name. Test: `tests/routes::pages_are_not_stored_fragments_revalidate_and_files_are_kept_for_a_year`.

## The stream

## CON-18. `GET /events?tail=live|messages`: the stream of the two tails
The console's only stream: `tail=live` (the request tape, with the page's filters) or `tail=messages&topic=...`; anything else is a 400 before a stream opens. The first event, `ready`, arrives at once; later events are `rows` (the rows added or grown since the cursor, as HTML, newest first, the `id` being the next cursor). The server looks once a second at most (`pace=1|3|10`, never under 1 s and never over 30 s) and sends only when rows were added or folded; the browser's `Last-Event-ID` resumes a reconnect from the last row it saw, so no row comes twice. Test: `tests/stream::a_tail_opens_at_once_with_ready_and_sends_nothing_while_nothing_is_added`, `tests/stream::only_the_two_tails_can_be_followed`, `tests/live_tail::rows_arrive_as_html_after_the_cursor_once_and_a_reconnect_brings_only_the_newer`, `stream::tests::the_pace_is_what_was_asked_within_the_caps`.

## CON-19. No task, no timer and no wake-up for a page; a tail is looked at once a second
There is no ticker, no floor timer and no per-request bump of subscribers (CON-801): the recorder wakes nobody, and a tail's stream asks `Recorder::since` once a second while it is open. A closed tail costs nothing; a stream that ends gives its permit back (the cap is `max_streams`). Test: `tests/stream::a_tail_that_goes_away_gives_its_place_back`, `tests/stream::shutdown_while_ten_tails_are_open_completes_in_under_a_second`, `recorder::tests::a_tail_sees_only_the_rows_its_filters_pass`.

## CON-20. Shutdown ends every stream
Every stream selects on the server's cancellation token, so a graceful shutdown never waits on one. Test: `tests/stream::shutdown_while_ten_streams_are_open_completes_in_under_a_second` (with 10 open streams, under a second), `tests/process console::a_stopping_broker_does_not_wait_for_an_open_console_stream`.

## Assets and components

## CON-21. The assets are a manifest, under hashed names (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
assets.rs (removed in R5) lists every file of `assets/` with `include_bytes!`; a test fails on a file that is not listed and on an entry with no file. Served names carry a content hash; the stylesheet names the fonts' hashed names. `console.css` is the kit's `paper.css` and `kafka.css` unchanged plus `design/console.css`, joined; a test says so. Inter and JetBrains Mono (Latin subset, OFL) are embedded with their licences kept beside them; no third-party script ships. The total is about 140 KB. Test: assets::tests::every_file_of_assets_is_in_the_manifest_and_every_entry_has_its_file (removed in R5), assets::tests::a_licence_is_kept_for_every_third_party_file (removed in R5), assets::tests::the_stylesheet_is_the_kit_and_the_consoles_rules_joined (removed in R5), assets::tests::a_name_changes_with_its_content_and_only_then (removed in R5), assets::tests::every_served_file_is_found_by_its_hashed_name_and_the_stylesheet_names_its_fonts (removed in R5), assets::tests::the_shipped_files_weigh_what_the_stack_decision_says (removed in R5).

## CON-22. The components write what their golden files say (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
The shell, the buttons and states, the notices, the layout, and the table, form, filter and code components each have a golden file under `tests/golden/`; a change to the HTML is a diff in review. Variants are enums, so a misspelt class does not compile. Test: `tests/components::buttons_states_and_pills`, `tests/components::notices`, `tests/components::layout`, `tests/components::table_form_filter_and_code`, ui::button::tests::a_class_is_built_from_its_variant_and_size (removed in R5).

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

## CON-150. The writes are one list (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
actions.rs (removed in R5) lists the Kafka APIs the console writes with (CreateTopics, Produce, CreatePartitions, DeleteRecords, DeleteTopics) and a test compares it with the `data-op` names the committed boards draw, both ways. Test: actions::tests::actions_match_boards (removed in R5).

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

## CON-161. A dialog is an overlay of its parent page (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
Reversed by the owner on 2026-11-13 (see CON-400): a dialog is no longer a page of its own. A link marked `data-dialog` opens it in place over the page; without a script its address is the PARENT page with the dialog open on it; either way it is one POST form. Test: `tests/routes_write::a_dialog_is_a_full_page_to_a_browser_and_its_body_alone_to_a_script`, ui::client::tests::the_script_opens_dialogs_by_the_names_this_file_defines (removed in R5).

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
Name, partitions, cleanup policy, size on disk and last write (from `partition_files`), whether a transaction is open, retention where the topic sets its own, "N segments" where a topic has more segments than partitions, and the groups that have committed offsets on it ("read by"). Internal topics are hidden; `__consumer_offsets`, which Metadata never lists, is shown as a constant row when asked for, said to be no log. The filter matches names as typed. "Written by" is not drawn: it needs the recorder. Test: `tests/boards_topics::the_list_has_the_boards_nine_topics_and_the_hidden_internal_one`, `tests/live_topics::every_route_answers_as_the_console_and_has_the_boards_parts`, `gather::configs::tests::a_policy_is_delete_compact_or_both`.

## CON-109. `ConsoleView::partition_files`: size, last write and the offline cause, lazily
Per partition: bytes of its segments, how many segments, when the newest was last written, and why writes are refused after a failed flush. An open partition answers from its reader's snapshot; a closed one is read from its directory; nothing is opened or kept, so looking at a list does not change which partitions are open. The topic is checked for Describe with the console's caller, and an unknown topic and a refused one are not told apart. Test: `tests/view_partition_files::each_partition_reports_its_size_and_a_partition_never_written_reports_none`, `tests/view_partition_files::a_partition_that_was_closed_to_make_room_reports_the_same_from_its_directory`, `tests/view_partition_files::a_partition_whose_flush_failed_is_offline_with_the_cause_and_the_others_are_not`, `tests/view_partition_files::a_topic_the_caller_may_not_describe_or_that_does_not_exist_is_nothing`, `log::files::tests::a_log_of_several_segments_lists_each_with_its_base_and_size`, `log::files::tests::what_the_directory_says_is_what_the_open_log_says`, `log::files::tests::a_failed_flush_is_the_offline_cause_a_reader_can_read`, `log::files::tests::a_directory_that_is_not_there_has_no_segments_and_foreign_files_are_not_segments`.

## CON-110. An offline partition is named with its cause, not a time
A topic with a partition a failed flush took offline says which, what still works (reads), what is refused (KAFKA_STORAGE_ERROR, 56) and what restarting does. The broker keeps the cause and not the time of the failure, so the page does not say when. Test: `tests/boards_topics::the_partitions_and_the_offline_pages_are_their_boards`, `tests/security::hostile_names_and_values_are_inert_on_the_offline_empty_record_and_unknown_pages`.

## CON-111. A record is one page, complete
Its key, value, headers, time (and whose clock), codec, producer id, epoch and its own sequence number, the transaction it belongs to (from the coordinator while it is open) and where it stands for a read_committed consumer. A record that is not there (an offset outside the log, a partition that does not exist, a place compaction or a marker emptied) is a 404 page in the shell. Test: `tests/boards_topics::the_record_pages_show_the_text_and_the_bytes`, `tests/live_topics::a_compacted_topic_a_gzip_batch_headers_and_a_binary_value_are_shown_as_they_are`, `tests/live_topics::a_record_past_the_end_is_a_page_that_says_so_and_not_an_error`.

## CON-112. The list and the Messages page are regions a Refresh fetches again
The list's regions are `topics-stats` and `topics-table`, a topic's `topic-stats` and `topic-messages`; each is also a plain GET (`/fragments/{id}` with the page's query) that answers 304 to an unchanged ETag. No page opens a stream by itself (CON-800). Test: `tests/live_topics_more::a_fragment_is_what_the_page_holds_and_an_unchanged_one_is_a_304`, `tests/security/topics::the_stream_takes_the_two_tails_and_refuses_anything_else`.

## CON-113. Hostile keys, values, headers and names are text
**Untested since R5.** The tests named here drove the server-rendered pages and were removed with them (2026-11-13). The behaviour is the data's (`gather/`, `model/`, the backend, unchanged) and the API serves the same view-model, so it holds there; no test names it at the API yet.
A record's key, value and header names and values, a topic name (every legal character, 249 bytes), a transaction id, an offline cause and every value of the query are shown escaped on every page and fragment: no script, no attribute break-out, no bidirectional control, no NUL, and a very long line is cut. Test: `tests/security::hostile_keys_values_headers_and_names_are_inert_on_every_topic_page`, `tests/security::hostile_topic_names_in_the_list_come_out_inert`, `tests/live_topics_more::hostile_records_headers_and_names_from_a_real_broker_are_inert_on_every_page`, pages::topics::table::tests::a_hostile_key_and_value_are_text_in_their_cells_and_the_offset_is_the_link (removed in R5).

## CON-114. A refusal is Kafka's, as a client would see it
**Untested since R5.** The tests named here drove the server-rendered pages and were removed with them (2026-11-13). The behaviour is the data's (`gather/`, `model/`, the backend, unchanged) and the API serves the same view-model, so it holds there; no test names it at the API yet.
On an ACL-on broker a principal that may describe a topic but not read it sees the topic and then "The broker refused the read: TOPIC_AUTHORIZATION_FAILED (29)."; one that may not describe it sees no such topic; one with no right at all sees an empty broker. The console itself is a super user and reads everything; the refusal is shown by acting as another caller (`Backend::acting_as`, tests and examples only). Test: `tests/live_topics_more::on_an_acl_broker_a_principal_that_may_describe_but_not_read_is_refused_in_kafkas_words`, words::topics::tests::a_timeout_is_in_seconds_up_to_two_minutes_as_the_board_writes_it (removed in R5).

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
Lag is the log end (ListOffsets) minus the committed offset (OffsetFetch). A partition never committed (OffsetFetch answers -1) is `NoCommit`, never lag `end`; a commit beyond the log end (a topic recreated under an app that kept its position) is `PastEnd(by)`; a partition with no log end is `Gone`. A negative lag is never produced and never shown. Tests: `lag::tests::the_table_of_cases`, `lag::tests::lag_is_never_negative_and_a_behind_group_is_exactly_the_difference`, lag::tests::a_bar_is_zero_when_not_behind_two_at_least_when_behind_and_never_over_a_hundred (removed in R5).

## CON-201. A commit past the end of a recreated topic is said, not subtracted
**Untested since R5.** The tests named here drove the server-rendered pages and were removed with them (2026-11-13). The behaviour is the data's (`gather/`, `model/`, the backend, unchanged) and the API serves the same view-model, so it holds there; no test names it at the API yet.
Over a real broker: commits made after a topic was deleted and made again, shorter, show "3 past the end" on the Lag page and on the list ("orders-0 is 3 beyond its log"), with the callouts that say what Kafka accepted and what a consumer would do. Test: live_groups::a_commit_beyond_the_end_of_a_recreated_topic_is_past_the_end_never_negative_lag (removed in R5).

## CON-202. A group with no commit shows "No commits yet", not the log end
**Untested since R5.** The tests named here drove the server-rendered pages and were removed with them (2026-11-13). The behaviour is the data's (`gather/`, `model/`, the backend, unchanged) and the API serves the same view-model, so it holds there; no test names it at the API yet.
A group whose members are subscribed to a topic but have committed nothing shows its partitions as "No commits yet" and the tab says "lag not known". Test: live_groups::a_group_with_no_commit_shows_no_commits_and_not_the_log_end_as_lag (removed in R5).

## CON-203. The expiry date is the moment the sweep takes the commit
`groups::expiry::first_expiry` applies the rules of `groups::expired` (empty since, own age, the presumed members after a restart, subscribed topics kept, a group in a rebalance not looked at) and is held to `expired` itself at the moment before, at and after each date over a grid of groups. The date on the Lag page is that date, and a sweep at that moment takes the commit. Tests: `groups::expiry::tests::the_date_is_the_one_the_sweep_acts_on_over_a_grid_of_groups`, live_groups::the_expiry_date_shown_is_the_moment_the_sweep_takes_the_commit (removed in R5).

## CON-204. What the protocol hides comes from `ConsoleView::group_detail`
**Untested since R5.** The tests named here drove the server-rendered pages and were removed with them (2026-11-13). The behaviour is the data's (`gather/`, `model/`, the backend, unchanged) and the API serves the same view-model, so it holds there; no test names it at the API yet.
The classic leader, the generation, the session and rebalance timeouts of each member, how long ago each was heard, which member a rebalance waits for and how long is left, a consumer group's epochs, the date a group became empty and the retention: read in the group's own task (one consistent look), Describe-checked on the group, and the offsets filtered by Describe on their topic. No transition changed: a member's `heard` and a rebalance's `began` are written where the timers were already armed. Tests: live_groups::a_classic_group_with_a_member_and_commits_shows_its_members_lag_and_what_the_protocol_hides (removed in R5), live_groups::a_rebalance_in_progress_says_who_it_waits_for (removed in R5).

## CON-205. A KIP-848 group is described by its own API
**Untested since R5.** The tests named here drove the server-rendered pages and were removed with them (2026-11-13). The behaviour is the data's (`gather/`, `model/`, the backend, unchanged) and the API serves the same view-model, so it holds there; no test names it at the API yet.
A consumer group of the new protocol is Dead to DescribeGroups; the page asks ConsumerGroupDescribe and shows epoch, target epoch, assignor, and each member's target and assigned partitions. Test: live_groups::a_consumer_group_of_the_new_protocol_is_described_by_its_own_api (removed in R5).

## CON-206. The sentences say only what the group machine says now
A group's facts are metric tiles (state, members, generation or epoch, lag, committed partitions, last commit), never a sentence with a number in it (CON-805); the attention line names who is waited for and shows the rejoin deadline in its own slot (CON-812). Test: `tests/boards_groups::the_group_pages_say_what_the_boards_say`, `tests/live_groups::a_rebalance_in_progress_says_who_it_waits_for`.

## CON-207. A reset is a commit with no generation, and the broker's refusal is shown in its words
Reset offsets sends OffsetCommit with generation -1 and no member (what `kafka-consumer-groups --reset-offsets` sends); the preview is the same computation as the write (earliest, latest, an offset, a time, a shift, current; each kept within the log). On a group with a member Kafka answers UNKNOWN_MEMBER_ID (25) and the page shows that code, its words ("The coordinator is not aware of this member."), the console's reading of the rule and what would pass. Tests: live_groups::a_reset_of_an_empty_group_moves_its_offsets_and_a_group_with_a_member_is_refused_in_kafkas_words (removed in R5), `groups_words::tests::the_reading_of_a_refused_reset_counts_the_members_and_a_delete_names_the_rule`, `backend::requests::groups_read::tests::a_reset_is_a_commit_that_names_no_generation_and_no_member`.

## CON-208. Delete group is refused with NON_EMPTY_GROUP and done when empty
**Untested since R5.** The tests named here drove the server-rendered pages and were removed with them (2026-11-13). The behaviour is the data's (`gather/`, `model/`, the backend, unchanged) and the API serves the same view-model, so it holds there; no test names it at the API yet.
DeleteGroups on a group with a member shows NON_EMPTY_GROUP (68) and "The group is not empty."; on an empty group it deletes and returns to the list. Test: live_groups::deleting_a_group_is_refused_with_members_and_done_when_empty (removed in R5).

## CON-209. Delete offsets forgets them, and a subscribed topic is refused
**Untested since R5.** The tests named here drove the server-rendered pages and were removed with them (2026-11-13). The behaviour is the data's (`gather/`, `model/`, the backend, unchanged) and the API serves the same view-model, so it holds there; no test names it at the API yet.
OffsetDelete on an empty group forgets the commits and lands on the Lag page; on a topic a member is subscribed to Kafka answers GROUP_SUBSCRIBED_TO_TOPIC (86) and the dialog shows it. Test: live_groups::deleting_offsets_forgets_them_and_a_subscribed_topic_is_refused (removed in R5).

## CON-210. An ACL-on broker refuses as a client would be refused
**Untested since R5.** The tests named here drove the server-rendered pages and were removed with them (2026-11-13). The behaviour is the data's (`gather/`, `model/`, the backend, unchanged) and the API serves the same view-model, so it holds there; no test names it at the API yet.
With ACLs on, a caller that may only DESCRIBE a group is refused a reset and a delete with GROUP_AUTHORIZATION_FAILED (30), and a group it may not describe is not told apart from no group. Test: live_groups::on_a_broker_with_acls_a_caller_without_the_right_is_refused_as_a_client_would_be (removed in R5).

## CON-211. A group id is hostile input
**Untested since R5.** The tests named here drove the server-rendered pages and were removed with them (2026-11-13). The behaviour is the data's (`gather/`, `model/`, the backend, unchanged) and the API serves the same view-model, so it holds there; no test names it at the API yet.
A group id with `/`, `?`, `%`, spaces, unicode, a `<script>`, an attribute break, or 255 bytes is escaped in every page and percent-encoded in every link. Test: live_groups::a_hostile_group_id_is_escaped_and_encoded_on_every_page (removed in R5).

## CON-212. The group requests are bounded and name what they do
DescribeGroups and ConsumerGroupDescribe name at most 100 groups; an OffsetFetch asks for every partition of one group and waits for a stable answer; a commit or a delete names at most 4,096 partitions. Tests: `backend::requests::groups_read::tests::a_describe_names_at_most_a_hundred_groups`, backend::requests::groups_read::tests::an_offset_fetch_asks_for_every_partition_of_one_group_and_waits_for_a_stable_answer (removed in R5), `backend::requests::groups_read::tests::the_deletes_name_what_they_delete`.

## CON-213. The assignment bytes are read without trusting their counts
A member's assignment and subscription (DescribeGroups hands the bytes over) are decoded with every count checked against the bytes left. Test: `gather::protocol::tests::bytes_that_are_not_an_assignment_or_a_subscription_are_not_read`.

## CON-214. The sixteen boards pass the structure gate in both renders (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
The fixture render (the larder: analytics, fulfilment, payments-workers, search-indexer, run through the same assembly as a live group) and the live render over a real broker both pass `bun tools/board-diff.ts --require` the sixteen ids, with every allowed difference carrying a reason and an `expires` (the wire dock: 9e; the Story page: 9f; the rest: close). Tests: boards_groups::every_board_of_the_slice_renders_and_is_written_for_the_tool (removed in R5), boards_groups::the_list_has_the_boards_four_groups_in_order_with_their_lag_words (removed in R5), boards_groups::the_group_pages_say_what_the_boards_say (removed in R5), boards_groups::the_consumer_group_shows_target_and_assigned_and_the_rebalance_shows_who_is_waited_for (removed in R5), boards_groups::the_lag_pages_never_show_a_negative_number_and_say_where_a_commit_is (removed in R5), boards_groups::the_refusals_carry_kafkas_code_and_words (removed in R5).

## CON-215. The regions of the Groups pages are listed once (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
`groups-live`, `groups-table`, `group-stats` and `group-lag-table` are listed in regions.rs (removed in R5); the page lists them, the shell draws the Refresh control from the list. Test: regions::tests::a_page_lists_each_region_once (removed in R5), `tests/live_groups::a_rebalance_in_progress_says_who_it_waits_for`.

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
## Overlays, motion and the small things (slice p9o; the rules from CON-400 up)

The owner's words (2026-11-13): "I see too many pages. Aren't we supposed to have a sidebar instead of a whole page?" and "the transitions between pages need to be nice and gentle". The design is `docs/port/console/overlays.md`.

## CON-400. An overlay is its parent page with the overlay open on it
A board of kind `drawer` or `dialog` has an address of its own; a direct visit answers the parent page's whole document (its landmarks, its one `h1`, its breadcrumb and stream) with the overlay as the last element of the body, `role=dialog`, `aria-modal`, `aria-labelledby` bound to a heading that is there, the parent `inert`; the scrim and the Close control are plain links to the parent's address, under any prefix. Test: `tests/overlays::a_direct_visit_is_the_parent_page_with_the_overlay_open_on_it`.

## CON-401. A script is sent the overlay alone, marked as one
`X-Fragment` gets the overlay element and no document, with `x-overlay: 1`; the script tells an overlay from a page by that header. Test: `tests/overlays::a_script_is_sent_the_overlay_alone_marked_as_one`.

## CON-402. A record is a drawer over the Messages page it was opened from
The drawer's address carries the page's query (its filters) and closing returns to that page; the offsets of the table and the buttons that open dialogs are openers. Test: `tests/overlays::the_records_parent_keeps_its_query_and_the_drawer_closes_back_to_it`.

## CON-403. An overlay of a name that is not a topic is inert
Hostile topic names in any overlay address are text in the page that says there is no such topic; no overlay is drawn over a page that is not there. Test: `tests/overlays::the_overlays_of_a_hostile_topic_name_are_text_in_the_page_that_says_there_is_none`, `tests/overlays::an_overlay_with_no_parent_topic_is_the_page_that_says_so`.

## CON-404. The script opens, closes and keeps overlays as the contract says
Open in place with no navigation, push the address, Esc, the scrim, Close and Back close it and restore the address, the scroll and the focus; Tab stays inside, the page behind is `inert`; a second opener replaces the content; a server-rendered overlay is adopted; a form's answer replaces the overlay (a refusal) or the page (a result); with reduced motion nothing animates. Test: `tools/dom/overlay.test.ts` (run: `cd tools/dom && bun test`).

## CON-405. The motion is gentle and only for people who have not asked for less
Scrim 160 ms, drawer 220 ms with ease-out, dialog 180 ms, closing 140 ms, the page 180 ms with a 4 px rise; opacity and transform only; the top bar and the rail keep their own view-transition names; all of it inside `prefers-reduced-motion: no-preference`. A fragment that changed fades 120 ms where it changed and an unchanged one does not move. Test: `tests/overlays::the_motion_is_gentle_and_only_for_people_who_have_not_asked_for_less`, `tools/dom/overlay.test.ts`.

## CON-406. Jump to a topic or a group
`/jump` is the palette to a script (the overlay alone, with no history entry) and a page of links with a plain GET filter to a browser; ⌘K and Ctrl+K open it, typing narrows, the arrows and Enter go, Esc closes. It lists the topics, then the groups (ListGroups through the Backend), each with a visible `topic` or `group` tag; at most 500 names; every name is text and a link made by `segment` (CON-410). Test: `tests/overlays::the_jump_palette_is_an_overlay_alone_to_a_script_and_a_page_of_links_to_a_browser`, `tools/dom/polish.test.ts`.

## CON-407. j and k, and a copy that says it copied
`j` and `k` move through the rows of the page's table (and through the records of an open drawer), never in a field; a copy button says "Copied" for a moment in the width it had. Test: `tools/dom/polish.test.ts`.

## CON-408. The Groups and Config dialogs and the member drawer are overlays
The member drawer (`member-detail`) is a drawer over the Members page; Reset offsets and Delete offsets are dialogs over the group's Lag page (Delete offsets over the Lag page with the expiring offsets), Delete group over the group's page; the four edit and reset dialogs of Config are dialogs over the topic's Config page. A direct visit is the parent page (its heading, its landmarks) with the overlay open, closing is a link to the parent's address, a script is sent the overlay alone, and the buttons and links that open them are openers (`data-dialog`). Test: `tests/overlays::a_direct_visit_is_the_parent_page_with_the_overlay_open_on_it`, `tests/overlays::a_script_is_sent_the_overlay_alone_marked_as_one`, `tests/overlays_more::the_buttons_that_open_a_group_dialog_or_a_member_are_openers`.

## CON-409. A refusal answers in the dialog, a result is a page
Kafka's refusal of Delete offsets (GROUP_SUBSCRIBED_TO_TOPIC) and an invalid Config or Reset field answer the dialog again (an overlay to a script, the parent with the dialog open to a browser), in Kafka's words; a write that worked is a redirect to the page that shows it, and a refusal the boards draw as a page (Reset refused, Delete refused, Refused, Saved) is that page. Test: `tests/overlays_more::a_refusal_in_a_dialog_answers_the_dialog_over_its_parent_and_a_result_is_a_page`.

## CON-410. Overlays and palette entries of hostile names are inert (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
A hostile group id, member id or topic name in an overlay address is text in the page that says there is none, with no overlay over it; in the palette every name is escaped in the link, the `title` and the text. Test: `tests/overlays_more::the_overlays_of_a_hostile_group_or_member_name_are_text_in_the_page_that_says_there_is_none`, pages::jump::tests::a_hostile_name_is_text_in_a_link_made_by_segment_and_the_tag_says_which_it_is (removed in R5), `tests/live_groups_more::the_jump_palette_lists_the_groups_a_real_broker_lists_and_a_hostile_group_id_is_text`.

## CON-411. A control that asks for a GET in an overlay never writes
The Reset dialog's Preview (`formmethod="get"`) is fetched as a GET with the fields in the address and the overlay that comes back replaces the dialog and takes that address; the dialog's own POST is the only thing that writes. Test: `tools/dom/overlay.test.ts` (a Preview control in an overlay never writes).

## CON-412. A table's header sticks under the top bar
Every `table.t` header is `position: sticky` at 52 px (the top bar's height) and the table is not its own scroll container (the cells draw the corners, the borders are separate); inside an overlay the header does not stick. Test: `tests/tables::a_table_header_sticks_under_the_top_bar_and_the_table_is_not_its_scroll_container`.

## CON-413. A long key or name gives way in the middle (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
A key in the Messages table, a topic name in the list and a member id keep their last ten characters and lose their start to an ellipsis when the cell is narrow; the whole text is the hover and is in the page for a copy; short texts are untouched; a hostile text is escaped. Test: ui::table::tests::a_short_text_is_left_alone_and_a_long_one_is_split_before_its_last_ten_characters (removed in R5), ui::table::tests::the_two_halves_are_the_whole_text_and_never_cut_a_character (removed in R5), ui::table::tests::a_hostile_text_is_escaped_in_the_title_and_in_both_halves (removed in R5), `tests/tables::a_long_key_gives_way_in_the_middle_and_keeps_its_end`.

# Slice 9e: the recorder, Live, the drawers, the docks (CON-300 to CON-319)

Branch `p9e`. The recorder is `kafka_server::Observer` (`crates/kafka-server/src/observer.rs`) implemented by `kafka_console::Recorder` (`crates/kafka-console/src/recorder/`); the pages are `pages/live/`, their models `model/live.rs`, their fixtures `fixtures_live.rs`.

## CON-300. The observer is optional, and with none installed a request costs one `Option` test
`Config::observer` is `None` by default and `--console=false` leaves it so. A connection takes a number and builds its `Watch` only when an observer exists; the request header is cloned for the observer only then. The Produce benchmark (`crates/kafka-console/examples/produce_bench.rs`, interleaved with a build of main before the change) shows the same throughput within the noise of two runs of one binary (about 3%). Test: `connections::tests::numbers_start_at_one_and_none_are_taken_without_an_observer`, `connection::tests::observed::each_request_is_told_twice_in_order_between_the_open_and_the_close`.

## CON-301. Every request is told twice, in order, whatever becomes of it
`begin` runs before the handler with the request; `end` after it with the answer (encoded, so its size is known), before the answer is written, so a slow client does not lengthen what the broker is said to have taken. A request refused on its header (a stranger before login), a body that does not decode, a request that gets no answer (`acks=0`), and a handler cut short by a stop are each told as `Closed` or `Nothing`, with the server's words. The connection is told when it opens and when it closes, with the server's sentence for why. Test: `connection::tests::observed::a_request_the_connection_is_closed_for_is_told_as_closed_and_a_login_changes_the_principal`, `connection::tests::observed::a_request_with_no_answer_is_told_as_nothing`.

## CON-302. An observer that panics ends its own connection and no other
Every connection is a task of its own, so in a test build a panic ends only that connection. A release build is `panic = "abort"`: observers must not panic, which the workspace lints (no `unwrap`, `expect`, indexing or `panic`) enforce for the recorder. Documented in the module. Test: `connection::tests::observed::an_observer_that_panics_ends_its_own_connection_and_no_other`.

## CON-303. `Summarize` is a typed match with no wildcard, and every API has a golden file
`recorder/summary.rs` matches `Request` and `Response` exhaustively: an API added to the served table without a summary does not compile. Each of the 43 served APIs has a golden file of what is kept of a representative request and answer (`recorder/tests/golden/<Api>.txt`; `BLESS=1` rewrites them). Summaries are bounded: 20 topics, 8 partitions each, 16 results, names 128 bytes, text 256; batch headers are walked (at most 64), records never. Test: `recorder::summary::tests::every_served_api_has_a_golden_file`, `recorder::summary::tests::each_summary_is_what_its_golden_file_says`, `recorder::facts::tests::topics_are_bounded_clipped_and_counted_past_the_bound`, `recorder::facts::tests::batch_headers_are_walked_not_records`.

## CON-304. No secret enters the recorder
Never kept: record keys, values and headers, SASL tokens and the server's side of the exchange, SCRAM salts and salted passwords, config values (names only), ACL contents beyond counts. A planted-secret test runs a real login (and a failed one), a produce with a planted key, value and header, a secret config change and a SCRAM change against a broker with SASL on, then searches the recorder's `Debug`, every row and detail, every Live page, drawer, dock, the clients and the first request; a mutated summary that keeps a token fails it. Test: `recorder::summary::tests::no_planted_secret_or_record_byte_reaches_a_summary`, `tests/redaction::no_planted_secret_reaches_the_recorder_the_stream_or_a_page`, `recorder::tests::the_debug_of_a_recorder_prints_counts_and_nothing_a_client_sent`.

## CON-305. The error table has a row for every code the broker serves
Name, code, Kafka's own words, a plain sentence, a class (`Ok`, `Warn`, `Bad`, `Refused`, `Denied`) and what would pass. A test walks the broker, server, wire and auth sources for every `ResponseError` they name and fails on a code with no row; another proves the walk can fail; another checks each row's name and code against the protocol crate's. Test: `recorder::errors::tests::every_error_the_broker_names_has_a_row`, `recorder::errors::tests::the_scan_would_find_a_code_with_no_row`, `recorder::errors::tests::a_rows_name_and_code_are_the_protocol_crates_own`, `recorder::errors::tests::codes_are_unique_and_the_classes_are_what_the_boards_say`.

## CON-306. The ring is bounded by rows and by bytes
2,000 rows and 1 MiB by an estimate of what each row holds (struct, text, vectors); strings are clipped, so a 900 KiB `reason` or client id (the Go edition's TRF-1) is kept at 256 or 128 bytes and cannot exceed the budget. The fold index forgets a row that is dropped. Test: `recorder::ring::tests::the_ring_never_holds_more_than_its_rows`, `recorder::ring::tests::the_ring_never_holds_more_than_its_byte_budget`, `recorder::ring::tests::the_byte_count_is_what_the_rows_weigh`, `recorder::ring::tests::the_fold_index_forgets_a_row_that_was_dropped`, `recorder::tests::a_900_kib_reason_is_clipped_and_cannot_exceed_the_byte_budget`.

## CON-307. An idle recorder holds nothing
No ring is allocated until a request is recorded; when the last row goes the memory is given back; the connection table empties as connections close and is capped at 4,096. Test: `recorder::ring::tests::an_idle_ring_holds_nothing`, `recorder::tests::the_state_is_quiet_until_a_request_arrives`, `recorder::clients::tests::a_connection_is_a_row_until_it_closes_and_is_not_kept_past_the_cap`.

## CON-308. Repeats fold: heartbeats and idle polls for ten minutes, a stream of requests for two seconds, an error never
The key is connection, API, version, target, result and the transactional flag. A refusal or a failure stays on its own row. A folded row keeps its first and last time, a count, the totals of records and bytes, and the last 64 took samples (the median is of those, and says so). Test: `recorder::ring::tests::repeats_within_the_window_fold_into_one_row_and_a_later_one_does_not`, `recorder::ring::tests::a_request_that_is_not_a_poll_folds_only_in_a_stream`, `recorder::ring::tests::rows_that_never_fold_and_rows_with_other_keys_stand_alone`, `recorder::ring::tests::the_took_samples_are_the_last_sixty_four`, `recorder::tests::heartbeats_fold_into_one_row_that_idle_hiding_hides_and_refusals_stand_alone`.

## CON-309. The window is computed from the oldest row, never promised
The Live strip says when the oldest row kept was written (an absolute time, `Reaches back to 19:12 UTC`), computed from the ring and never promised; the quiet wire says it has nothing yet. Test: `tests/boards_live::live_wire_has_the_boards_sentence_filters_refusals_and_sixteen_rows`, `tests/live_wire::a_console_without_a_recorder_says_the_wire_is_empty_and_the_broker_without_an_observer_is_unchanged`.

## CON-310. A cursor says what changed; nothing is woken
`Recorder::since(filters, cursor)` returns the rows that passed the filters and changed after the cursor (a fold counts as a change: the row comes again with its count grown), newest first, at most a hundred, with the cursor to ask from next time; the page's `cursor` is the same version. Test: `recorder::tests::since_returns_what_changed_after_a_cursor_including_a_fold`, `recorder::tests::a_tail_sees_only_the_rows_its_filters_pass`.

## CON-311. A Fetch names its topic, though Kafka 4 names it by id
Fetch v13 sends a topic id. The recorder learns id to name from the Metadata and CreateTopics answers that pass, and shows the name; an id it has not learned stays an id. Test: `recorder::tests::a_fetch_names_its_topic_by_id_and_the_recorder_learns_the_name_from_metadata`.

## CON-312. Denied is amber and says Denied; Kafka's refusals are violet
An ACL or a login that says no is the class `Denied`: the word "Denied", the `warn` colour, a drawer whose rule is "An ACL allows this". Kafka's own refusals stay `Refused`. Test: `tests/live_wire::an_acl_refusal_is_amber_denied_and_not_violet`.

## CON-313. A refused request opens the drawer its error calls for, with Kafka's words
Answered requests open the detail drawer; `INVALID_PRODUCER_EPOCH` and `PRODUCER_FENCED` the fenced drawer (the requests of the same transactional id that came before, then the two epochs); `MESSAGE_TOO_LARGE` the size drawer (the limit is read from the broker when the drawer opens, and said to be as of now); every other refusal the generic drawer with the broker's own message, the code's rule and what would pass. A request that has fallen off the tape is a 404 that says so. Test: `tests/live_wire::each_refusal_opens_its_own_drawer_with_kafkas_words_and_the_rule`, `tests/boards_live::the_drawers_have_their_boards_parts`.

## CON-314. What a client sent is text on every page
A client id of `<script>alert(1)</script>` and software of `<img src=x onerror=alert(2)>` are escaped on the tape, in the clients drawer and in every dock; commands put in a drawer are shell-quoted. Test: `tests/live_wire::the_tape_shows_what_the_clients_did_and_what_was_refused`, `pages::live::commands::tests::a_plain_name_is_left_alone_and_a_hostile_one_is_quoted`.

## CON-315. The docks, the first request and Home's sentence come from the same tape
A topic's dock, a group's wire (`/groups/{group}/wire`, built for slice 9c to link to) and the first-request page filter or read the one recorder; Home says how many apps appear on the wire and how long ago the last request landed, and says nothing of apps when this console has no recorder. The console's own requests are not on the tape, and the docks say so. Test: `tests/live_wire::the_docks_the_first_request_and_home_come_from_the_same_tape`.

## CON-316. The filters are in the address
`?q=&client=&api=&topic=&group=&kind=&refused=1&idle=0&before=&until=&limit=`: a filter survives a reload and can be shared; a value that cannot be used is no filter; the limit is held to 1 to 100. A click on a client, API or topic cell filters by it. Test: `model::live::tests::an_address_is_the_filters_and_back_again`, `model::live::tests::nonsense_is_no_filter_and_the_limit_is_held_to_its_range`, `model::live::tests::a_click_on_a_cell_filters_by_it_and_leaves_the_page_back`, `recorder::tests::filters_by_client_api_topic_group_and_refusal`.

## CON-317. Pause holds the tape still, counts what arrives, and a drawer holds it too
With a script Pause keeps new answers aside and counts the requests recorded since; Resume (or closing a drawer) brings the newest in; new rows are marked for a moment; `j` and `k` move between rows and a select filter applies on change. Without a script Pause is a link to the page as it was (`?until=`). Tests: `tools/dom/live.test.ts` ("a click on Pause holds the tape in place...", "a drawer that is open holds the tape without a banner..."), `recorder::tests::the_page_back_and_the_paused_page_are_cut_by_row_id`.

## CON-318. Without a recorder the broker behaves as it did, and the page says the wire is empty
Test: `tests/live_wire::a_console_without_a_recorder_says_the_wire_is_empty_and_the_broker_without_an_observer_is_unchanged`; the broker's and server's own tests run unchanged.

## CON-319. The twelve boards pass the structure gate in both renders
Fixture render (`tests/boards_live::every_board_of_the_slice_renders_and_is_written_for_the_tool`) and live render over a real broker with real client traffic (`tests/live_wire::the_live_render_of_the_boards_over_a_real_broker`), every difference in `design/allow.json` with a reason (those that make the page better than the board begin `improvement:`).

### What these boards show that the recorder cannot supply, and what the page does instead
Tagged in `docs/port/debt.md` J151 to J170: "waited 480 ms" (the recorder knows how long the client was willing to wait); "Record 0 has a null key" (no record is kept: the broker's own message and the topic's config now); the producer's story (only requests still on the tape, not why a transaction timed out: that is the group-ring slice's); a heartbeat that is idle is hidden by default.

## CON-414. A Live drawer is an overlay over the Live page it was opened from
`/live/{id}` (Detail, Refused, Fenced, Too large by what became of the request) and `/live/clients` are drawers over the Live page with the filters in the address; a direct visit is that page with the drawer open, Close and the scrim are plain links to the same filtered page, a script is sent the drawer alone, the tape's time links and the refused links are openers that carry the filters, a request not on the tape is a 404 page with no drawer. Test: `tests/overlays_more::a_live_drawer_closes_to_the_live_page_with_its_filters_and_the_tape_links_open_it_in_place`, `tests/overlays::a_direct_visit_is_the_parent_page_with_the_overlay_open_on_it`, `tests/overlays_more::a_request_that_is_not_on_the_tape_and_a_hostile_id_are_the_page_that_says_so_with_no_drawer`.

## CON-415. An open drawer holds the tape, and closing it lets the tape catch up
While a Live drawer is open (adopted from the server or opened in place) what the stream brings is kept aside with no banner; closing it by Esc, the Close control or Back brings the newest in. Test: `tools/dom/live.test.ts` (a Live drawer opened by the overlay system holds the tape).

## Slice 9g: the Log page and Producers (rules numbered from CON-500)

Boards `topic-log`, `log-planner`, `log-retention`, `log-empty`, `topic-producers`, `txn-open`, `txn-holding`. Routes: `/topics/{topic}/log` (the four Log boards: the data chooses), `/topics/{topic}/producers`, `/topics/{topic}/transactions/{txn}` (`txn-open`, a route of its own because it is a page about one transactional id, not about a partition) and `/topics/{topic}/transactions/{txn}/held` (`txn-holding`, reached by the button "Who is held"). All are read-only pages: a GET changes nothing and no page has a form.

## CON-500. The survey of a log says what each segment holds, and only what it read
`LogReader::survey` reads batch headers (and, for a batch compaction thinned or one that carries a delete horizon, the batch, within a budget) and says per segment: base, next, bytes, records, newest timestamp, the offsets removed (a batch the cleaner dropped and a record it took out of a batch both leave a gap), and the markers. A log bigger than the budget says it was read in part and counts what it could not place; it never guesses. Test: `log::tests::survey::the_gaps_of_a_compacted_log_are_exactly_the_offsets_a_round_removed`, `a_thinned_batch_places_the_offsets_it_lost_inside_it`, `a_budget_that_is_spent_says_the_walk_is_partial_and_never_guesses`, `an_empty_log_has_one_empty_active_segment_and_no_gaps`, `log::walk::tests::gaps_that_touch_are_one_range_and_gaps_apart_are_two`, `a_gap_past_the_budget_is_counted_and_says_the_walk_is_partial`.

## CON-501. The planner's verdict on the page is what the cleaner then does
The page calls `compact::judge` itself (re-exported read-only) on the facts the survey gives it, which are the facts `PartitionLog::clean_job` reads; `reading` says the numbers behind the verdict whether or not the log is due. A test runs the verdict and then a real round: due means segments were rewritten and the checkpoint moved, not due means nothing changed (a log never cleaned, one with a small dirty share, one held back by a minimum lag). Test: `the_verdict_is_what_the_cleaner_then_does` (`kafka-broker/tests/view_log_facts.rs`), `log::tests::survey::what_the_survey_gives_the_planner_is_what_the_cleaner_gives_it`, `compact::plan::tests::the_reading_says_what_stops_cleaning_and_agrees_with_the_verdict_whatever_the_log`, `a_transaction_open_exactly_at_the_active_segment_is_the_active_segment_that_stops_cleaning`.

## CON-502. The ruler is drawn from the real compacted partition
What is in a gap is exactly what a consumer no longer gets from the topic. Test: `the_gaps_are_exactly_the_offsets_the_cleaner_removed` (`kafka-broker/tests/view_log_facts.rs`); `log_ruler::tests::the_gaps_are_where_the_segment_lost_offsets_and_only_in_cleaned_segments`.

## CON-503. A deletion a round kept is listed with the time the next round removes it
The delete horizon of its batch, the record's own timestamp as when it was written. Test: `a_kept_deletion_is_listed_with_its_horizon`, `log::tests::survey::a_deletion_a_round_kept_is_listed_with_the_time_it_goes`.

## CON-504. Retention says what the next pass would do (by the broker's own rule) and what the last one did
`kafka_storage::count_to_drop` is the rule, on the sizes and ages the survey read; the last pass, and how many partitions' starts retention has moved since the broker began, are small notes kept in memory by `logs.rs` and `cleaner.rs` (bounded: one per cleaned partition, at most 512; one pass; nothing until a round or a pass has happened; forgotten when the broker stops). Test: `retention_says_what_the_next_pass_does_and_what_the_last_one_did`, `log_notes::tests::a_broker_that_has_done_nothing_keeps_nothing`, `the_partitions_remembered_are_bounded_and_the_oldest_goes_first`.

## CON-505. Looking is lazy and leaves the broker as it was
A partition never written is not opened and says so; a written one is opened as a Fetch would open it and let go again if nothing else holds it. Test: `an_empty_partition_says_so_and_looking_opens_nothing`, `a_closed_partition_is_looked_at_and_closed_again`.

## CON-506. The view obeys the protocol's rights and does not say which partitions exist
`log_facts` needs DESCRIBE on the topic, `producers` needs READ (as DescribeProducers does); a partition that is not there and a topic the caller may not see answer alike. Test: `the_view_obeys_describe_and_does_not_say_which_partitions_exist`.

## CON-510. A producer's window is its last five batches, newest first
With their sequences, where they went and when. Test: `each_producer_has_its_last_five_batches_newest_first` (`kafka-broker/tests/view_producers.rs`).

## CON-511. An open transaction holds the last stable offset where it began; an abort is remembered with its marker
Test: `an_open_transaction_holds_the_last_stable_offset_and_an_abort_is_remembered`.

## CON-512. Looking at producers writes nothing and keeps nothing
Test: `looking_writes_nothing_and_keeps_nothing`.

## CON-520. The seven boards render from the larder, whole, and have the parts of their boards in order (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
Test: every_board_of_the_slice_renders_and_is_written_for_the_tool (removed in R5), the_due_log_has_the_parts_of_its_board_in_order (removed in R5), the_other_three_states_have_the_parts_of_their_boards_in_order (removed in R5), the_producers_and_transaction_pages_have_the_parts_of_their_boards_in_order (removed in R5) (tests/boards_log.rs (removed in R5)).

## CON-521. Every sentence of the Log page is a pure function with this partition's numbers
Due, not due, empty, cleaner off, running, failed, forced by age, held by a transaction or a minimum lag; the rules, the round that will be run, what would make a log due; retention's sentence, expiry and rules. Test: `each_rule_of_the_planner_carries_this_partitions_numbers`, `an_open_transaction_holds_cleaning_back_and_a_minimum_lag_makes_a_segment_too_new`, `the_next_round_names_the_segments_it_rewrites_and_where_the_checkpoint_goes`, `what_would_make_a_log_due_is_the_threshold_in_bytes`, `a_size_limit_says_what_the_partition_holds`, `the_rules_of_retention_say_when_the_passes_are`, `a_share_exactly_at_the_threshold_is_not_over_it`.

## CON-522. The ruler's width is offsets and its gaps are where records were removed
Test: `log_ruler::tests::a_cell_is_as_wide_as_the_offsets_it_spans_and_the_colour_says_what_the_cleaner_did`, `a_log_start_moved_up_hatches_what_is_below_it`, `the_markers_sit_where_the_offsets_are_and_the_end_is_drawn_at_the_right_edge`, a_marker_allows_for_the_gaps_between_the_cells (removed in R5), `an_empty_log_is_one_active_cell_with_its_own_words`.

## CON-523. The four boards are the four states of one page, and none has a form (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
Test: pages::log::tests::the_four_boards_are_the_four_states_of_one_page_chosen_by_the_data (removed in R5), no_page_of_the_log_has_a_form_a_write_or_a_compact_now_button (removed in R5), an_empty_log_has_no_segments_table_no_cards_and_no_other_logs (removed in R5), the_live_parts_are_fragments_the_stream_can_name (removed in R5), the_segments_table_opens_the_segment_that_holds_the_checkpoint (removed in R5).

## CON-524. The Producers and transaction sentences are pure functions
Test: `a_transaction_is_described_with_its_clock_its_timeout_and_what_it_holds`, `what_it_holds_back_counts_records_per_partition`, if_it_is_not_ended_the_sentence_names_the_time_the_markers_and_the_new_epoch (removed in R5), `the_page_that_says_who_is_held_counts_the_distance_in_words`.

## CON-525. The Producers page has a toggle that works without a script, and never names a client it has not seen (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
Test: pages::producers::tests::each_producer_is_a_native_disclosure_open_for_the_one_with_a_transaction_and_a_deep_link_opens_another (removed in R5), no_producer_row_is_a_link_with_a_query_and_nothing_says_aria_expanded (removed in R5), the_tail_marks_the_stable_offset_and_counts_the_held_records (removed in R5), a_producer_is_shown_with_a_name_only_when_the_recorder_gave_one (removed in R5), the_transaction_page_has_the_countdown_and_the_two_ways_on (removed in R5).

## CON-530. A compacted log is drawn from what the cleaner left
**Untested since R5.** The tests named here drove the server-rendered pages and were removed with them (2026-11-13). The behaviour is the data's (`gather/`, `model/`, the backend, unchanged) and the API serves the same view-model, so it holds there; no test names it at the API yet.
Test: a_compacted_log_is_drawn_from_what_the_cleaner_left (removed in R5) (tests/live_log.rs (removed in R5)).

## CON-531. A log that is not due says why, and the cleaner then leaves it alone
**Untested since R5.** The tests named here drove the server-rendered pages and were removed with them (2026-11-13). The behaviour is the data's (`gather/`, `model/`, the backend, unchanged) and the API serves the same view-model, so it holds there; no test names it at the API yet.
Test: a_log_that_is_not_due_says_why_and_the_cleaner_then_leaves_it_alone (removed in R5).

## CON-532. Retention is named before and after a pass
**Untested since R5.** The tests named here drove the server-rendered pages and were removed with them (2026-11-13). The behaviour is the data's (`gather/`, `model/`, the backend, unchanged) and the API serves the same view-model, so it holds there; no test names it at the API yet.
Test: retention_is_named_before_and_after_a_pass (removed in R5).

## CON-533. An empty log says how the first record gets there
**Untested since R5.** The tests named here drove the server-rendered pages and were removed with them (2026-11-13). The behaviour is the data's (`gather/`, `model/`, the backend, unchanged) and the API serves the same view-model, so it holds there; no test names it at the API yet.
Test: an_empty_log_says_how_the_first_record_gets_there (removed in R5).

## CON-534. The Producers page names the transaction that holds the stable offset
**Untested since R5.** The tests named here drove the server-rendered pages and were removed with them (2026-11-13). The behaviour is the data's (`gather/`, `model/`, the backend, unchanged) and the API serves the same view-model, so it holds there; no test names it at the API yet.
Test: the_producers_page_names_the_transaction_that_holds_the_stable_offset (removed in R5).

## CON-535. A transaction page says what it holds back and what the timeout will do
**Untested since R5.** The tests named here drove the server-rendered pages and were removed with them (2026-11-13). The behaviour is the data's (`gather/`, `model/`, the backend, unchanged) and the API serves the same view-model, so it holds there; no test names it at the API yet.
Test: a_transaction_page_says_what_it_holds_back_and_what_the_timeout_will_do (removed in R5).

## CON-536. An aborted transaction is listed with its marker
**Untested since R5.** The tests named here drove the server-rendered pages and were removed with them (2026-11-13). The behaviour is the data's (`gather/`, `model/`, the backend, unchanged) and the API serves the same view-model, so it holds there; no test names it at the API yet.
Test: an_aborted_transaction_is_listed_with_its_marker (removed in R5).

## CON-537. A caller without rights is told no such topic
**Untested since R5.** The tests named here drove the server-rendered pages and were removed with them (2026-11-13). The behaviour is the data's (`gather/`, `model/`, the backend, unchanged) and the API serves the same view-model, so it holds there; no test names it at the API yet.
Test: a_caller_without_rights_is_told_no_such_topic (removed in R5).

## CON-538. Hostile ids and keys are escaped, and a 249-byte topic name fits (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
Test: hostile_ids_and_keys_are_escaped_and_a_long_topic_name_fits (removed in R5); pages::log::tests::a_topic_name_that_is_hostile_is_escaped_everywhere_it_is_shown (removed in R5), pages::producers::tests::hostile_ids_are_escaped_in_every_place_they_appear (removed in R5).

## CON-539. A GET of these pages reads and changes nothing
**Untested since R5.** The tests named here drove the server-rendered pages and were removed with them (2026-11-13). The behaviour is the data's (`gather/`, `model/`, the backend, unchanged) and the API serves the same view-model, so it holds there; no test names it at the API yet.
Test: a_get_of_these_pages_reads_and_changes_nothing (removed in R5).

## CON-540. The fragments are regions and answer with a tag
The log and producers regions (`log-stats`, `log-ruler`, `log-cards`, `producers-stats`, `producers-table`) are plain GETs that answer with an ETag. Test: `tests/live_log::the_fragments_are_watchable_and_answer_with_a_tag`.

### What these boards show that the broker cannot supply yet

Tagged in the code (`pages/log.rs`, `pages/producers.rs`) and in `debt.md` J231 to J248.
- "Compact now" is Lab: not built, no button stands where it stood.
- "Flushed through offset N": the log does not keep how far its flush got; the Durability card says how the log is flushed and what the last stop left.
- The first entries of a segment's index; the epoch history of a producer ("raised from 1 at 17:14:11"); the offsets a transaction staged; whether the producer or the timeout aborted.

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

## CON-578. The weight limits the page names are the ones weight.md writes (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
Test: settings_words::tests::the_limits_are_the_ones_weight_md_writes (removed in R5).

## CON-579. A start-up warning holds exactly when the broker would say it
SEC-188 (console beyond loopback in front of SASL, TLS, ACLs or a loopback listener: a wider door than the broker; with ACLs made after an open start and no `--console-unprotected` the console answers this machine only, SEC-250, and the warning says so without holding), SEC-189 (SASL without TLS beyond loopback), SEC-191 (SASL with `--acl-default allow`). Test: `access_words::tests::each_warning_holds_exactly_when_the_broker_would_say_it_at_start`, `access_words::tests::acls_made_after_an_open_start_shut_the_console_to_other_machines_and_the_warning_says_so`.

## CON-580. The broker's view names users and super users and holds no secret
`ConsoleView::access` (DESCRIBE on the cluster), `intervals` (DESCRIBE_CONFIGS) and `data_size`. Test: `view::tests::access_names_the_flag_users_and_the_super_users_and_never_what_they_log_in_with`.

## CON-581. The writes the console issues are the ones the boards draw (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
CreateAcls and DeleteAcls join the list. Test: actions::tests::actions_match_boards (removed in R5).

### What these boards show that the broker cannot supply yet

Tagged in the code and in `debt.md` J261 to J278.
- Idle memory and threads on a system that does not let a process read them (macOS): the tile says "Measured on Linux only".

## Integration of 9g and 9h, and what they left open (slice p9x; CON-582 to CON-589)

## CON-582. Settings names the clients, the cleaner's last round and a group's expiry from what the broker holds
"Today fulfilment speaks Fetch v12 and orders Produce v9, all inside the ranges" is the newest client of the tape that used each of Fetch and Produce with the version of its newest request (a client with no id is nobody; a version outside the range offered is named as outside); "Its last round was on inventory.levels at 21:31 UTC, and the next look finds it due" is the newest round the cleaner put in place on a topic the console may describe, and the planner's own verdict on that log now; "analytics' expire in 3 d 2 h" on the Offsets retention row is the soonest date the sweep takes a group's offsets. With no recorder no client is named; with no round the page says none was put in place since the broker started; with no date no group is named. Test: live_settings_wired::settings_names_the_clients_the_last_round_and_the_expiry_from_what_the_broker_holds (removed in R5), pages::settings::tests::the_sentences_from_the_recorder_and_the_cleaner_say_only_what_they_were_given (removed in R5), `settings_live::tests::a_version_outside_the_range_or_of_an_api_not_offered_is_not_inside`.

## CON-583. Without a recorder or a round the Settings page names none and invents none
**Untested since R5.** The tests named here drove the server-rendered pages and were removed with them (2026-11-13). The behaviour is the data's (`gather/`, `model/`, the backend, unchanged) and the API serves the same view-model, so it holds there; no test names it at the API yet.
Test: live_settings_wired::without_a_recorder_or_a_round_the_page_names_none (removed in R5).

## CON-584. The Settings edit dialog and the two ACL dialogs are overlays over their pages (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
`/settings/config/edit`, `/access/acls/new` and `/access/acls/delete` answer the parent page with the dialog open on it to a browser and the dialog alone to a script (`X-Fragment`); a refusal or an invalid field answers the dialog again, a result is a page or a redirect to the parent. Test: overlays::a_direct_visit_is_the_parent_page_with_the_overlay_open_on_it (removed in R5), overlays::a_script_is_sent_the_overlay_alone_marked_as_one (removed in R5), pages::settings::tests::the_edit_dialog_is_one_form_with_a_check_a_save_and_the_call (removed in R5).

## CON-585. The users table has the board's Iterations column (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
The iteration count of a SCRAM credential is a public parameter (the server-first message sends it to every client): one count when a user's mechanisms agree, each with its mechanism when they differ, `none` for a user with only PLAIN; password, salt, stored key and server key are still never read. Test: access_words::tests::the_iteration_cell_is_one_count_when_they_agree_each_when_they_differ_and_none_without_scram (removed in R5), pages::access::tests::no_secret_field_exists_to_be_shown (removed in R5), live_access::the_page_says_what_a_secured_broker_holds_by_name_only (removed in R5).

## CON-586. The palette also goes to the pages (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
Home, Live, Topics, Groups, Access, Settings and Connect come first, each tagged `page` and going to its own address under the base; with no script the filtered page keeps the pages the filter names. Test: jump::tests::the_pages_come_first_each_goes_to_its_own_address_and_a_filter_keeps_the_ones_it_names (removed in R5).

## CON-587. The Log and Producers pages have the topic's wire dock, and a producer is named by the client that wrote (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
The dock (the last requests that touched the topic, from the recorder) sits under every page of the two tabs and is watched with the page; "Seen as" lists the clients whose Produce carried the producer's id, with the library tags their ApiVersions gave, at most three, newest first, and "not known" for a producer the tape never saw write; a hostile client id is text. Test: live_log_wired::a_producer_is_named_by_the_client_that_wrote_and_the_dock_is_under_both_pages (removed in R5), pages::producers::tests::a_producer_is_shown_with_a_name_only_when_the_recorder_gave_one (removed in R5).
# Slice 9f: the group event ring and the Story (CON-600 to CON-617)

Three boards: `group-story` (a page), `story-rebalance` (a drawer over the Story page), `story-flapping` (a state: the rule held against the group). The heart of the slice is in `kafka-broker`: the only slice that touches the group state machines, so the rule is absolute: a note is an extra effect, never a different transition.

## CON-600. A note is an extra effect, and the machines do exactly what they did before they took notes
`Effect::Note` is pushed beside the effects of a step and read by nobody who decides anything. 500 seeded sequences of 400 steps (members joining, rejoining with other protocols, syncing, heartbeating, leaving by id and by static instance, going silent, consumer-protocol members coming and going, topics growing) are run through a group and every answer, every timer and every description it gives, notes left out, is hashed; the hash is a constant worked out on the machines as they were before any note was added, one for the build with the console's view (`inspect`) and one without. The same constant holds with the notes on, so a note changes no transition. Test: `groups::group::tests::differential::the_machine_does_what_it_did_before_it_took_notes`. The Phase 4 and 5 gates are unchanged and were run: the broker's group tests, the oracle replay (58 of 58) and the interop group cells.

## CON-601. Every way a classic rebalance begins says why
A member that joins (the first of an empty group, or one that was not in it), one that leaves, one whose session runs out, one that joins again offering other protocols, the leader of a running group joining again, a group made classic again by the last consumer-protocol member leaving: each is a cause on the note of the rebalance it began, with the member, the client id, the members before and after, and the wait the members were given; a member dropped as the generation forms (the rebalance timeout) is its own note; the generation formed and the assignment handed out (what each member holds) are noted too. A join into a rebalance already forming begins nothing and says nothing. Tests: `groups::machine::tests::notes::*`.

## CON-602. Every move of a consumer group's epoch says why
A member joining, leaving (epoch -1), expiring or not giving up what it was told to in its rebalance timeout; a subscription, pattern or assignor changed; a right to describe a topic given or taken; partitions added to, or a topic made or deleted among, the topics the members read (`TopicsChanged`, which the broker sends every group); a classic group made a consumer group. An epoch that does not move says nothing. Tests: `groups::group::tests::notes::*`, and over a real broker `view_group_events::partitions_added_to_a_topic_a_consumer_group_reads_are_a_topic_that_changed` and live_story::partitions_added_to_a_topic_a_consumer_group_reads_are_told_as_a_topic_that_changed (removed in R5).

## CON-603. The ring is bounded, lazy and kept by the group's own task
200 notes per group, 64 KiB of notes per group, 100 groups (the oldest group's ring is let go when a 101st speaks), emptied at a restart. The ring belongs to the group's task: a note is a push with no lock; only the first note of a group and the end of its task touch the shared ledger. A group that never rebalances never has a ring: an idle broker pays nothing. Tests: `groups::events::tests::*`.

## CON-604. The client's own reason is kept clipped and never logged
JoinGroup v5 and LeaveGroup v5 carry a `reason`. It is clipped to 128 bytes at a character boundary with control characters left out, as it is read (a 900 KiB reason costs as much as a short one and the Go edition's 1.8 GiB cannot happen), kept only in a note, and never written to a log. Tests: `groups::note::tests::*`, `view_group_events::a_join_and_a_leave_are_told_with_their_causes_and_the_clients_clipped_reasons`, live_story::a_classic_group_that_joins_and_leaves_tells_its_story_with_a_clipped_reason (removed in R5).

## CON-605. `ConsoleView::group_events` is read-only and Describe-checked
It returns owned plain structs; a caller that may not DESCRIBE the group is told nothing and not that the group exists; a group the broker does not have in memory (restarted, deleted) has `None`, which is not the same as an empty story; asking is a request to the group's task like any other and changes nothing. Tests: `view_group_events::*`.

## CON-606. The Story is the group's generations, newest first, from the ring
**Untested since R5.** The tests named here drove the server-rendered pages and were removed with them (2026-11-13). The behaviour is the data's (`gather/`, `model/`, the backend, unchanged) and the API serves the same view-model, so it holds there; no test names it at the API yet.
`/groups/{group}/story`: Now (what the group is doing, and for a rebalance the time the members have left, live), each generation with its cause, who, the members before and after, how long it took to settle, what each member held, who did not join in time, how long it lasted, the members that were in it; the causes the broker can name with how often each happened here; whether the group is flapping; another group that rebalances rarely, for contrast; and the footnote: 200 events per group, emptied at a restart. Tests: boards_story::the_story_has_the_boards_four_generations_newest_first_and_its_cards (removed in R5), `story::tests::*`, `live_story::*`.

## CON-607. A rebalance opens as a drawer over the Story (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
`/groups/{group}/story/{generation}` is a drawer over the Story page (a direct visit is the Story with the drawer open; a script is sent the drawer alone; Close and the scrim are links back); a generation the ring does not hold is a 404. Tests: boards_story::the_rebalance_drawer_says_who_when_the_deadline_and_what_moves (removed in R5), live_story::a_script_gets_the_drawer_alone_and_a_browser_gets_the_story_with_it_open (removed in R5).

## CON-608. The flapping rule is one pure function and its words are its code
Three or more expired-then-joined cycles in ten minutes: a session expiry, then a join by the same client, each expiry used once; the window's edge counts, a millisecond older does not. `story::flap_of`; the page's verdict, rows and sentence are made from what it returns. Tests: `story::tests::three_expired_then_joined_cycles_in_ten_minutes_are_flapping_and_two_are_not`, `story::tests::the_window_is_ten_minutes_and_its_edge_counts`, `story::tests::the_member_that_comes_back_is_the_client_that_expired`, `story::tests::the_rule_held_against_the_boards_group_says_what_the_board_says`.

## CON-609. A clean leave never counts toward flapping
A member that sends LeaveGroup told the broker it was going; nothing ran out. Any number of leaves and joins is no cycle. Tests: `story::tests::a_clean_leave_never_counts_and_a_join_after_a_leave_is_no_cycle`, live_story::a_member_that_expires_and_comes_back_until_the_rule_names_it_is_flapping (removed in R5).

## CON-610. The Story tab is a link with a count on every page of a group (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
Members, Lag and Story carry the number of rebalances the ring holds for the group (Members the members, Lag the lag, as before); the tab that is showing is the current page. Tests: live_story::a_classic_group_that_joins_and_leaves_tells_its_story_with_a_clipped_reason (removed in R5) (the count on the Members page), `boards_story::*`.

## CON-611. A cause is given only when the ring has one
The Story's rows and cards give a cause only when the broker's ring of notes has it; a group the broker has no notes of says what the machine says now. Test: `tests/live_story::a_classic_group_that_joins_and_leaves_tells_its_story_with_a_clipped_reason`.

## CON-612. The Story says what the ring is
**Untested since R5.** The tests named here drove the server-rendered pages and were removed with them (2026-11-13). The behaviour is the data's (`gather/`, `model/`, the backend, unchanged) and the API serves the same view-model, so it holds there; no test names it at the API yet.
"200 events per group, emptied at a restart" under the generations, and "Generations 1 to N are older than the ring keeps" when it is full. Test: boards_story::the_story_has_the_boards_four_generations_newest_first_and_its_cards (removed in R5).

## CON-613. The regions of the Story are listed once (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
`story-stats` and `story-now` are listed in regions.rs (removed in R5). Test: regions::tests::a_page_lists_each_region_once (removed in R5).

## CON-614. The three boards pass the structure gate in both renders
Fixture render (`tests/boards_story::every_board_of_the_slice_renders_and_is_written_for_the_tool`), every difference in `design/allow.json` with a reason.

## CON-615. A group id and a generation in the address are hostile input
**Untested since R5.** The tests named here drove the server-rendered pages and were removed with them (2026-11-13). The behaviour is the data's (`gather/`, `model/`, the backend, unchanged) and the API serves the same view-model, so it holds there; no test names it at the API yet.
Shown as text, put in links only through `segment`, a generation that is not a number or not in the ring is a 404 that echoes nothing. Test: live_story::a_hostile_group_id_is_escaped_and_encoded_on_the_story_pages (removed in R5).

## CON-616. A caller that may not describe the group gets no group
**Untested since R5.** The tests named here drove the server-rendered pages and were removed with them (2026-11-13). The behaviour is the data's (`gather/`, `model/`, the backend, unchanged) and the API serves the same view-model, so it holds there; no test names it at the API yet.
The Story, the drawer and the flapping page of a group the caller may not DESCRIBE are the 404 of a group that is not there; nothing of the story leaks. Test: live_story::a_caller_who_may_not_describe_the_group_is_told_no_group_not_that_it_has_a_story (removed in R5).

### What these boards show that the broker cannot supply, and what the page does instead
Tagged in `docs/port/debt.md` J291 to J302: "Topic changed" on a classic group is "Leader rejoined" (a client sees the change and its leader joins again); the topic that changed is not named; "last told to rejoin" is "last heard"; a member joining a rebalance already forming is folded into it; the causes' phrases are the boards', since no recording carries a Kafka reason.

## CON-617. A group with one rebalance and nothing since has a quiet story
**Untested since R5.** The tests named here drove the server-rendered pages and were removed with them (2026-11-13). The behaviour is the data's (`gather/`, `model/`, the backend, unchanged) and the API serves the same view-model, so it holds there; no test names it at the API yet.
Stable, one generation, none forming: the sentence is one line ("One rebalance in the last 3 h 44 m: search-indexer-1 joined at 18:30:12, 3 h 44 m ago. Nothing has moved since."), there is no Now card and no cards of causes, the one generation is the whole timeline, and a card says why a quiet story is a good one (the lag is not caused by rebalancing; look at Lag). The header's button is "See its lag". This is the `group-story-calm` frame of the canvas, which is not one of the three boards. Test: boards_story::a_group_with_one_rebalance_and_nothing_since_has_the_quiet_story (removed in R5).
# The pages pass (UX agent 2, CON-700 to CON-709 so far)

Branch `ux-pages`. The owner drove the built console and found it broken in feel: a breadcrumb that says `read_committed`, an unbuilt link that lands on Home, a Consumers tab that left the topic, a toolbar that moves, an expander that scrolls to the top. Each rule below is a defect class, with the test that keeps it out. Pages: trail.rs (removed in R5), handlers/unbuilt.rs (removed in R5), pages/topics/bar.rs (removed in R5), `pages/topics/consumers.rs`, pages/topics/common.rs (removed in R5), ui/disclosure.rs (removed in R5); the Chrome scenarios are `tools/ux/pages-drive.ts`, the hunt is `tools/ux/pages-audit.ts` (results in `docs/port/console/ux-audit.md`).

## CON-700. The breadcrumb is the entity path and nothing else (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
`Resources > Topics > orders > Partitions`, `Resources > Groups > fulfilment > Members`, `Live`, `Settings`: never a filter, a mode, a partition chip, an isolation level, a tab state or a state of the page (the old `orders-0`, `read_committed`, `Paused`, `A busy minute`, `Lag: no commits yet`, `Create refused`, `Record sent`). `trail::Place` has a field for every entity and none for a query, so a handler that holds a filter cannot put it in the trail. Test: trail::tests::a_place_is_its_entity_path_and_nothing_else (removed in R5), trail::tests::no_place_takes_a_filter_so_a_filter_cannot_be_in_a_trail (removed in R5), `tests/ux_pages::no_breadcrumb_carries_a_query_value_a_mode_or_a_state_and_the_title_is_its_last_step`.

## CON-701. Every step of a breadcrumb is a page that answers (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
`Resources` links to the Topics list (the rail's own Resources entry goes there too; there is no hub to invent). The last step has no link. A name is cut at 60 characters and escaped. Test: trail::tests::every_link_of_a_trail_is_a_page_the_console_serves (removed in R5), trail::tests::the_last_step_has_no_link_and_every_other_step_links (removed in R5), trail::tests::a_name_is_shown_cut_at_sixty_characters_and_escaped_by_the_shell_not_here (removed in R5), `tests/ux_pages::every_link_of_every_breadcrumb_is_a_page_that_answers`.

## CON-702. The tab title is the last step of the breadcrumb
`Shared::document_over` takes the title from the last step whatever the handler passed, so the tab says what the breadcrumb says. Test: `tests/ux_pages::no_breadcrumb_carries_a_query_value_a_mode_or_a_state_and_the_title_is_its_last_step`.

## CON-703. A screen this edition lacks is one designed page (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
`Handler::Planned` answers `pages::unavailable`: its own heading and breadcrumb (`Lab`, `Resources > Topics > orders > Log`), "Not available in this edition", one honest sentence ("Lab, fault injection for requests, is not part of this edition yet"), HTTP 200 and a way back: the page the visitor came from when the `Referer` is one of ours and not the page itself, else the section. Every page carries `<meta name="robots" content="noindex">`. Test: handlers::unbuilt::tests::the_way_back_is_the_page_we_came_from_when_it_is_one_of_ours (removed in R5), handlers::unbuilt::tests::a_referer_that_is_not_ours_or_not_a_page_is_no_way_back (removed in R5), handlers::unbuilt::tests::under_a_prefix_only_a_page_of_the_console_counts (removed in R5), `tests/ux_pages::an_unbuilt_screen_is_one_designed_page_and_never_home_under_another_title`, `tests/ux_pages::the_unbuilt_page_goes_back_to_where_the_visitor_came_from_or_to_the_section`.

## CON-704. No link leads to Home's content under another title
Where the product wants an unbuilt screen visible it is a link to the page above (the rail, the tabs); `.soon` and `.is-soon` are the CSS for a disabled-looking item with a "Soon" tag where a page prefers that; elsewhere it is not linked. A test finds no `data-extra` landmark of Home on any unbuilt page. Test: `tests/ux_pages::an_unbuilt_screen_is_one_designed_page_and_never_home_under_another_title`.

## CON-705. The Consumers tab stays inside the topic
`/topics/{topic}/consumers` lists the consumer groups with offsets committed on this topic (ListGroups, OffsetFetch, DescribeGroups, ListOffsets, group detail through `ConsoleView`), bounded at 25 groups (the furthest behind first), each with its state, protocol, members and how many hold this topic, and a row per partition of THIS topic: log end, committed, and a lag bar with the Groups pages' own arithmetic and words (`crate::lag`, `cell_lag`). The group name links to its page; the tab's count is the number of groups listed; nobody reading says "No consumer group has committed offsets for orders". A group that reads without committing is not listed (the tab's count says the same). It is better than the board: there is none. Test: `tests/live_consumers::the_consumers_tab_lists_the_groups_that_committed_on_the_topic_with_their_lag_per_partition`, `tests/live_consumers::a_topic_nobody_has_committed_on_says_so_and_a_missing_topic_is_not_found`, `tests/live_consumers::on_a_broker_with_acls_the_tab_lists_only_what_the_caller_may_describe`, `tests/live_consumers::hostile_group_and_topic_names_are_text_on_the_tab`.

## CON-706. The tabs of a topic share one header
`pages::topics::topic_frame` is the head (name, words, the topic's sentence, buttons that are a function of the topic) and the five tabs with their counts; Messages, Config and Consumers start with it, byte for byte but the tab that is on, so switching tabs changes nothing above the tab body (Chrome: the boxes of the header and of every tab are identical on the three pages). The tabs keep one font weight so the underline moves and the text does not. Test: `tests/ux_pages::the_tabs_of_a_topic_share_one_header`; Chrome: `tools/ux/pages-drive.ts topic-tabs-stable`.

## CON-707. The Messages toolbar is one layout in every state
Row one: the partition control, the find box, the time box, the shortcut to a held record, and at the right "Reading as" with `read_uncommitted` then `read_committed` in that order. Row two is always one line tall: the filters that are on, each a chip with a link that takes it off, "Clear filters", or "No filters: the newest records of every partition." The open-transaction callout is above the toolbar in every state of a topic that has an open transaction (it used to vanish when a partition was chosen). Choosing a partition, a filter or an isolation level changes the data region `#topic-messages` and what row two says, never where anything is. `read_committed` is the same page: its explainers (the zoomed offsets, what each isolation level gets, who is held back) are a section under the table that is open while reading `read_committed` and shut otherwise. Test: `tests/ux_pages::the_messages_toolbar_is_one_layout_and_the_callout_stays_in_every_state`, `tests/ux_pages::the_filters_that_are_on_are_chips_that_can_be_taken_off_and_none_is_a_sentence`; Chrome: `tools/ux/pages-drive.ts messages-stable`.

## CON-708. An expander is a native disclosure, never a link (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
"Where from", "Kafka's text" and the replication section of the Config page are `<details>`/`<summary>` (`ui::disclosure`): they open in place with no navigation, the scroll stays, they work with no script, and they open with a 160 ms motion that reduced motion switches off. What is open is not in the address; `?open=where:segment.bytes` and `?replication=1` are deep links the server renders `open`. The filter chips carry the filter and not what is open. Test: ui::disclosure::tests::a_disclosure_is_a_details_that_is_open_only_when_asked (removed in R5), ui::disclosure::tests::nothing_in_a_disclosure_can_navigate (removed in R5), pages::config::tests::an_expander_is_a_native_disclosure_that_is_open_only_for_a_deep_link_and_never_a_link (removed in R5), pages::config::tests::the_replication_group_is_shut_until_asked_for_and_says_how_many_it_holds (removed in R5), `tests/ux_pages::no_page_has_an_expander_that_is_a_link_with_a_query`; Chrome: `tools/ux/pages-drive.ts expanders`.

## CON-709. Numbers do not jitter, rows answer hover and focus
Tables and counters use tabular figures; a row of a table shows hover and keyboard focus alike; a target that a same-page link or the address names clears the sticky top bar (`scroll-margin-top`) and a same-page jump scrolls smoothly (not under reduced motion). Test: the Chrome audit `tools/ux/pages-audit.ts`; CSS in `design/console.css`.

## CON-588. Log, Producers and a transaction use the topic's own head and trail (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
The five tabs of a topic, and a transaction under Producers, start with the same head and the same tab row (`topic_frame`), byte for byte but the tab that is on (`tests/ux_pages::the_tabs_of_a_topic_share_one_header`); what a page used to carry in its head (the words of the log, the partition, the sentence that follows the data, a transaction's state and its buttons) is its first block under the tabs. The breadcrumb of these pages and of Settings and Access is a `trail::Place` (`TopicPage(topic, Log | Producers)`, `Transaction`, `TransactionHeld`, `Settings`, `Access`): a saved or refused page, a dialog and a partition chip do not change it. A producer's batches are a native `<details>` (never a link with `?open=`), open for the producer that holds a transaction and for the ids a deep link names. Test: ux_pages::no_breadcrumb_carries_a_query_value_a_mode_or_a_state_and_the_title_is_its_last_step (removed in R5), ux_pages::no_page_has_an_expander_that_is_a_link_with_a_query (removed in R5), pages::producers::tests::each_producer_is_a_native_disclosure_open_for_the_one_with_a_transaction_and_a_deep_link_opens_another (removed in R5).

## CON-589. A dialog's field to fix is a 200 to a script and a 422 to a plain POST (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
Test: overlays_more::a_field_to_fix_is_a_200_to_a_script_and_a_422_to_a_plain_post (removed in R5).
## CON-650. Every route answers a whole document; an htmx request is answered with the region (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
A request with `HX-Request` (and not a history restore) is answered with the page's region only: the title, `main` (breadcrumb, banner, content, the stream's address) and, out of band, the rail and the overlay root; the shell is not sent again. Any other request, and a history restore, is answered with the whole document. Every answer says `Vary: HX-Request, HX-History-Restore-Request, HX-Target`. Test: an_htmx_request_gets_the_region_and_a_plain_one_the_whole_document (removed in R5) (tests/region.rs), a_history_restore_wants_the_whole_document (removed in R5).

## CON-651. The client stack runs under the strict policy and is the console's own, pinned and licensed (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
The document loads htmx, idiomorph, our two scripts, Alpine's anchor plugin and Alpine's CSP build, in that order, all `<script src>` (no inline script), with htmx configured by a `<meta>` that turns `allowEval` and script tags off and limits requests to the page's own origin. The vendored files are in the manifest with a licence text each, under 160 KB in all; our two scripts evaluate no string. Test: the_document_names_the_scripts_in_load_order_and_configures_htmx_without_eval (removed in R5) (tests/region.rs), the_vendored_script_stays_within_the_budget_of_the_stack_decision (removed in R5), no_script_of_ours_evaluates_a_string (removed in R5), a_licence_is_kept_for_every_third_party_file (removed in R5) (src/assets/tests.rs).

## CON-652. An overlay's write that answers with a page puts it in main and gives the address back (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
A request that targeted the overlay root and got a page (not an overlay) is told to swap it into `main` (cross-faded), and to replace the overlay's history entry with the page's own address (the redirect's, for a GET) or with the page behind the overlay it was posted from (`X-Overlay-Parent`, a same-origin path only); an overlay answered to the overlay root is left alone. Test: a_page_answered_to_a_request_for_the_overlay_root_says_where_it_goes (removed in R5), a_page_answered_to_a_post_from_an_overlay_takes_the_address_of_the_page_behind_it (removed in R5) (tests/region.rs), `tools/dom/overlay.test.ts` (forms in an overlay).

## CON-653. Only a page fetched ahead of a click may be kept by the browser, and only for seconds (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
The script fetches a boosted link's page when the pointer rests on it for 65 ms or it takes focus, marking the request `HX-Preloaded`; only such an answer carries `Cache-Control: private, max-age=5`, every other answer is `no-store`. Links that open an overlay, change a region in place or leave the console are not fetched ahead. Test: only_a_preloaded_page_may_be_kept_by_the_browser_and_only_for_seconds (removed in R5) (tests/region.rs), `tools/dom/navigation.test.ts` (preload on intent); in Chrome `nav-preload` (tools/ux).

## CON-654. A navigation swaps main only, and a control that owns a region changes only that region (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
Links and forms are boosted: a click swaps `main`, keeps the shell, updates the title, the breadcrumb and the rail, pushes the address, moves focus to the new page, starts at the top, re-opens the stream for the page and cross-fades in a same-document View Transition; Back restores the scroll. A control declared with in_place_link (removed in R5) / in_place_form (removed in R5) / `seg(.., Some(id))` morphs only its region: scroll, focus, typed text and the toolbar stay and only what changed fades. A request that takes over 250 ms shows a thin hint; one that cannot be made is followed as a plain link or form. Test: `tools/dom/navigation.test.ts` (a boosted link, a control that owns a region, history, the progress hint); in Chrome `nav-swaps-main-only`, `nav-scroll-new-page-top-and-back-restores`, `nav-view-transition`, `filter-in-place`, `live-filter-in-place` (tools/ux).

## CON-655. The drawer is full height on the right edge, above the top bar, and slides in from the edge (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
A drawer is a panel from y = 0 to the bottom of the window, on the right edge, 480 px wide, above the top bar and the rail, with a shadow to its left; it slides in over 240 ms from `translateX(100%)` (cubic-bezier .22, 1, .36, 1), the scrim fades in 180 ms, closing slides out over 180 ms; a dialog fades in from scale .98 over 180 ms; nothing moves with reduced motion; the scroll lock moves nothing (`scrollbar-gutter: stable`). Test: the_motion_is_gentle_and_only_for_people_who_have_not_asked_for_less (removed in R5) (tests/overlays.rs); in Chrome `drawer-geometry`, `drawer-motion`, `dialog-geometry`, `scroll-lock-does-not-shift-the-page`, `drawer-closes-three-ways-and-keeps-the-page` (tools/ux).

## CON-656. No console control is a browser-default select (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
A Select, a Combobox, a DropdownMenu, a Tooltip, a Popover and a segmented control are maud components that render a complete no-script control (a real `<select>`, links, a `title`) and an enhanced control for Alpine, with roles and ids a screen reader needs, and every label, value and description as text. The Live filters, the Produce partition and the Reset topic are Selects. Test: a_select_is_a_real_select_underneath_and_a_listbox_on_top (removed in R5), a_combobox_is_a_select_underneath_and_a_searchable_listbox_on_top (removed in R5), a_menu_is_a_button_and_a_menu_whose_items_are_links_and_buttons (removed in R5), a_segmented_control_is_a_group_of_links_with_one_current_and_may_work_in_place (removed in R5), hostile_labels_values_and_descriptions_are_text_everywhere (removed in R5), the_alpine_stand_ins_never_leak_into_the_page (removed in R5) (src/ui/components/tests.rs).

## CON-657. The components obey the keyboard contract
Enter, Space or the arrows open a Select; arrows, Home, End and type-ahead move; Enter chooses and fires `change` on the native control; Esc closes, returns focus and is not seen by an overlay behind; a Combobox filters, says "No match", and may keep free text; a menu moves focus through its items with wrap and returns it to its button; a tooltip waits 400 ms of hover or shows at once on focus; a segmented control has one Tab stop. Test: `tools/dom/components.test.ts`; in Chrome `select-keyboard`, `select-mouse-and-outside`, `select-flips-near-the-bottom`, `combobox-filters-and-creates`, `menu-keyboard`, `tooltip-delay`, `segmented-roving` (tools/ux).

## CON-658. A swap of a region that holds a component leaves the component as the script made it
Idiomorph edits the page into the server's new markup without undoing what a component's script did (the enhanced control shown, the plain one hidden, the accessible names) and leaves an open component alone; an unchanged answer fades nothing. Test: `tools/dom/navigation.test.ts` (what changed fades, and an answer that changes nothing animates nothing); in Chrome `live-filter-in-place` (focus stays on the Select inside the morphed Live toolbar).

# The integration pass (UX agent 3, CON-750 to CON-759)

Branch `ux-integrate`. The foundation (htmx, Idiomorph, Alpine, the components) and the page work were built apart; the audit (`docs/port/console/ux-audit.md`) found every same-page control still a boosted swap of `main`: a cross-fade and a jump to the top where the owner asked for nothing to move. This pass wires them together. Code: ui/inplace.rs (removed in R5), region.rs (removed in R5), `pages/{topics,groups,config}/`, `assets/{console,components}.js`; the Chrome scenarios are `tools/ux/scenarios/{regions,popups,palette,transitions}.ts`.

## CON-750. Every control that changes only the data it owns updates its region in place (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
A chip, a toolbar, a pager, a search box, "Clear filters" and "When do these expire?" ask for the page and morph ONE region of it (`#topics-view`, `#groups-view`, `#topic-view`, `#config-view`, `#group-lag-view`, `#jump-view`, `#live-wire`): `main` is not swapped, so there is no cross-fade, no scroll to the top and nothing above the region moves (layout shift 0, `scrollY` unchanged, the focus stays on the control). Each is also a plain link or GET form. A region holds the toolbar as well as the data it drives, so a chip that changes the toolbar's own state (which chip is on, a count) changes it with the data. Test: ui::inplace::tests::a_link_is_a_plain_link_that_asks_for_one_region (removed in R5), ui::inplace::tests::a_toolbar_form_is_a_plain_get_that_submits_in_place (removed in R5), ui::inplace::tests::a_region_is_a_wrapper_with_the_id (removed in R5); in Chrome `region-topics`, `region-groups`, `region-config`, `region-lag-expiring`, `region-jump`, `region-messages`, `region-messages-older`, `region-messages-time`.

## CON-751. A filter has a clean address, a pushed entry, and typing replaces it (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
The handler says what the state is once it has read it (`region::canonical`): the layer puts that address into the history (a form's duplicate and empty fields never reach the bar), pushes it for a chip, replaces the entry for typing in a search box (`X-History: replace`, 250 ms after the last key), and adds no entry when nothing changed. Back restores the earlier state. Test: region::tests::a_control_that_changes_the_address_pushes_the_clean_one (removed in R5), region::tests::a_control_that_changes_nothing_adds_no_history_entry (removed in R5), region::tests::typing_replaces_the_entry_with_the_clean_address (removed in R5), region::tests::the_handler_names_the_clean_address_and_nothing_outside_a_request_does (removed in R5); in Chrome the `url` and `history` checks of every `region-*` scenario.

## CON-752. A region the answer does not have is never emptied
When the page a state calls for is another page (an offline topic, the empty list), the answer has no such region; the script takes the address the way a browser with no script would instead of swapping nothing. The offline and empty states of a topic carry the region as well, so the usual case swaps. Test: in Chrome `region-missing-falls-back-to-navigation`.

## CON-753. The components are used where they belong (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
The partitions of a topic are a Segmented control (a Select past eight), "Reading as" and the Groups states (with their counts) are Segmented controls, "From time" is a Popover with a proper field, a Live filter list longer than eight is a Combobox, the two actions of a topic's own config value are a DropdownMenu. Each choice is also a link or a real control with no script. Test: ui::components::tests::a_segmented_control_is_a_group_of_links_with_one_current_and_may_work_in_place (removed in R5); in Chrome `region-messages`, `region-groups`, `region-messages-time`, `row-menu-opens-an-overlay`.

## CON-754. No menu, select or popover overlaps the rail or leaves the window
Floating UI keeps a card in the window but knows nothing of the rail: a card is nudged sideways (a margin the anchor never touches) to sit right of the rail and inside the window. Test: in Chrome `popups-stay-clear-of-the-rail` (the card would have been 114 px over the rail).

## CON-755. The palette has sections, recents and an empty state (retired in R5)
**Retired in slice R5 (2026-11-13).** This rule described the server-rendered console (maud pages, htmx, Alpine, idiomorph, the boards gate), which was removed: the React console's rules are CON-1000 and up. Its tests went with it and are named here unquoted.
Topics, Groups and Pages, each under its own heading (a section with no name has none; a filter hides the heading of a section with no match); the last five places opened from it are kept in this browser as a kind and a name, shown first under "Recent" as copies of the list's own rows (a stored name is matched, never built into markup) and hidden while something is typed; no match says so. Test: pages::jump::sections_tests::the_palette_lists_topics_groups_and_pages_each_under_a_heading_and_a_section_with_no_name_has_none (removed in R5); in Chrome `palette-sections-and-recents`.

## CON-756. A truncated or abbreviated value, and an icon button, has a tooltip
The `title` of a middle-ellipsis value or of an exact time, and the `aria-label` of a button with no text, are shown by one shared card the way the Tooltip component shows its own (400 ms, at once on focus, Esc, leaving), with the `title` set aside while shown; the rail's names are the same card when the rail is narrow. Without a script the `title` is what it was. Test: in Chrome `tooltip-shared-for-title-and-icon-buttons`.

## CON-757. A page change is a swap of `main` in the same document, latest click wins
Every link and form of the console, across all the built pages, swaps `main` (the document, the top bar and the rail survive; the title, the rail and the history follow) in a view transition of about 180 ms; with `prefers-reduced-motion: reduce` no animation runs at all (the browser's default fade included); five quick clicks end on the last page with nothing stacked; a slower answer to an earlier click never lands after a later one; a click that comes while the swap is on its way waits for it; Back and Forward put each page's scroll back, instantly. Test: in Chrome `transitions-no-document-flash-across-pages`, `transitions-forms-swap-in-place`, `transitions-reduced-motion-runs-none`, `transitions-fast-clicks-do-not-stack`, `transitions-overtaken-click-never-lands-late`, `transitions-back-forward-restore-scroll`; `tools/dom/navigation.test.ts` (the scroll comes back with `behavior: "instant"`).

## CON-758. No page scrolls sideways at 1024 px
Below 1180 px the rail keeps its icons (the names are tooltips), the page's padding and a table cell's are tighter, and a key never wraps inside a table. Measured on 35 pages at 1024 by 768: horizontal overflow 0 (it was 122 px on Live, 74 on the Messages page, 42 on a topic's Wire, 29 on Partitions). Test: `bun tools/ux/shots.ts --sizes 1024x768` prints `overflow` for a page that does.

## CON-759. A filter field shows its focus once, on its box; headings balance and paragraphs avoid a widow
`.finput:focus-within` draws the ring and the input draws none; headings use `text-wrap: balance`, paragraphs `text-wrap: pretty`. Test: in Chrome the screenshots under `target/ux/shots/`; no automated check.

## CON-760. Every filter changes the data it filters
On the Live page the search box (asks again 250 ms after the last key), "All apps", "All topics", "Refused only" and "Idle polls hidden" change the rows; so do the Topics list's filter and toggle, the Groups states, the Messages partition and key, and the Config chips. The fixtures tape used to ignore the query (no filter did anything); it now passes each row through the recorder's own rules (`fixtures_live::passes`), the stream's fragment keeps the filter, Back brings the earlier rows back. A control that looks right and changes nothing is the defect this rule is for. Test: `tests/ux_filters::every_filter_of_the_live_page_changes_the_rows`, `tests/live_wire::every_filter_of_the_live_page_changes_the_rows_of_a_real_tape`; in Chrome `filters-live-change-the-tape` (rows 16 to 3 for a typed client, 16 to 4 for a client or a topic, 16 to 2 for refused only, a different set with idle polls shown) and `filters-lists-change-the-data`; the recorder's own rule is `recorder::tests::filters_by_client_api_topic_group_and_refusal`.

## CON-761. An overlay fits the window and scrolls inside itself
A dialog is a flex column at most 100dvh less 48 px tall: the title is fixed, the body scrolls (`overflow-y: auto`, `overscroll-behavior: contain`), the foot with the buttons stays in view; a drawer is as tall as the window on the right edge and scrolls inside itself; the page behind never scrolls. Measured for 19 overlays (produce, delete, add partitions, delete records, config edit and reset, reset offsets, delete group, offset delete, member, story, record, request and clients drawers) at 1280 by 720, 1024 by 640 and 800 by 600: every panel inside the window, every foot in view with its buttons, every body that is longer than the window scrolls. Test: in Chrome `overlays-fit-and-scroll-at-three-sizes`.

## CON-762. A list opened from a Select or a Combobox fits its options
The list is at least as wide as its trigger and as wide as its longest option up to 420 px (and never wider than the window less 32 px), each option is one line with an ellipsis and its label in `title`; no word is broken in the middle. Test: in Chrome `select-menu-fits-its-labels`.

## CON-763. One focus system
A focused control shows exactly one ring: 2 px, the primary colour at 45 %, 2 px off the outermost shape, following its radius (forced-colors: `Highlight`). A box around an input (the top bar's search, a filter field, a form field, a Combobox) draws it on the box and its input draws none; a control that is its own shape draws it on itself. Test: in Chrome `focus-one-ring-on-the-outer-shape` (19 kinds of control, one ring each, the screenshots `target/ux/focus-*.png`).

## CON-764. Home is a strip of figures, two balanced columns and the way on
A strip of six figures (running, partitions open, topics, memory, threads, profile), then the Address card (with the advertised-listener check) and "This instance" in two columns of 7 to 5 (the second never under 380 px; one column under 1180 px), then the call to action across the page. Nothing overflows or is clipped at 1024, 1280, 1440 and 1920 px, and no other built page overflows at 1024. Test: in Chrome `home-fits-at-four-widths` and `every-page-fits-at-1024` (27 pages).

## Live updates: one design (CON-800 to CON-812)

`docs/port/console/live-design.md` is the design, the measured problem and the per-page table. Browser proofs (not in CI, they need Chrome and a real broker): `tools/ux/scenarios/live.ts` (`stream-rate`, `refresh-interval`, `text-stable`, `tail-live`, `tail-reconnect`, `table-stable`, `live-above-the-fold`); DOM tests: `tools/dom/live.test.ts`.

## CON-800. Every page is a snapshot (retired in R5)
**Retired in slice R5 (2026-11-13)** as written: its tests drove the server-rendered pages and were removed with them. The design rule itself (`docs/port/console/live-design.md`) carries over to the React console, which keeps it with its own components and Playwright scenarios (react-plan.md, CON-1000 and up).
No page opens an `EventSource`, sets a timer, polls or listens for a tick until a person asks: Home, Topics, a topic, Groups, Lag, Log, Producers, Story, Live, Config, Settings, Access and Connect make no request and see no DOM mutation after they load. Test: ui::shell::tests::no_page_opens_a_stream_and_a_page_that_lists_regions_has_the_refresh_control_in_the_breadcrumb_row (removed in R5), `tests/live_topics_more::a_fragment_is_what_the_page_holds_and_an_unchanged_one_is_a_304`, `tests/boards_live::the_page_is_the_same_page_in_every_state_and_the_states_are_slots`.

## CON-801. The recorder wakes nobody
No shared tick, no 2 s floor timer, no per-request bump of page subscribers. Test: `recorder::tests::since_returns_what_changed_after_a_cursor_including_a_fold`, `tests/stream::a_tail_that_goes_away_gives_its_place_back`.

## CON-802. The Refresh control (retired in R5)
**Retired in slice R5 (2026-11-13)** as written: its tests drove the server-rendered pages and were removed with them. The design rule itself (`docs/port/console/live-design.md`) carries over to the React console, which keeps it with its own components and Playwright scenarios (react-plan.md, CON-1000 and up).
At the right of the breadcrumb row of a data page: a Refresh button (key `r`), a stamp `Updated hh:mm:ss` (UTC, the render time, never "s ago") and an interval picker Off, 5 s, 10 s, 30 s, 1 min (default Off, least 5 s, remembered in `localStorage`, one preference for every page). A page that lists no regions has no control. Test: ui::refresh::tests::the_control_names_its_regions_stamps_the_render_time_and_defaults_to_off (removed in R5), ui::shell::tests::no_page_opens_a_stream_and_a_page_that_lists_regions_has_the_refresh_control_in_the_breadcrumb_row (removed in R5).

## CON-803. A refresh is conditional and morphs only what changed (retired in R5)
**Retired in slice R5 (2026-11-13)** as written: its tests drove the server-rendered pages and were removed with them. The design rule itself (`docs/port/console/live-design.md`) carries over to the React console, which keeps it with its own components and Playwright scenarios (react-plan.md, CON-1000 and up).
A refresh fetches the page's regions (`[data-frag]`) with `If-None-Match`; an unchanged region answers 304 and nothing happens; a changed one is morphed in place by id (the header of a table, `data-morph-skip`, never). Nothing fades. Test: regions::tests::a_page_lists_each_region_once (removed in R5), `tests/home_live::creating_a_topic_flips_title_sentence_and_cards_in_the_one_fragment_without_a_reload`.

## CON-804. A poll waits its turn (retired in R5)
**Retired in slice R5 (2026-11-13)** as written: its tests drove the server-rendered pages and were removed with them. The design rule itself (`docs/port/console/live-design.md`) carries over to the React console, which keeps it with its own components and Playwright scenarios (react-plan.md, CON-1000 and up).
Polls are chained (the next is scheduled when the last has finished), stop while the tab is hidden, are skipped while a field of the page has focus or a menu or overlay is open, and a failed poll says `will retry at hh:mm:ss` and leaves the page as it was. Test: ui::refresh::tests::the_control_names_its_regions_stamps_the_render_time_and_defaults_to_off (removed in R5).

## CON-805. Prose never holds a live value
A fact that changes is a metric slot (`ui::metrics`: a label, a figure of reserved width in tabular figures, a unit, a static tooltip) or a table cell; a sentence is static text. Test: ui::metrics::tests::a_stat_has_a_label_a_figure_of_declared_width_and_the_full_value_on_hover (removed in R5), `words::tests::the_steady_sentence_is_static_and_holds_no_number`, `tests/boards_topics::the_topic_page_has_the_open_transaction_the_toolbar_and_the_ten_newest`.

## CON-806. Times are absolute (retired in R5)
**Retired in slice R5 (2026-11-13)** as written: its tests drove the server-rendered pages and were removed with them. The design rule itself (`docs/port/console/live-design.md`) carries over to the React console, which keeps it with its own components and Playwright scenarios (react-plan.md, CON-1000 and up).
A time in a page is the clock or the date in UTC (`format::stamp`); how long ago is a hover the script works out when the pointer is on it, so the text of a region does not change while the data does not. Test: format::tests::an_instant_is_written_in_absolute_terms (removed in R5), ui::metrics::tests::an_instant_is_absolute_and_the_relative_form_is_not_in_the_text (removed in R5), `tests/boards_topics::the_list_has_the_boards_nine_topics_and_the_hidden_internal_one`.

## CON-807. A layout never depends on a value
A conditional block has a region of reserved height, calm when it has nothing to say (the transaction line of a topic, the rebalancing line of Groups, the refusals of Live); the Live page has the same parts in the same order in every state. Test: `tests/boards_live::the_page_is_the_same_page_in_every_state_and_the_states_are_slots`, `tests/live_topics::every_route_answers_as_the_console_and_has_the_boards_parts`.

## CON-808. Tables are DataTables (retired in R5)
**Retired in slice R5 (2026-11-13)** as written: its tests drove the server-rendered pages and were removed with them. The design rule itself (`docs/port/console/live-design.md`) carries over to the React console, which keeps it with its own components and Playwright scenarios (react-plan.md, CON-1000 and up).
`table-layout: fixed` with a `<colgroup>` the page declares, one line a cell with the full text in `title`, numbers right-aligned, a row height fixed by the density (comfortable 44 px, compact 36 px, roomy 56 px), a stable id on every row, a header outside every refresh, a reserved empty state. Test: ui::datatable::tests::the_page_declares_the_widths_and_the_table_never_takes_them_from_its_rows (removed in R5), ui::datatable::tests::an_empty_table_keeps_its_header_and_says_so_in_a_reserved_block (removed in R5), ui::datatable::tests::a_row_id_is_safe_for_any_key (removed in R5).

## CON-809. The only streaming views are tails, and a person turns them on
The Live page and a topic's Messages page offer `Snapshot | Live tail` (default Snapshot) and a speed (Realtime 1 s, Relaxed 3 s, Slow 10 s, Paused; default Relaxed, remembered). Rows arrive in batches no faster than the speed, are put at the top, are held behind a `N new` pill when the reader has scrolled, hovers, selects or has focus in the table, and never change a row that is there. Test: ui::tail::tests::the_control_starts_as_a_snapshot_and_offers_the_tail_only_to_a_script (removed in R5), `stream::tests::the_pace_is_what_was_asked_within_the_caps`, `tests/stream::a_tail_opens_at_once_with_ready_and_sends_nothing_while_nothing_is_added`.

## CON-810. A tail reconnects from where it was
The indicator says Live, Reconnecting…, `Disconnected · Retry in N s` (waits of 1, 2, 4, 8, 15, 30 s with jitter) or Snapshot; a reconnect resumes from the last event id, so no row comes twice. Test: `tests/live_tail::rows_arrive_as_html_after_the_cursor_once_and_a_reconnect_brings_only_the_newer`, `tests/live_tail::the_filters_of_the_page_are_the_filters_of_the_stream`.

## CON-811. Nothing fades (retired in R5)
**Retired in slice R5 (2026-11-13)** as written: its tests drove the server-rendered pages and were removed with them. The design rule itself (`docs/port/console/live-design.md`) carries over to the React console, which keeps it with its own components and Playwright scenarios (react-plan.md, CON-1000 and up).
No opacity animation of an update, ever; the only motion is the background of a new row, for 1.5 s. Test: ui::client::tests::the_script_follows_the_rules_the_spike_found_and_runs_under_the_strictest_policy (removed in R5).

## CON-812. A deadline is a slot
A countdown (a transaction's timeout, the time left to rejoin) is an absolute time and an element the script counts down by itself, with no request. Test: `tests/boards::the_attention_home_carries_the_boards_transaction`, `tests/boards_groups::the_list_has_the_boards_four_groups_in_order_with_their_lag_words`.
# The multi-select pass (CON-850 to CON-857)

Branch `ux-multiselect`. The owner, on the real Live page: "The topics filter does not seem to work. It should be a multi select." The filter did narrow the rows; it did not look as if it had, and it could not take two values. Code: ui/components/multiselect.rs (removed in R5), `assets/multiselect.js`, pages/live/filters.rs (removed in R5), `recorder/view.rs`, `model/live.rs`, `model/group_states.rs`, `model/query.rs`; Chrome scenarios `tools/ux/scenarios/multiselect.ts`.

## CON-850. A topic filter keeps exactly the topics asked for, and every row says which one it passed by
The match was a substring (`orders` kept every row of `orders.dlq`), and a request that names several topics or a group's commit passed while its Target cell named the first topic or the group (`inventory.levels and 5 more`, `fulfilment generation 1`), so half the visible rows did not mention the chosen topic. A topic is now matched by its whole name; a row that passes puts the topic it passed by in the Target cell (the group, or the other topics, move to the line under it) and its cell links to that topic. Free text (`q=`) is still a substring match. Test: `a_topic_filter_keeps_that_topics_rows_exactly_and_each_row_says_the_topic` (a real broker, real requests over the Kafka listener, through the recorder), `a_kind_matches_any_of_its_values_and_a_topic_matches_exactly`.

## CON-851. The MultiSelect (retired in R5)
**Retired in slice R5 (2026-11-13)** as written: its tests drove the server-rendered pages and were removed with them. The design rule itself (`docs/port/console/live-design.md`) carries over to the React console, which keeps it with its own components and Playwright scenarios (react-plan.md, CON-1000 and up).
A button that says the selection compactly (`All topics`; `orders`; `orders, payments.settled`; `3 topics`), a clear x, and a popup with a search field (the chosen as removable tags in it), "Select all" and "Clear" with the count chosen, and a scrollable listbox (`aria-multiselectable`, each option `aria-selected`, a check, its row count, an ellipsis and a `title` for a long name). As wide as its longest option up to 420 px, at least 240 px; the chosen first when it opens; flips near the window's edge and stays right of the rail. Keys: Enter, Space and the arrows open it; arrows, Home and End move; Space (empty search) and Enter toggle; typing filters; Backspace in the empty search removes the last chosen; Esc closes and returns focus to the button. Test: a_multiselect_is_checkboxes_in_a_details_underneath_and_a_multiselectable_listbox_on_top (removed in R5), the_button_says_the_selection_compactly (removed in R5), a_multiselect_lists_each_value_once_keeps_a_chosen_value_the_tape_lacks_and_shows_counts (removed in R5), hostile_multiselect_labels_and_values_are_text (removed in R5), the_multiselect_script_defines_its_component_and_every_method_the_markup_binds (removed in R5); `tools/dom/multiselect.test.ts`; in Chrome `multiselect-keyboard`, `multiselect-flips-near-the-bottom-and-clears-the-rail`, `multiselect-long-labels-ellipsis-and-width`.

## CON-852. Repeated parameters: any within a kind, all across kinds
`?client=a&client=b&topic=orders&api=Fetch`: a row passes a kind when it matches any of its values, and a row is shown when it passes every kind. A kind keeps at most 32 values of 128 characters, without repeats; an `api` that is no API of the broker is dropped; a hostile address is a page with no filter. A cell's "Show only X" still replaces the kind with that one value. Test: `a_repeated_field_is_several_values_in_the_address_and_back`, `the_values_of_a_kind_are_bounded`, every_filter_of_the_live_page_changes_the_rows_of_a_real_tape (removed in R5), `a_topic_filter_keeps_that_topics_rows_exactly_and_each_row_says_the_topic`.

## CON-853. Each option counts the rows it would leave
The count beside an app, an API or a topic is the number of rows of the tape (a folded run is one row) that pass the other filters, so choosing it leaves that many; a value that is chosen but no longer on the tape stays an option with no count. Test: `each_option_counts_the_rows_it_would_leave_among_the_other_filters`.

## CON-854. Two or more chosen values are tags under the toolbar, and an empty tape says which filters are on (retired in R5)
**Retired in slice R5 (2026-11-13)** as written: its tests drove the server-rendered pages and were removed with them. The design rule itself (`docs/port/console/live-design.md`) carries over to the React console, which keeps it with its own components and Playwright scenarios (react-plan.md, CON-1000 and up).
Each tag is an in-place link that takes its value off; "Clear filters" resets every filter. A tape that is empty because of the filters says "No request matches these filters", lists them in words (`app is x; topic is a or b`) and has the same way out. Test: the_empty_tape_names_the_filters_and_two_values_make_tags (removed in R5); in Chrome `multiselect-live-persists-in-the-address-and-back-restores`, `multiselect-clear-focus-and-empty-state`.

## CON-855. A choice reaches the server once, and the address restores it
Ticking sends the form once, 450 ms after the last change, or at once when the popup closes; the popup is left alone by the swap that answers (a component that is open is not morphed), the scroll and the toolbar do not move, the address carries every value, Back restores the earlier selection and a reload opens with it. Test: `tools/dom/multiselect.test.ts` ("ticking three options asks the server once"); in Chrome `multiselect-live-persists-in-the-address-and-back-restores`.

## CON-856. Without a script it is checkboxes in a details
The control is a `<details>` of real checkboxes named after the field, with an Apply button; the script hides it (`visibility`) and keeps it as the state, so the form sends the same either way. Test: in Chrome `multiselect-without-a-script-is-checkboxes-in-a-details`, `multiselect-repeated-parameters-in-the-gallery-form`.

## CON-857. The Groups states and the Messages partitions take several values
`/groups?state=stable&state=empty` lists the groups in either state (a MultiSelect with the count of each, instead of the Segmented control); `/topics/t/messages?partition=0&partition=2` reads those partitions (a topic with few partitions keeps the Segmented control and gains "More", a MultiSelect; one with many has the MultiSelect alone; a chip for each partition takes it off; an offset still needs exactly one). Test: groups_states_and_messages_partitions_take_several_values (removed in R5), `several_partitions_are_a_repeated_field_ascending_once_each_and_bounded`, `states_are_a_repeated_field_and_a_group_shows_in_any_of_them`. Supersedes the Segmented states and partitions of CON-753.

## CON-930. A rule is described by a sentence made once from the rule
`+400 ms on 1 in 4 Produce requests to orders.`, `Answers 1 in 5 Fetch requests to inventory.levels with REQUEST_TIMED_OUT.`: the board's words, with no number that follows the broker, so a sentence never changes while a rule runs. Names come from the rule (`Slow produce to orders`, `Fail inventory.levels fetches`). Test: `a_sentence_is_the_boards_and_holds_no_number_that_follows_the_broker`, `a_drop_says_whose_connections_and_when`.

## CON-931. A Lab form says what to fix, field by field, and previews before it starts
The Slow and Fail forms open with the boards' defaults (a link with half the fields opens a whole form); a field that is wrong gets its own sentence; the preview is the rule the form makes, the sentence, and what the wire would show, worked out without making anything; text is cut to a length no rule needs. Test: `the_forms_open_with_the_boards_defaults_and_make_the_boards_rules`, `each_wrong_field_gets_its_own_sentence`, `a_link_with_half_the_fields_opens_a_whole_form`, `the_preview_is_the_rule_and_what_the_wire_shows`, `text_is_cut_to_a_length_no_rule_needs`.

## CON-932. The engine's refusal comes back inside the form, in words
A rule the engine will not take (the 51st) returns the form with the reason, and the preview; a replaced rule does not count twice. Test: `a_rule_that_is_refused_by_the_engine_comes_back_in_its_form_with_the_reason`, `a_replaced_rule_keeps_the_count`, `every_refusal_of_the_engine_has_a_sentence`.

## CON-933. The Lab page is the table's rules, the ones that ended in the last hour, and the rehearsals kept (200 at most)
A rule in force and one that is over are the same view with and without an end; the rehearsals that happen once (expire, rebalance, clean) leave a line, refused ones too. Clear history forgets both. Test: `a_rule_in_force_and_one_over_are_the_same_view_with_and_without_an_end`, `a_history_of_rehearsals_is_bounded`, `an_expired_member_and_a_forced_rebalance_go_through_the_groups_own_machine`.

## CON-934. Drop is a rule that ends by itself
"Once" is a rule limited to the connections open now and as many hits as there are of them, for a minute; "keep dropping" is a rule by client for 1 or 5 minutes. An app with no connection open, or a pattern in place of a name, is refused in words. Test: `drop_once_needs_an_open_connection_and_a_plain_name`, `drop_once_closes_the_connections_that_were_open_and_not_the_next_one`.

## CON-935. A member choice survives any name
The expire dialog's choices carry the group and the member in one value that round-trips hostile names. Test: `a_member_choice_round_trips_hostile_names`.

## CON-936. A rule's page lists the requests it touched, from the wire
The view-model of a rule carries the newest rows the recorder holds for it (each tagged with the rule, its name and what it did), and says when older ones may have left the ring. Test: `a_slow_rule_delays_a_fetch_and_the_wire_says_so`.

## CON-937. Compact now shows the planner's verdict and refuses with a sentence
The dialog says whether a round is due (`61 % dirty against 50 %`), what it would rewrite and where the checkpoint would move; it says why not for a topic that does not compact, a partition nothing was written to, a cleaner that is off, or nothing due. Test: `compact_now_runs_the_real_round_and_refuses_with_a_sentence_otherwise`.

## CON-938. The wire names the errors the Lab makes up
REQUEST_TIMED_OUT (7) and NOT_LEADER_OR_FOLLOWER (6) have rows of the error table, so a failed row says `REQUEST_TIMED_OUT (7)` and not `ERROR_7`. Test: `a_rows_name_and_code_are_the_protocol_crates_own`.

## CON-939. The story names a forced rebalance
A rebalance forced from the Lab is the cause "Forced from the Lab", for both protocols. Test: `a_forced_rebalance_makes_the_members_join_again_and_says_it_was_forced`.

# The React console, slice R0 (rules 1000 to 1014; 1015 to 1049 kept for R1 to R5)

Branch `react-r0`, the plan in `docs/port/console/react-plan.md`. The page layer is React + shadcn/ui in `web/`, built by bun and Vite into `crates/kafka-console/web-dist/` (committed) and embedded in the binary: one process. The JSON API is `api.rs`, the bundle is `web.rs`, the router that joins them and the classic console is `app::console`. The browser gate is Playwright in `web/e2e` (Firefox, WebKit, Chromium), run locally against the release binary; its scenarios are named below in words. Numbers from 1015 up to 1049 are kept for the slices R1 to R5.

## CON-1000. The React console answers every page address with index.html
Every GET under the console's root that is not `/api/`, `/assets/` or a file of the bundle answers `index.html` (`no-cache`), so a deep link (`/topics/orders`, `/gallery?record=r7`) opens that page. Test: `every_page_address_answers_index_html_so_a_deep_link_works`; Playwright "a deep link opens the page as it was".

## CON-1001. The bundle is embedded gzipped and served from memory
Text files are the `.gz` that `pack` of `web/scripts/build.ts` makes, held as `files::Served` and answered by `files::packed`: gzip as it is to a client that accepts it, inflated for another, ETag and 304; hashed files are `immutable` for a year. Test: `a_hashed_file_is_gzip_to_a_client_that_accepts_it_and_cached_for_a_year`, `every_text_file_of_the_bundle_inflates_to_its_source`.

## CON-1002. A stale bundle fails the tests
`web-dist/source-hash.txt` is the FNV-1a hash of the `web/` sources the bundle was built from; the test computes it the same way. CI rebuilds the bundle and fails on any difference. Test: `the_embedded_bundle_was_built_from_the_current_web_sources`.

## CON-1003. No inline script, no evaluated string
`index.html` loads its script by `src` only and no script file calls `eval(` or `new Function(`; zod (in the gallery chunk only) runs jitless, so the CSP never refuses anything (the browser gate fails on the console error a refusal logs). Test: `the_bundle_has_no_inline_script_and_evaluates_no_string`.

## CON-1004. index.html follows the prefix
At start the two relative references of `index.html` are made absolute under `--console-prefix` and a `<meta name="doze-base">` names the root for the client's router and fetches; a hostile prefix is escaped. Test: `index_html_is_rewritten_for_the_root_and_the_prefix`, `the_built_index_points_at_files_the_bundle_has`, `under_a_prefix_everything_moves_with_it`.

## CON-1005. The API answers only the console's own fetch
Every `/api/v1/` request must carry `X-Console: 1` (a page on another site cannot send it without a CORS preflight, which is never granted) and no `Sec-Fetch-Site` other than `same-origin`; a refusal is `{code, message}` JSON, 403. Test: `the_api_answers_json_only_to_the_consoles_own_fetch`.

## CON-1006. A change through the API needs the same-origin proof as well
A POST passes `origin::check` (Origin equal to Host, or `Sec-Fetch-Site: same-origin`); a GET of a POST route is 405 and an unknown `/api/` address is a JSON 404; a POST to a page address is 405. Test: `a_change_through_the_api_needs_the_same_origin_proof_too`.

## CON-1007. Home, Connect, the shell and the palette are the view-models, serialised
`/api/v1/home` (the HomeView and the words Rust decides), `/api/v1/connect`, `/api/v1/shell` (address, profile, security chip, counts; the classic console's address until R5), `/api/v1/jump?q=` (topics and groups containing the text, any case, at most 200 of each), `POST /api/v1/probe`. Test: `the_api_says_what_home_connect_the_shell_and_the_palette_need`.

## CON-1008. The TypeScript types are generated from the Rust types
`ts-rs` (a dev-dependency) writes `web/src/api/types/*.ts`; a stale or an extra file fails the test. Test: `the_typescript_types_of_the_api_are_current`.

## CON-1009. No memory figure anywhere
The owner (2026-11-13): "don't show memory". `InstanceView::resident_bytes` is never serialised and Home shows no memory tile. Test: `a_home_answer_carries_no_memory_figure`; Playwright "Home: one h1, the figures as stat tiles, no memory figure anywhere".

## CON-1010. An address the app has no page for is its Not here
There is one page layer, the React app, and no other route (the server-rendered console and its `/classic/` mount were removed in slice R5). Any GET under the console root that is not `/api/`, `/assets/`, `/healthz` or a file of the bundle answers `index.html`, whose router shows Not here for an address it has no page for; no file outside the bundle is served, and a POST to a page address is 405. Test: `an_address_the_app_has_no_page_for_is_its_not_found_and_no_old_file_is_served`; Playwright "a deep link opens the page as it was, and an address with no page is Not here".

## CON-1011. The host guard, the headers and the limits stand in front of all of it
The CSP is `default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'`. Test: `the_host_guard_and_the_security_headers_stand_in_front_of_all_of_it`.

## CON-1012. No planted secret reaches a JSON answer
The planted-secret gate reads, over a real broker with SASL and a Lab rule in force: every GET endpoint of the API (`GET_PATHS`),
the GETs of one group (`GROUP_GET_PATHS`, over groups the test commits), the Lab's GETs (the home, both forms, a rule, the
rehearsals, Compact), the answer of a representative POST of every write (made, checked and refused: probe, create topic, produce,
add partitions, delete records, topic config edit and reset, compact, the three group writes, settings edit, ACL create and
delete, the Lab's rule writes, rehearsal, compact and clear, delete topic), and the first frames of the live tail from the first
cursor; the messages tail, which shows records by design, is searched for the credentials and the secret config. Added by the
2026-11-13 console security audit (finding 4); it found the Settings dialog saying back the value typed for a config that is not
one of the five (`ssl.key.password`) in the call it shows: such a value is now `(the value, not shown)`, in the call and in the
broker's sentence. Test: `no_planted_secret_reaches_the_recorder_the_stream_or_a_page`.

## CON-1013. A hostile name is JSON text
React escapes text and no HTML is ever injected; the API returns what it was given as a string. Test: `a_hostile_name_in_the_palette_comes_back_as_json_text`.

## CON-1014. Every control works in Firefox, WebKit and Chromium
The Playwright gate drives the shell (sidebar, collapse, chips, palette, RefreshControl), the gallery (Button, Input, Textarea, Select, Combobox, MultiSelect, RadioGroup, Switch, Checkbox, ToggleGroup, Tabs, Dialog, Sheet, DropdownMenu, Popover, Tooltip, Toast, StatStrip, CodeValue, EmptyState, DataTable), Home and Connect, with the mouse and the keyboard, asserting outcomes; every test fails on a console error; no page scrolls sideways at 1024, 1280, 1440 and 1920; an interval refresh moves nothing. Files: `web/e2e/tests/shell.spec.ts`, `gallery.spec.ts`, `pages.spec.ts`. Results per browser in `docs/port/console/react-plan.md` ("R0 done").

# The React console, slice R1b: topic writes and the Config tab (rules 1150 to 1199)

Branch `react-r1b`. The endpoints are `crates/kafka-console/src/api/topic_writes.rs` and `api/config.rs`; the pages are `web/src/pages/topics/create.tsx`, `writes/` (the overlays) and `config.tsx` (+ `config-dialogs.tsx`). Every write goes through the same `Source` call as the classic form; the words the dialogs say (teaching, button labels, the preview of Delete records, who uses a topic) are Rust's (`write_words.rs`, moved out of the page layer so that R5 can remove it). A write always answers 200 with an `outcome` (as R2's group writes, so no browser logs a failed request): `done` and what was made, `refused` with `{code, message, detail}` when Kafka refused (its error name and the broker's sentence, `detail` the state the page draws), `invalid` with `{code: "INVALID", message, detail}` when the form could not be sent; 404 when there is no such topic. The browser specs are `web/e2e/tests/topic-writes.spec.ts` and `topic-config.spec.ts`.

## CON-1150. Create topic is a page of three steps with the server's checks as you type
`/topics/new`: Name and partitions, Configuration, Review (shadcn Tabs as a stepper, the step in the address, Back returns to the step before with what was typed). zod says at once what it can; the form view (`GET /api/v1/create-topic`) says a name that is taken or collides with the broker's own sentence; once nothing is wrong the same CreateTopics with `validate_only` is asked and its answer shown. Check only makes nothing; Create makes the topic and the page says what was made. Test: `create_topic_checks_makes_and_is_refused_in_kafkas_words`; Playwright "three steps, the server's live check, Check only, then Create", "a name Kafka forbids, a name that is taken or collides".

## CON-1151. A refused create is Kafka's code and sentence, and the way on
A name taken between the form and the call is TOPIC_ALREADY_EXISTS (36) with the broker's message, "Open the existing topic", "Choose another name", the other refusals of the call and the answer. Test: `create_topic_checks_makes_and_is_refused_in_kafkas_words`; Playwright "a topic made while the form is open".

## CON-1152. Produce is a dialog over the topic that answers in place
`?dialog=produce` on a topic opens it (Esc and Back close it, a deep link opens it): partition (auto or one), key, value with a JSON hint, header rows added and removed (at most 8), acks as a segmented control, the teaching folded so the form fits, the call as it will be sent (the server's, for what is typed). Sent, the dialog says the partition and the offset the log chose, whether read_committed sees it, and the answer. Test: `produce_writes_one_record_and_a_keyless_one_to_a_compacted_topic_is_refused`; Playwright "a keyed record with a header to a chosen partition", "a keyless record is accepted on a delete topic".

## CON-1153. A refused produce is Kafka's words in the dialog
A keyless record to a compacted topic is INVALID_RECORD (87) and the key field says "A compacted topic needs a key on every record."; a record over `max.message.bytes` is MESSAGE_TOO_LARGE (10); the answer is drawn. Test: `produce_writes_one_record_and_a_keyless_one_to_a_compacted_topic_is_refused`; Playwright "a keyless record to a compacted topic", "a record over max.message.bytes".

## CON-1154. Add partitions previews what it adds and is refused in Kafka's words
The field starts at one more than now; the unit says what the count adds (`adds 2: t-1 and t-2`) and the button counts it; fewer than now is INVALID_PARTITIONS (37) in the dialog. Test: `add_partitions_delete_records_and_delete_topic`; Playwright "the new total, what it adds, then the broker's count".

## CON-1155. Delete records previews what goes and who is below it
A partition Select and the offset; the preview (Rust's words) says how many records go, which offsets, and each group with a commit on the partition; the button counts it; after, the log start is where it was asked. A topic that only compacts is POLICY_VIOLATION (44). Test: `add_partitions_delete_records_and_delete_topic`; Playwright "the preview says what goes", "on a topic that only compacts".

## CON-1156. Delete topic needs the name typed, says who uses it, and leaves for the list
The button stays shut until the name is typed; groups with commits and open transactions are named; after, the console goes to Topics and says what was deleted. Test: `add_partitions_delete_records_and_delete_topic`; Playwright "type the name to confirm", "a topic a group reads says it is in use".

## CON-1157. After a write the pages show the new state without a reload
Every query but the form previews is marked stale after a write, so the page under the overlay refetches at once (the Config tab shows a saved value under the closing dialog). Playwright "edit a value: Check asks Kafka and changes nothing, Save changes it".

## CON-1158. The Config tab: groups, the filter and the chips in the address, panels that open in place
`/topics/$topic/config`: the changed configs first, then the groups (replication folded); `?q=` and `?show=changed|compaction|segments` (Back restores them); each row has its value and gloss, a source badge, Kafka's first sentence and when a change counts; "Where from" and "Kafka's text" are Collapsibles (no navigation, the scroll stays); Edit and Reset from a DropdownMenu on a value the topic sets. Test: `the_config_tab_edits_checks_saves_appends_and_resets`; Playwright "the chips and the filter are in the address", "Where from and Kafka's text open in place".

## CON-1159. The edit dialog checks and saves, in its three shapes, and answers in place
`?edit=<key>`: one value, a list (append, subtract, set), the two compaction lags together; Check sends `validate_only` and says what the config would be; Save says before and after and when the broker reads it; a refusal is Kafka's code and sentence with what would pass and "Edit again"; `?reset=<key>` deletes the topic's value. Test: `the_config_tab_edits_checks_saves_appends_and_resets`; Playwright "a list: append a policy", "the two lags together", "a value Kafka will not take", "reset: the topic follows the broker again".

## CON-1160. A write through the API without the same-origin proof changes nothing
Every R1b POST is behind the API guard: an Origin of another site is 403 `FORBIDDEN` and nothing is sent to the broker. Test: `a_write_without_the_same_origin_proof_is_refused`.

# The React console, slice R2: Groups (rules 1200 to 1249)

Branch `react-r2`. The pages are `web/src/pages/groups/` (the list, a group's layout with its tabs as routes, Members, Lag, Story, Wire, the dialogs), the API is `crates/kafka-console/src/api/groups.rs`, the browser gate is `web/e2e/tests/groups.spec.ts` (Firefox, WebKit, Chromium; seeded world and a broker with traffic). The data and every word of a refusal are the classic console's (`gather::groups`, `gather::group`, `gather::group_write`, `gather::story`, `groups_words`).

## CON-1200. The Groups API is the classic view-models, serialised
`GET /api/v1/groups?q=&state=` (`GroupsView`, the states on, the rebalancing line as Rust's spans), `/api/v1/groups/{group}?expiring=1` (`GroupLagView`), `/story` (`StoryView`); the requests that touched the group are the Live slice's `/api/v1/live/group-wire` and its `WireDock`, mounted on the Members and Wire tabs. A group the broker does not know is a JSON 404 `GROUP_NOT_FOUND` naming it. Test: `the_groups_api_says_what_the_list_a_group_and_its_story_show`, `every_get_of_one_group_is_json_that_no_cache_keeps_and_carries_no_memory_figure`.

## CON-1201. A reset is previewed with a GET and sends exactly what the preview showed
`GET /api/v1/groups/{group}/reset?topic=&to=&value=` reads OffsetFetch and ListOffsets and writes nothing; a value that is not a number is said in the preview (`The offset must be a whole number.`). `POST .../reset` takes the same choice as JSON; the dialog's Reset is off until the preview on screen is the choice made. Test: `a_reset_is_previewed_with_a_get_and_refused_in_kafkas_words_while_the_group_has_members`; Playwright "Reset offsets: a GET preview, then the reset; the Lag tab shows where the group now reads".

## CON-1202. A refused group write answers Kafka's name, number and words, and the console's reading
A 200 whose `outcome` is `refused` (a refusal is an answer, so no page logs a failed request; `done` and `invalid` are the other outcomes), with `code`, `message`, `number` and `refused`: `code` is Kafka's error name (`NON_EMPTY_GROUP`), `message` Kafka's own sentence (`The group is not empty.`), `refused` the rule, what would pass and the call refused. The dialog stays open and shows them. Test: `deleting_a_group_with_members_is_refused_as_non_empty_group`, `deleting_offsets_needs_a_topic_and_is_refused_while_a_member_reads_it`; Playwright "Delete group with a member is refused as NON_EMPTY_GROUP in Kafka's words", "Reset offsets of a group with a member is refused, in Kafka's name and words, and nothing changes".

## CON-1203. A group write needs the same-origin proof and a JSON body
A POST from another origin is 403 before anything reaches the broker; a body that is not JSON is 400 `BAD_REQUEST`; Delete offsets with no topic is the outcome `invalid` (the form would otherwise tick every topic). Test: `a_group_write_needs_the_same_origin_proof`, `deleting_offsets_needs_a_topic_and_is_refused_while_a_member_reads_it`.

## CON-1204. Filters and overlays of the Groups pages are in the address
The list's name filter (`q`) and states (`state`, a MultiSelect with the count of each), the Lag tab's `expiring=1`; a member's Sheet (`member=`), a rebalance's Sheet (`generation=`), the flapping rule's Sheet (`rule=1`) and the write dialogs (`dialog=reset|delete|delete-offsets`). An overlay opened on the page pushes an entry (Back closes it), a deep link opens the page with the overlay on it, Esc closes it and the focus returns to what opened it. Parsers: `web/src/pages/groups/search.ts` (bun tests in `words.test.ts`). Playwright "A member opens as a Sheet with its own address: Esc closes it, the focus returns, Back and a deep link work", "Groups: the counts are tiles, the name filter and the states are in the address, and a row opens its group".

## CON-1205. A group's tabs are routes and the group's figures are tiles
`/groups/{group}` (Members), `/lag`, `/story`, `/wire`, each with the group's h1, pills, StatStrip (state, members, generation or epoch, lag, committed partitions, last commit) and switch-group chips (a Combobox past eight groups); Refresh fetches every query of the group and moves nothing. Playwright "A group: the state, the figures as tiles, the tabs are routes, and Refresh moves nothing".

## CON-1206. The state of a group chooses its Members layout
Classic stable or empty: members, reads with lag bars, who holds which partition, the group's facts. KIP-848: the epoch cards, target and assigned per member, where a group can be. Rebalancing: who it waits for with the rejoin deadline counted down in the browser (CON-812), what happens next, who rejoined and who left. Each with the group's last requests ("What just happened"). Playwright "A group: the state, the figures as tiles, the tabs are routes, and Refresh moves nothing".

## CON-1207. The Story counts its deadline down in its own slot
The Now card's rejoin deadline is the fetch time plus the remaining milliseconds, counted down each tenth of a second in a `<time>` element of reserved width, with no request; the slot is there whatever the group is doing, so the card never changes height. Playwright "Story: the generations newest first; a rebalance opens as a Sheet, the flapping rule as another".

## CON-1208. A time to reset to is chosen with the console's own picker, in UTC
A month grid and a time field in a Popover (`components/app/utc-datetime-picker.tsx`), never the browser's date widget; the value sent is milliseconds since the epoch. Playwright "Reset offsets: a GET preview, then the reset; the Lag tab shows where the group now reads".

## CON-1209. After a write the group's pages, the list and the sidebar's count are fetched again
Reset, Delete offsets and Delete group invalidate the group's queries, the list and the shell; a deleted group's page says "No such group". Playwright "Delete offsets forgets the commits of the topics ticked; Delete group then removes the empty group".

# The React console, slice R3: Live (rules 1250 to 1262; 1263 to 1299 kept)

Branch `react-r3`. The Live page, its tail, its Sheets, the first-request page and the wire docks in React (`web/src/pages/live/`), over the JSON API of `crates/kafka-console/src/api/live.rs`. The owner's complaint this slice answers: the classic Live page "flickering like crazy", "it should not be moving this fast". The rules of `docs/port/console/live-design.md` (CON-800 to CON-812) carry over; these are the React page's own. The browser scenarios are in `web/e2e/tests/live.spec.ts`, named below in words.

## CON-1250. The Live API is the classic tape's view-models, serialised
`GET /api/v1/live` (the LiveView of the filters, the words Rust decides: the empty tape's command, the phrases of the filters that are on, the state's name, and `tail_from`, the cursor a tail starts at), `/api/v1/live/request/{id}` (`found: null` and the words once the row fell off the tape: a 200, because a 404 is an error line in every browser's console), `/api/v1/live/clients`, `/api/v1/live/first` (`first: null` while no client spoke), `/api/v1/live/topic-wire?topic=`, `/api/v1/live/group-wire?group=&kind=&refused=1&q=`. The filters are the classic ones (`q`, repeated `client`, `api`, `topic`, `refused=1`, `idle=0`, `before`); a snapshot is at most 100 rows. Test: `the_live_api_says_the_tape_the_requests_the_clients_and_the_docks`, `the_snapshot_asks_for_at_most_a_hundred_rows`.

## CON-1251. A request's Sheet and the rules behind a refusal are decided in Rust
`RequestResponse` carries the kind (`detail`, `refused`, `fenced`, `too_large`, by `RequestKind::of`), the rules list made by the same functions the classic drawers use (`refused_rules`, `fenced_rules`, `too_large_rules` in `refusals.rs`), the topic's limits as the broker says them now, the commands that read a fetched record again and the keyed produce for an INVALID_RECORD on a compacted topic. Kafka's own words come from the broker's message or the error table, never from the page. Test: `the_live_api_says_the_tape_the_requests_the_clients_and_the_docks`.

## CON-1252. The tail is a JSON stream behind the API's proof, at the pace asked, never faster than once a second
`GET /api/v1/tail/live?<filters>&pace=<s>` answers `text/event-stream`: `ready` at once (`{cursor, reset, pace}`), then `rows` (`{cursor, rows}`, newest first, `id` the cursor) only when a look found rows added or a folded row grown. The pace is clamped to 1 to 30 s by the server whatever is asked. It needs `X-Console: 1` like every API request, so the page reads it over fetch (`web/src/lib/sse.ts`), not EventSource. A permit of `max_streams` is held while it is open. Test: `the_json_tail_sends_rows_after_the_cursor_once_and_a_reconnect_brings_only_the_newer`.

## CON-1253. A cursor names its process; a reconnect brings only the newer, a restart starts a new tape
A cursor is `<epoch>.<version>`. A reconnect sends the last one as `Last-Event-ID` and gets only what changed after it. A cursor of another epoch (the broker restarted: the recorder is empty and its ids start again) is answered `reset: true` and the rows of the new life; the page drops its tape first, so no row is shown twice and no id of the old life stands for a new request. Test: `a_cursor_of_another_life_names_no_version`, `the_json_tail_sends_rows_after_the_cursor_once_and_a_reconnect_brings_only_the_newer`; Playwright "the tail says Reconnecting, then Live, with no row twice and none of the old life" (kills and restarts its own broker).

## CON-1254. The tape merges by request id, bounded
A batch's new rows are prepended; a row already on the tape (a folded repeat that grew) is replaced where it is, under the same React key, so only its own cells change; the tape keeps 200 rows, the rows held aside 500 (one entry per id). Test: bun "the tape (CON-1254)" in `web/src/pages/live/live.test.ts`.

## CON-1255. Live opens as a snapshot and asks nothing more until a person does
Snapshot is the default mode (the `Snapshot | Live tail` ToggleGroup, the mode in the address as `tail=1`), the RefreshControl Off, the speed Relaxed (3 s; Realtime 1 s, Slow 10 s; remembered as `doze:tail-speed`). After load the page makes no request at all. Playwright "Live is a snapshot by default".

## CON-1256. A tail moves only the tape, no faster than the pace, and never rewrites a row that was there
Batches are applied at most once a pace (measured: every gap at least the pace); the header, the stat strip, the refusals strip, the toolbar, the tail bar and the table head keep their boxes over 20 s; no row that was on the tape is replaced or rewritten (a MutationObserver and node identity), apart from a folded repeat's count and time; new rows only get a 1.5 s background highlight (paint only, `.tape tr:has([data-fresh])`). The tape is a box of fixed height, so a batch never moves the page. Playwright "the tail: batches never faster than the pace, existing rows never rewritten, new rows highlighted, the page around it still".

## CON-1257. A tail holds what a person is reading
While the tape is scrolled, the page is scrolled past its top, the pointer is on it, text in it is selected, the focus is in it, or an overlay or a popup is open, a batch is held, not inserted: a `↑ N new · Show` pill counts the held rows, the scroll positions do not change, and Show applies them and returns to the top. Playwright "the tail holds while the tape is scrolled: the pill counts, the scroll stays, Show applies".

## CON-1258. The connection says what it is
Grey `Snapshot`, green `Live`, amber `Connecting…` and `Reconnecting…` (the first retry, after 1 s), red `Disconnected · Retry in N s` with a local countdown and `Retry now` (backoff 2, 4, 8, 15 s); a stream silent for 40 s (its keep-alive is 15 s) is taken as dead and opened again. Playwright "the tail says Reconnecting, then Live, with no row twice and none of the old life".

## CON-1259. The filters are in the address
Search text (debounced), Apps, Topics and APIs as MultiSelects with the count of each value on the tape, Refused only (a Toggle), Idle polls hidden (a Switch, on by default), the page back (`before`): all in the address, kept on reload, Back undoes them; a click on a client, an API or a topic in a row filters by it; an empty tape under filters names them and offers Clear filters. Playwright "filters change the rows, live in the address, and survive a reload".

## CON-1260. A request and the clients are Sheets with their own address
`?request=<id>` opens the request (detail, refused, fenced, too large; the Lab tag when a Lab rule touched it), `?clients=1` the clients; Esc, Close and Back close them, a deep link opens them, closing a deep-linked one replaces the address. Playwright "a request opens in a Sheet with its own address", "a refused request: Kafka's words, the rule behind them, what would pass", "the clients Sheet".

## CON-1261. The first-request page and the wire docks
`/connect/first-request` (who the newest client is, what it asked in order, the wire for it); `WireDock` (`web/src/pages/live/wire-dock.tsx`) with `topic` or `group` for the topic and group pages to mount, shown on `/gallery`. Both are snapshots that follow the RefreshControl. Playwright "the first-request page and the docks".

## CON-1262. Every Live control works in Firefox, WebKit and Chromium, at every width
The scenarios above run in the three engines with the mouse and the keyboard, fail on a console error, and no page scrolls sideways at 1024, 1280, 1440 and 1920 (snapshot, tail, under a Sheet); the tape's columns are chosen by the width of the table, never by its rows. Playwright "no page scrolls sideways at 1024 to 1920".
# The React console, slice R1a: Topics, the read side (rules 1100 to 1149)

Branch `react-r1a`. The API is `crates/kafka-console/src/api/topics.rs`; the pages are `web/src/pages/topics/` (one lazily loaded chunk);
the browser specs are `web/e2e/tests/topics-list.spec.ts`, `messages.spec.ts` and `topic-tabs.spec.ts`. Slice R1b builds the writes and
the Config tab on the frame this slice built (`pages/topics/frame.tsx`, the `TopicOverlays` and `TopicsOverlays` mounting points).

## CON-1100. The Topics pages are the classic view-models, serialised
`GET /api/v1/topics?q=&internal=1` (`TopicsView` and its state), `/api/v1/topics/{topic}` (`TopicFrame`: the `TopicHead` with its
records, size, last write, held-back count, offline partitions and which actions the head carries), `/messages` (the `MessagesView` for
the filters the classic page reads, with the reach sentence, whether a tail applies, the tail's cursor and the limit "Older" asks for),
`/records/{p}/{o}`, `/consumers`, `/log?partition=`, `/producers?partition=`, `/transactions/{id}`, with the words Rust decides
(the planner's and retention's rules, the transaction's sentences). Test: `the_topics_list_and_a_topics_frame_are_the_view_models_serialised`,
`consumers_log_producers_and_a_transaction_answer_with_their_words`.

## CON-1101. A name that is no topic, a record or a transaction that is not there, in the classic page's words
A page's lookup (a topic's frame, a record, a transaction) that finds nothing answers 200 `{"missing": {code, message}}` with
`UNKNOWN_TOPIC_OR_PARTITION`, `NO_SUCH_RECORD` or `TRANSACTIONAL_ID_NOT_FOUND` and the sentence the classic page says: "it is not there"
is an answer, and a 404 would put an error in Chromium's and Safari's console on a page that works. The tabs of a topic wait for its
frame, so a name that is no topic fetches nothing else; a tab's own address for no topic is still a 404 refusal. The pages
show it (No such topic; the record Sheet says why; No transaction called ...). Test: `the_topics_list_and_a_topics_frame_are_the_view_models_serialised`;
Playwright "the record Sheet ...", "Producers ...".

## CON-1102. Every filter of Messages is in the address and reaches the broker only through MessageQuery
Partitions (`?partition=0,2`), the one box (digits an offset, else a key), the time (a Popover with a field and quick choices), the
isolation level and the limit are typed search params; the API reads them with the classic `MessageQuery::parse`, so a value it
cannot use is a sentence on the page. Test: `messages_take_their_filters_from_the_query_and_a_record_is_whole`; Playwright
"partitions, key and time ...", "isolation ...", "Older ...".

## CON-1103. The record is a routed Sheet
`?record=partition:offset` opens the Sheet over the Messages page (a deep link opens both); Esc, Close and Back close it and the focus
goes back to the record's own button. JSON is laid out (Pretty) or as sent; binary is hex; headers and the producer, transaction and
position facts are the classic drawer's. Playwright "the record Sheet: a row opens it with its own address, Esc and Back close it, a deep
link opens it".

## CON-1104. The Messages tail is opt-in, server-paced and resumable
`GET /api/v1/tail/messages?topic=&pace=&since=` is server-sent events read over fetch (the API's `X-Console` proof): `ready` at once,
then a `rows` event (JSON) only when a look found new rows, at most one per `pace` seconds (1 to 30; Realtime 1, Relaxed 3, Slow 10),
its id the cursor; a reconnect sends `Last-Event-ID`. A key, an offset or a time is refused (a fixed read). One permit per stream.
Test: `the_messages_tail_is_an_event_stream_that_starts_with_ready_and_refuses_a_fixed_read`.

## CON-1105. A tail never rewrites a row, never moves the header, and holds while you read
New rows are prepended by stable id with a 1.5 s highlight; rows on screen keep their DOM nodes; the header and the toolbar never move;
while the table is scrolled, hovered, focused or has a selection, new rows wait behind an `N new · Show` pill; the indicator says
Live, Connecting, Reconnecting or Disconnected with a countdown (backoff 1, 2, 4, 8, 15 s). Playwright "the Live tail: new rows at the
top, no row rewritten, the header stable, held behind a pill while hovered".

## CON-1106. Compact now is the Lab's real round, behind a confirm
`POST /api/v1/topics/{topic}/log/compact?partition=` (same-origin proof) runs `gather::lab::compact`; the Log page's Dialog shows the
planner's verdict first, or the sentence for why nothing can run; the outcome is a toast of what the round did or the refusal in the
dialog. Without a live Lab it is `LAB_OFF`, 409. Test: `compact_now_is_a_post_with_the_proof_and_is_refused_without_a_live_lab`;
Playwright "Compact now on a compacted topic ...".

## CON-1107. The topic's frame is the same on every tab, and the tabs are routes
Name, state words, actions (Partitions, Produce, Delete topic: the dialogs are `?dialog=` for R1b), the stat strip, a transaction region
of the same height whether a transaction is open or not, and Messages, Log, Config, Consumers, Producers as `/topics/$topic/...` with
their counts; switching tabs moves nothing above the tab body. Playwright "the frame: one h1, the stats, the transaction region, five
tabs as routes, and a tab switch moves nothing above it".

## CON-1108. The Topics list's filter and Show internal work, in the address
The filter follows typing (replacing the history entry), Esc clears it, and nothing matching is its own state with Clear filter; Show
internal is a shadcn Switch (mouse, label, keyboard) that adds `__consumer_offsets`; a reload keeps both. Playwright "Show internal: the
switch works with the mouse and the keyboard ...", "the filter narrows the list as you type ...".
# Slice R4: Settings, Access, the Lab (React)

## CON-1300. Settings is its view-model, serialised
`GET /api/v1/settings` is the `SettingsView` the classic page renders (the five dynamic broker configs with their source in the table's words, the version profile, the intervals, the weight, the data directory, the flags); `GET /api/v1/settings/edit?key=&scope=` the edit dialog of one of the five (NOT_FOUND for any other). Test: `the_settings_api_answers_the_page_and_the_edit_dialog`.

## CON-1301. The Edit dialog checks and saves in Kafka's words
The dialog is routed (`?edit=<key>&scope=this`); the config is a shadcn Select, Applies to a ToggleGroup, the value has a unit hint that follows what is typed; Check sends IncrementalAlterConfigs with `validate_only`, Save without; a refusal shows Kafka's error name, code and sentence with the rule and what would pass; a save updates the table and says who sees the value. `POST /api/v1/settings/edit` answers `{outcome: saved | refused | checked | invalid | switched}`. Files: `web/e2e/tests/settings.spec.ts`. Test: `the_settings_api_answers_the_page_and_the_edit_dialog`.

## CON-1302. The weight carries no memory figure
The weight is the binary's size, the open partitions against the most kept open, and the partitions on disk. `SettingsWeight::resident` and `threads` are never serialised. Test: `the_settings_api_answers_the_page_and_the_edit_dialog`; Playwright "Settings: every section, figures as tiles, the five configs, no memory figure".

## CON-1303. The data directory wraps its whole path
The path breaks at any character inside its box (never an ellipsis, never past the card) and has its own Copy button. File: `web/e2e/tests/settings.spec.ts` ("the data directory wraps its whole path").

## CON-1310. Access is its view-model, serialised, with no secret
`GET /api/v1/access` is the `AccessView` (authentication, authorizer, TLS, users by name with mechanisms and iteration counts, ACLs, the console's principal and exposure, the start warnings) and the form's lists of resource types and operations. A user carries five fields and none is a secret. Test: `the_access_api_answers_the_page_and_the_two_acl_dialogs`.

## CON-1311. Create ACL and Delete ACL are dialogs over Access
`?acl=new` opens Create ACL (principal, host, resource type Select, pattern ToggleGroup, name, operation Select, permission ToggleGroup) with the CreateAcls call the server writes for what is typed; `?delete=<the ACL's words>` opens Delete ACL with what the broker decides afterwards (`acl_words::after_delete`) and the DeleteAcls call. A missing field is said under it; a refusal is Kafka's. The ACL table is a DataTable whose filters (text, resource type, permission) are in the address. Files: `web/e2e/tests/access.spec.ts`. Test: `the_access_api_answers_the_page_and_the_two_acl_dialogs`.

## CON-1312. A write of Settings, Access or the Lab needs the same-origin proof
Every R4 POST passes the API guard (`X-Console: 1`, no foreign `Sec-Fetch-Site`, `origin::check`); a console without the Lab answers every Lab write `LAB_OFF` and the page says the Lab is off. Test: `a_write_of_settings_access_or_the_lab_needs_the_same_origin_proof`.

## CON-1320. The Lab's API is the engine's, over a real broker
`GET /api/v1/lab` (the rules in force, ended in the last hour, the rehearsals done once, the compacted topics), `GET /api/v1/lab/form?kind=` (the choices: APIs the engine can fail, topics, apps with a connection open, groups, codes, shares, lifetimes), `POST /api/v1/lab/rules` (`check` answers the preview and the sentences; otherwise `made` with the rule's number or `invalid`), `GET /api/v1/lab/rules/{id}` (the rule and the requests it touched, each with what the rule did), `POST .../enabled`, `.../remove`, `/api/v1/lab/history/clear`, `GET|POST /api/v1/lab/once/{drop|expire|rebalance}`, `GET|POST /api/v1/lab/compact`. Test: `the_lab_api_previews_makes_switches_and_removes_a_rule_over_a_real_broker`.

## CON-1321. The Lab page's state is the data's
Empty (nothing in force, nothing ended in the hour), active (a DataTable of the rules in force with a stable id per rule, an on/off Switch, hits, the last hit as an absolute time, the end time, remove with a confirm dialog), expired (the Ended table and the list of rehearsals done once, with Clear history). File: `web/e2e/tests/lab.spec.ts`.

## CON-1322. A Slow rule for a client slows that client's requests, and the rule counts them
Made from the New rehearsal dialog (`?new=slow`) against a broker with traffic: the server's preview says what it will do before it is made; the rule's sheet (`?rule=<id>`) shows hits, the last hit, the time left counted down locally (CON-812) and the requests it touched, each with the lab tag and the delay in its time. File: `web/e2e/tests/lab.spec.ts`. Test: `the_lab_api_previews_makes_switches_and_removes_a_rule_over_a_real_broker`.

## CON-1323. A Fail rule on Produce answers the client with the code it names
The producer receives the error (kafkajs names it `NOT_LEADER_OR_FOLLOWER`); the broker is never asked. File: `web/e2e/tests/lab.spec.ts`. Test: `the_lab_api_previews_makes_switches_and_removes_a_rule_over_a_real_broker`.

## CON-1324. A rule can be switched off and on, and removed
The Switch of a row and of the sheet; Remove asks first (`?remove=<id>`) and the rule moves to Ended as Removed. Test: `the_lab_api_previews_makes_switches_and_removes_a_rule_over_a_real_broker`; `web/e2e/tests/lab.spec.ts`.

## CON-1325. A rule's form takes several APIs, topics, partitions and a group
APIs and topics are MultiSelects, partitions a MultiSelect of the chosen topics' partitions, client and group Comboboxes; each wrong field gets its own sentence from the engine. Test: `a_rule_form_takes_several_apis_topics_partitions_and_a_group`.

## CON-1326. Drop, Expire, Rebalance and Compact are tabs of the same dialog
Each lists what there is to choose (apps with a connection open, members, groups with members, compacted topics), shows the call, and leaves a line in the Lab's history. Compact now shows the planner's verdict first and runs only when a round is due; `CompactNowDialog` (`web/src/pages/lab/compact-now.tsx`) is the same dialog for a topic's Log tab, and `LabRuleBadge` (`web/src/pages/lab/badge.tsx`) the lab tag for the Live tape and its drawer. File: `web/e2e/tests/lab.spec.ts`.

## CON-1330. No browser-default control on Settings, Access or the Lab
Every list is a shadcn Select, Combobox or MultiSelect; no native select, date or number field is visible on any of the pages or their dialogs, in any browser. Files: `web/e2e/tests/settings.spec.ts`, `access.spec.ts`, `lab.spec.ts` (`noNativeSelect`).

## CON-1331. Every overlay of R4 has its own address and gives the focus back
The Edit dialog, Create ACL, Delete ACL, New rehearsal, the rule's sheet and Remove are search params: Back closes one opened from the page, a deep link opens it, Esc closes it and the focus returns to what opened it (also in Firefox and Safari, where a clicked button is not focused). Files: `web/e2e/tests/settings.spec.ts`, `access.spec.ts`, `lab.spec.ts`.

# The UX audit fixes of 2026-11-13 (rules 1340 to 1359)

An independent three-browser audit of the React console found overlays that lost the focus, a filter chip that was not
applied, frozen figures, numbers in sentences, layout shift while tailing and a crowded 1024 layout. Each fix is at its root
(a shared component where the cause was shared) and is held by a test.

## CON-1340. Every Dialog and Sheet gives the focus back to what opened it
One mechanism, `web/src/lib/opener.ts` through `web/src/hooks/use-overlay-opener.tsx`, mounted by the shared `DialogContent` and
`SheetContent`: it follows the last pointer press and the last focus for the whole document (Firefox and Safari do not focus
a clicked button), learns the opener while the overlay first renders (before the page behind goes `inert` and blurs it), keeps
a way to find the same control again (focus key, test id or label, inside its row), maps a menu item to its menu's trigger, and
falls back to the opener of the overlay it was opened from, then to `main`. No page carries its own focus code. File:
`web/e2e/tests/focus-return.spec.ts` (every overlay: keyboard + Esc, mouse + close button; and after a redraw of the opener).

## CON-1341. An offset with no partition chosen is read from on every partition
As `kcat -o N` without `-p` and Kafka UI's offset seek: the chip says `from offset N in each partition`, and each partition read
gets its share of the page, so every partition shows. A chip is never shown for a filter that is not applied. Test:
`messages_take_their_filters_from_the_query_and_a_record_is_whole`; file: `web/e2e/tests/messages.spec.ts`.

## CON-1342. A dialog's actions are always in view
`DialogFooter` sticks to the bottom of the box that scrolls, edge to edge over its padding (`--dp-x`, `--dp-y`), in every dialog;
`DialogContent` never grows past the window. File: `web/e2e/tests/calm-layout.spec.ts`.

## CON-1343. The Lab rule's sheet keeps up while it is open
It follows the interval chosen for every page and has its own Refresh (the stamp and the button), so its hits and rows arrive
without closing it. File: `web/e2e/tests/lab.spec.ts`.

## CON-1344. Clients is one definition on Live and in its sheet
A client is a `client.id` with a connection open; its connections are the unit (`9 clients`, `11 connections`), the same on the
tile, the button and the sheet. Test: `live_counts_clients_as_the_clients_sheet_does`; file:
`web/e2e/tests/live.spec.ts`.

## CON-1345. A topic's Config says where its values come from as figures that add up
Configs, Set on this topic, Set for the broker (dynamic broker and dynamic default), Start flag, Kafka's default (and Elsewhere
when Kafka names another source): labelled figures, never a sentence, and the parts sum to the whole. File:
`web/e2e/tests/topic-config.spec.ts`.

## CON-1346. A topic's figures follow its Messages tail
Each batch the table takes fetches the topic's frame again: Records and Last write change in the same boxes, never older than
the rows under them. File: `web/e2e/tests/messages.spec.ts`.

## CON-1347. A checkbox's words are its label
In Delete offsets the topic's name and its offsets are a `label` beside the checkbox (never wrapping it): a click on them ticks
and unticks. File: `web/e2e/tests/groups.spec.ts`.

## CON-1348. Create topic's done state has its own address
`/topics/new?created=<name>`: a reload shows the topic made, never the form again; without the session's answer it goes to the
topic. File: `web/e2e/tests/topic-writes.spec.ts`.

## CON-1349. The sidebar remembers the person's choice, and starts as icons below 1280 px
`doze:sidebar` in localStorage; with no choice made, icons only below 1280 px and full from it. There is one `main` landmark.
File: `web/e2e/tests/shell.spec.ts`.

## CON-1350. A tail's new rows shift nothing on screen
`DataTable`'s `anchorTop`: rows prepended while the reader is at the top are put in with the box scrolled by their height in
the same frame, then the box glides back to the top (a scroll, not a layout shift; at once with reduced motion; at once when
the person asked with Show). Measured in Chromium while tailing: Live 0.517 to 0, Messages 0.05 to 0. The held pill sits in
the tail's bar, in a slot it always has, covering no row; the speed control is one component with one set of words. Files:
`web/e2e/tests/live.spec.ts`, `web/e2e/tests/messages.spec.ts`.

## CON-1351. The wordmark's text is the name, once
Home's h1 reads `doze Kafka`: the visible text is the name, with no hidden copy beside it. File: `web/e2e/tests/pages.spec.ts`.

## CON-1352. A MultiSelect that shows chips says each name once
The field says how many (`1 of 30 APIs`), the chips say which. Files: `web/e2e/tests/lab.spec.ts`, `gallery.spec.ts`.

## CON-1353. A list in the address is the name repeated
`?topic=orders&topic=clickstream` (`web/src/lib/search-params.ts`, the router's `parseSearch` and `stringifySearch`), never JSON;
an old link written as JSON still opens its filter. Files: `web/src/lib/search-params.test.ts`, `web/e2e/tests/live.spec.ts`.

## CON-1354. One pattern for every write in a dialog
The dialog stays open and shows what the broker did, in place, then Done (`web/src/components/app/write-done.tsx`): Produce,
Add partitions, Delete records, Config edit and reset, Reset and Delete offsets, Create and Delete ACL, Settings edit, Compact
now, the Lab's once rehearsals and Remove. Only a write that removes the page's own subject (Delete topic, Delete group) leaves
for the list, with a toast. Files: `web/e2e/tests/access.spec.ts`, `groups.spec.ts`, `topic-config.spec.ts`, `settings.spec.ts`,
`lab.spec.ts`.

## CON-1355. The transaction region is one line of one height, open or calm
The callout is its title, labelled figures (Began, Held back; Timeout, Producer and partitions where the region is wide
enough, never wrapped) and the way to the transaction. File: `web/e2e/tests/calm-layout.spec.ts`.

## CON-1356. Strips of figures are balanced, their values at one height
`StatStrip` lays its tiles on a grid: all in one row when they fit, else as even as they can be (never a lone tile), and the
tiles are a subgrid, so a label on two lines moves no value. A tab's count has a reserved width. File:
`web/e2e/tests/calm-layout.spec.ts`.

## CON-1357. The topic's records are in view at 1024 x 900
A dense head (the strip, the one-line transaction region, tighter spacing), and the Live tape's narrowest columns keep an API
name and a result word whole. File: `web/e2e/tests/calm-layout.spec.ts`.

## CON-1360. A console closed to this machine is one calm page
When the API answers 403 `CONSOLE_CLOSED_TO_NETWORK` (SEC-250: the broker now has ACLs, and this browser is on another machine),
the fetch client and the stream reader mark the console closed (`web/src/api/closed.ts`) and the whole app gives way to
`web/src/pages/closed-to-network.tsx`: the wordmark, one card in the EmptyState's language with the title `This console no longer
answers other machines`, what happened, why it stopped, the two ways forward (use it from the machine doze-kafka runs on, at a
loopback address; restart with `--console-unprotected`), the server's own words, and Try again (a reload: the console answers
again once every ACL is deleted). No sidebar, top bar or toast behind it. Files: `web/e2e/tests/closed-to-network.spec.ts`, the
bun test `web/src/api/closed.test.ts`.
