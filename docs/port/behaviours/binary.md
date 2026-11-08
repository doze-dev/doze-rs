# Behaviours: the `doze-kafka` binary

### B1. One broker per data directory
A lock file in the data directory is held for the life of the process; a second broker is refused with a message that
says why. (redb is given no file locks by our file seam, so this is what keeps two processes off one database.)
Test: `tests/process::a_second_broker_on_the_same_data_directory_is_refused`, `lock::tests::*`

### B2. Listening, and saying so
`listening addr=<host:port>` on stderr once the listener is bound (the port actually bound, for `--listen :0`); text or
JSON lines (`--log-format`), no colour unless stderr is a terminal.

### B3. SIGTERM / SIGINT is a clean stop
The server stops first (nothing new arrives; waiting fetches are dropped), then every open partition is flushed and
closed and the stop is recorded (`clean-shutdown`), then the process exits 0. The next start then trusts what it finds;
after a kill (no marker) it distrusts every partition and the first flush covers what it reads back.
Test: `tests/process::it_serves_a_client_stops_on_sigterm_and_comes_back_with_the_data`, `an_acknowledged_write_survives_the_process_being_killed`; `kafka-broker::tests/shutdown::*`

### B4. Flags
Kept from the Go edition with the same meaning: `--listen --advertised-host --advertised-port --data-dir --segment-bytes
--index-interval-bytes --retention-bytes --retention-ms --fsync-interval --kafka-version --auto-create-topics
--default-partitions --max-partitions --message-max-bytes --log-level --log-format`; durations in Go's syntax (`1m30s`).
New: `--worker-threads` (default 2; idle threads are weight). Accepted and ignored, hidden, until their phase:
`--meta-db --group-initial-rebalance-delay --consumer-session-timeout --offsets-retention --acl-default --acl-super-user
--console --console-addr`, so that the conformance harness, which passes them all, can run the scenarios of the parts
that exist. **Refused** (the broker would otherwise be open): `--sasl-mechanisms --sasl-user --tls-cert --tls-key`.
Test: `cli::tests::*`, `duration::tests::*`, `tests/process::authentication_flags_stop_the_start_and_say_why`

### B5. Descriptors
At start the soft limit on open files is raised toward the hard one (to 65,536 at most; macOS allows 10,240), and the
number of open partitions is lowered to what the limit allows: `(limit - 256) / 4`, never under 8. An open partition
holds a log and an index, one more while its directory is flushed, and one of headroom. A flush of a directory that finds
no free descriptor (`EMFILE`, `ENFILE`) waits up to ten times 20 ms for one; it is not a failed flush and does not take
the partition offline. One request that needs more partitions than the limit allows (a consumer of 300 partitions at a
256 limit) is answered with storage errors for what could not open, and the broker stays up and serves the next request.
Test: `descriptors::tests::*`, `kafka-storage log::tests::append::a_directory_flush_that_finds_no_free_descriptor_*`;
by hand: 400 acks=all produces at a hard limit of 256 all succeed, 32 descriptors held at the end.

### B6. The clean-stop marker names what the stop cannot vouch for
For every partition this run opened, the verdict is its last close: flushed whole (vouched) or not. A partition this run
never opened keeps what the last stop said of it. A partition that was closed well to make room, reopened, written to,
and closed badly at the stop is therefore not vouched for.
Test: `shutdown::a_partition_whose_close_fails_*`, `shutdown::a_partition_closed_well_when_evicted_but_badly_at_the_stop_*`
