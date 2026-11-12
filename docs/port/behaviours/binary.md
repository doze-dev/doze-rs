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
`--meta-db --console --console-addr`, so that the conformance harness, which passes them all, can run the scenarios of the
parts that exist.
**The console flags and the start-up rule** (Phase 8, slice 8g; SEC-187 to SEC-189): `--console-allow-host HOST` (repeatable or comma separated) and `--console-unprotected` are parsed, checked and kept, and shown in `--help` with the note that the console arrives with Phase 9 and nothing yet acts on them (the start logs that they are accepted and ignored). What exists is the rule on their inputs: with SASL or TLS on, a `--console-addr` beyond loopback (unless `--console=false`) stops the start in the Go edition's words unless `--console-unprotected` is given, which starts it with a warning; a console address that is not `host:port` stops it too; and SASL without TLS on a `--listen` address that other machines can reach is warned about. **Every start refusal exits 2** (SEC-D62; it was 1). Test: `cli::tests::console::*`, `tests/process` `console::*`.
**ACLs are real** (Phase 8, slice 8b): `--acl-default allow|deny` (default `allow`: a resource no ACL covers is open, so a broker with
no ACLs refuses nothing; `deny` is Kafka's own default) and `--acl-super-user Type:name` (repeatable, or a comma list; blanks between
commas are dropped; a value that is not `Type:name` with both parts non-empty stops the start). Any other `--acl-default` stops the
start with `config: acl-default "x" must be "allow" or "deny"`, a bad super user with `config: acl super user "x" must be written
Type:name, such as User:admin`, both the Go edition's words. They apply with SASL off too (the principal is `User:ANONYMOUS`, SEC-D7).
**SASL is real** (Phase 8, slices 8a and 8c): `--sasl-mechanisms PLAIN,SCRAM-SHA-512,SCRAM-SHA-256` (comma list, exact upper-case names; any of the three) and `--sasl-user user:password`
(repeatable; split at the first colon, so a password may hold colons; ASCII). They go together: users without mechanisms, mechanisms
without users, an unknown mechanism, a name without a colon, an empty user or password, and a user given twice each stop the
start, in the Go edition's words, and none of those messages repeats a password. For a SCRAM mechanism the broker derives, at start,
a verifier for every user (4096 iterations, a random 16-byte salt per user and mechanism; nothing is stored), so a password is
the same for PLAIN and for both SCRAM hashes. The flag order is kept for the configuration; a SaslHandshake answer lists the
mechanisms in Java `HashSet` order whatever the flag says (SEC-7). The passwords are in no `Debug` output of the arguments or the settings and
in no log line (SEC-13).
**Users from a file, the environment, and the API** (Phase 8, slice 8f; rules SEC-160 to SEC-179): the users of `--sasl-user` show in `ps`, so `--sasl-users-file PATH` (one `user:password` a
line, split at the first colon; blank lines and lines whose first non-blank character is `#` are skipped; a CRLF ending and a leading byte order mark are ignored; nothing else is trimmed) and the
variable `DOZE_KAFKA_SASL_USERS` (`user:password` entries separated by commas, the space around an entry ignored, an empty entry skipped; **a password with a comma cannot be written there**) give the
same users from places `ps` does not show. The three sources add up (flag, file, environment) and are the **bootstrap** users: derived at every start, never stored, and refused by the API (SEC-D50).
A bad line is named by its number (`--sasl-users-file: line 3: not a valid entry (expected user:password)`, or `DOZE_KAFKA_SASL_USERS: entry 2: ...`) and its text is never repeated; a user given
twice across the sources names the user, never a password. On Unix the start warns (`--sasl-users-file PATH is readable by group or others (mode 644); run chmod 600 on it`) if group or others can read the file. Users from the file or
the variable without `--sasl-mechanisms` stop the start, like `--sasl-user`. **No user at all** (no flag, file or variable, and no stored credential) with SASL on still stops the start with `SASL is enabled but no
credentials are configured; give --sasl-user, --sasl-users-file or DOZE_KAFKA_SASL_USERS, or start with --sasl-allow-empty-users ...`: at once if the data directory has no database (nothing is created), else once the
database is open and has no stored user. `--sasl-allow-empty-users` starts it anyway, so that the first user is made over the API (`AlterUserScramCredentials`) by a super user (`--acl-super-user`); without a
super user nobody can then ever log in. **The bootstrap story:** the first user comes from a flag, a file or the variable; every later one can be made with `kafka-configs.sh --alter --add-config 'SCRAM-SHA-256=[iterations=4096,password=...]'`
and survives restarts. The flag needs `--sasl-mechanisms`. Test: `cli::tests::users::*`, `tests/process` `users::*`, `kafka-server tests/users_api`.
**TLS is real, behind a cargo feature** (Phase 8, slice 8e; rules SEC-140 to SEC-154): a build with `--features tls` (off by default; it needs a C compiler) takes `--tls-cert FILE` (a PEM chain, leaf
first) and `--tls-key FILE` (PKCS#8, SEC1 or RSA, unencrypted), both or neither. One alone stops the start with `config: TLS needs both --tls-cert and --tls-key, or neither`; a file that cannot be
used stops it with `config: cannot load the TLS certificate and key (--tls-cert X, --tls-key Y): <reason>` (the Go edition's words), before anything is bound, and the broker never falls back to
plaintext. The start logs `TLS is on` with the certificate's path, never the key. TLS and SASL are independent (SSL, SASL_SSL). **Refused** (the channel would otherwise be in the clear): in a build
**without** the feature, `--tls-cert` and `--tls-key` stop the start, exit non-zero, with a message that says to rebuild with `--features tls`; the flags are hidden from `--help` there and shown in a build that has them.
Test: `cli::tests::tls_is_refused_and_not_ignored_*` (default build), `cli::tests::tls::*` (`--features tls`), `tests/process` `sasl::flags_that_cannot_be_honoured_stop_the_start_and_say_why`,
`tls::a_broker_started_with_a_certificate_serves_tls_refuses_plaintext_and_prints_no_key`, `tls::a_certificate_that_cannot_be_used_stops_the_start_and_no_plaintext_listener_is_left`.
Test: `cli::tests::*` (`the_acl_flags_set_what_the_broker_enforces_and_nothing_is_ignored`, `a_bad_acl_default_or_super_user_is_refused_in_the_go_editions_words`), `duration::tests::*`, `tests/process` `acl::acl_flags_that_cannot_work_stop_the_start_in_the_go_editions_words`,
`acl::a_deny_default_with_no_sasl_binds_the_anonymous_user_and_a_restart_keeps_the_acls`, `sasl::flags_that_cannot_be_honoured_stop_the_start_and_say_why`,
`sasl::a_broker_started_with_sasl_logs_a_client_in_and_serves_it_and_prints_no_password`,
`sasl::a_broker_started_with_scram_logs_a_client_in_with_either_hash_and_prints_no_password`

### B5. Descriptors
At start the soft limit on open files is raised toward the hard one (to 65,536 at most; macOS allows 10,240), and the
number of open partitions is lowered to what the limit allows: `(limit - 256 - max connections) / 4`, never under 8
(every connection that may be open holds one). An open partition
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
