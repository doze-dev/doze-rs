# Behaviours: server (kafka-server)

How a connection is served, distilled from the Go source (`internal/protocol/conn.go`, `server.go` at `4081fa6`).
TLS is Phase 8 (slice 8e); SASL, the login session and the pre-authentication budget are Phase 8 slice 8a (`security.md`); the traffic observer is Phase 9.

### V1. One request at a time, in order
A connection reads a frame, answers it, then reads the next. A client may send its next request before it has the
answer (pipelining); the bytes wait in the buffer and are answered in order. A frame may arrive in any number of
pieces.
Test: `connection::tests::requests_are_answered_in_order_whether_they_arrive_together_or_a_byte_at_a_time`

### V2. ApiVersions advertises what is served, and only that
The profile's APIs, narrowed to the ones the broker has a handler for. A request for any other API is not accepted
(the connection is closed without an answer, as for an API Kafka does not advertise), so a client never negotiates
what it will be refused.
Test: `connection::tests::api_versions_lists_only_what_the_broker_serves`, `an_api_the_broker_does_not_serve_closes_the_connection_without_an_answer`; `kafka-wire::profile::tests::only_the_served_apis_are_advertised_and_accepted`

### V3. A frame over its API's cap closes the connection before the body is waited for
Test: `connection::tests::a_frame_over_its_cap_closes_the_connection_before_the_body_is_waited_for` (the caps
themselves: `kafka-wire::frame`).

### V4. Time limits
A connection may idle 10 minutes between requests (Kafka's `connections.max.idle.ms`). Once the first byte of a
request has arrived the whole frame has 30 s, counted from that byte (a trickle does not restart it; bytes left over
from the last read count as begun). A response has 30 s to be written to a client that has stopped reading.
Test: `a_connection_with_nothing_to_say_is_closed_after_the_idle_time`, `a_request_that_trickles_in_has_one_budget_not_one_per_byte`, `a_partial_request_left_in_the_buffer_has_the_frame_budget_not_the_idle_one`, `a_client_that_stops_reading_cannot_hold_the_connection`

### V5. A client that goes is not an error; one that goes mid-request is
Test: `a_client_that_hangs_up_mid_request_is_an_error_and_between_requests_a_goodbye`

### V6. A connection limit
At most 2048 connections (configurable); the next is closed as it arrives and its place is free again when a
connection ends.
Test: `tests/tcp::connections_past_the_limit_are_closed_and_a_freed_place_is_reused`

### V7. Stopping
Stopping the server closes every connection, including one waiting on a handler (a long-poll Fetch), and returns
once their tasks are gone; nothing listens afterwards.
Test: `tests/tcp::shutdown_closes_idle_connections_and_returns`, `connection::tests::stopping_the_server_ends_a_connection_that_is_waiting_for_a_request`

### Long polls, acks=0 and big frames
A stop interrupts a handler that waits: `tcp::stopping_the_server_does_not_wait_out_a_fetch_that_is_waiting_for_records`.
`Outcome::NoResponse` (Produce `acks=0`) writes nothing and the next answer is the next request's:
`tcp::an_acks_zero_produce_gets_no_answer_and_the_connection_goes_on_in_order`.
A frame of 64 KiB to 16 MiB is read into a buffer kept for the next one (`pool.rs`; numbers in weight.md); bigger frames
are read in chunks. Tests: `connection::tests::a_big_frame_*`.
