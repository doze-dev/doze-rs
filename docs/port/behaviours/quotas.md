# Client quotas

DescribeClientQuotas (48), AlterClientQuotas (49), and the four quotas a client is held to: `producer_byte_rate`,
`consumer_byte_rate`, `request_percentage` and `controller_mutation_rate`. What Apache Kafka 4.4.0 does, read from its source
(**(source)**, the file named) and held to the running broker: `crates/kafka-wire/examples/oracle_diff` sends the same
requests to real Kafka 4.4.0 (KRaft, combined mode) and to doze-kafka and compares the answers (75 steps, 0 differences,
2026-11-15); the arithmetic is held to Kafka's own classes run on the same inputs (`quotas::meter::tests`, from
`Sensor`/`Rate`/`TokenBucket`/`QuotaUtils` in the 4.4.0 jars); `kafka-configs.sh`, `kafka-producer-perf-test.sh` and
`kafka-consumer-perf-test.sh` were run against both (results under QT-12).

Code: `crates/kafka-broker/src/quotas.rs` (what is set and the meters), `quotas/meter.rs` (Kafka's `Rate` and
`TokenBucket`), `quotas/entity.rs` (the rules of the two APIs), `handlers/{describe,alter}_client_quotas.rs`,
`broker/served.rs` (enforcement), `kafka-server/src/connection.rs` (the mute), `kafka-meta/src/store/quotas.rs` (the table).

## QT-1. An entity is a user, a client id, both, or an address; each by name or as the default (source)
`ClientQuotaControlManager.validateEntity` and `configKeysForEntityType`, in this order and these words, INVALID_REQUEST (42):
no part "Invalid empty client quota entity"; a type other than `user`, `client-id`, `ip` "Unhandled client quota entity type: T";
an empty name "Empty T not supported"; an address with a user or client id "Invalid quota entity combination, IP entity
shouldnot be combined with User or ClientId" (Kafka's missing space kept); an address that is neither an address nor a name
that resolves "A is not a valid IP or resolvable host.". A type named twice keeps the last name (the request is read into a map).
Test: `quotas::entity::tests::each_refusal_of_an_entity_is_kafkas_message`, `::an_address_must_be_one_or_resolve`; oracle steps `refusals`

## QT-2. Each key belongs to its entity, is finite, above zero and whole where Kafka's type is (source)
`validateQuotaKeyValue`: a user or client id takes `producer_byte_rate` and `consumer_byte_rate` (LONG) and
`request_percentage` and `controller_mutation_rate` (DOUBLE); an address takes `connection_creation_rate` (INT). Otherwise
"Invalid configuration key K"; not finite "Quota K must be a finite number"; zero or less "Quota K must be greater than 0";
past the type "Proposed value for K is too large for a LONG." / "an INT."; a remainder over 1e-6 "K cannot be a fractional
value.". One bad value refuses the whole entity; the other entities of the request stand.
Test: `quotas::entity::tests::each_refusal_of_a_value_is_kafkas_message`, `::doubles_take_fractions_and_a_tiny_remainder_is_a_whole_number`,
`tests/client_quotas.rs::a_refused_entry_is_invalid_request_with_kafkas_words_and_the_others_stand`

## QT-3. Duplicates (source)
`ClientQuotaControlManager.alterClientQuotas`: an entry that names a key twice, or names an entity an earlier entry of the request
already altered, is "Ignoring duplicate entity ClientQuotaEntity(entries={...})" (client-id before user, the default `null`),
and the answer is one per entity: the refusal of a repeat takes the place of the earlier entry's answer while the earlier change
still stands, as in Kafka. A removal of a key the entity does not have changes nothing and is no error; removing an entity's last
key removes the entity.
Test: `quotas::entity::tests::a_repeated_key_or_entity_is_ignored_as_a_duplicate_and_the_first_change_stands`,
`::a_removal_of_the_last_key_removes_the_entity_and_of_an_absent_key_changes_nothing`; oracle steps `refusals, for real`, `clear`

## QT-4. Stored durably, in force once stored
A change is written to the metadata store (table `quotas`, one record per entity) in one transaction that is flushed before it is
answered and before it is put in force; a failed write answers every entry KAFKA_STORAGE_ERROR and changes nothing. A restart
puts the stored quotas back. `validate_only` judges and stores nothing. Changes are made one at a time (the broker's quota lock).
Test: `kafka-meta store::quotas::tests::quotas_survive_a_reopen_and_a_removal_is_gone_for_good`,
`tests/client_quotas.rs::quotas_set_are_described_and_kept_across_a_restart`

## QT-5. DescribeClientQuotas matches as Kafka's metadata image does (source)
`ClientQuotasImage.describe`: components are checked in order: an empty type "Invalid empty entity type."; a type twice "Entity
type T cannot appear more than once in the filter."; a type that is not user, client-id or ip is UNSUPPORTED_VERSION (35)
"Unsupported entity type T"; match type 0 (exact) needs a name ("Request specified MATCH_TYPE_EXACT, but set match string to
null."), 1 (default) and 2 (any name) take none ("... MATCH_TYPE_DEFAULT, but also specified a match string." / "...
MATCH_TYPE_SPECIFIED, ..."), anything else "Unknown match type N"; an ip component with a user or client-id one "Invalid entity
filter component combination. IP filter component should not be used with user or clientId filter component.". A refused
filter is answered with its code and message and null entries. No component: every entity, or none when `strict`. Otherwise the
entities that have each named part (exact name, the default, or any), and, when `strict`, no other part. A successful answer's
message is the empty string, as Kafka's generated default.
Test: `quotas::entity::tests::a_filter_matches_as_kafkas_image_does`, `::a_filter_is_refused_in_kafkas_order_and_words`; oracle:
the 21 filters at v0 and v1

## QT-6. Which quota applies: the most specific level that matches (source)
Per quota, separately (`ClientQuotaManager.DefaultQuotaCallback`): user and client id; user and default client id; user; default
user and client id; default user and default client id; default user; client id; default client id. The usage is kept under the
tags of the level that matched: a user level is one meter for every client id of the user, a default-client-id level a meter per
client id. With one level only in use the tags come without a search (`quotaTypesEnabled`), with the same outcome. As in Kafka a
request with no client id escapes the client-id levels altogether (its tags are a user alone), and the principal's name is the
part after `User:`.
Test: `quotas::tests::the_most_specific_level_that_matches_applies_in_kafkas_order`, `::each_level_alone_is_found_with_its_default_behind_it`,
`::a_client_with_no_client_id_escapes_client_id_quotas_as_in_kafka`, `::a_user_quota_is_shared_by_every_client_id_of_the_user`,
`::a_default_client_id_quota_meters_each_client_id_apart`, `::each_quota_is_its_own_and_an_address_quota_switches_nothing_on`

## QT-7. A rate is Kafka's windowed rate, and the throttle Kafka's (source)
11 windows of one second (`quota.window.num`, `quota.window.size.seconds`), 12 kept; a window is over a window after it began; a
window with nothing recorded for 11 s starts again; the span is from the oldest window's start to now and never less than 10 s;
the rate is the sum over the span. A value is recorded whatever the outcome, and the client is throttled when the rate is over
the limit, for `(rate - limit) / limit * span`, rounded as Java rounds (`QuotaUtils.throttleTime`). The request quota is never
more than one window (`ClientRequestQuotaManager`). A meter idle for an hour is dropped.
Test: `quotas::meter::tests::*` (four vectors of Kafka's own classes), `quotas::tests::the_request_quota_throttles_for_one_window_at_most`,
`::meters_go_when_the_last_limit_goes_and_idle_ones_are_dropped_after_an_hour`

## QT-8. Produce and Fetch (source)
`KafkaApis.handleProduceRequest`: the request's size (header and body) counts against `producer_byte_rate`, its time against
`request_percentage` unless `acks=0`; the records are taken either way and the answer (none for `acks=0`) carries the longer
throttle. `handleFetchRequest`: a consumer's `max_bytes` is first cut to what it may be sent in one go
(`maxValueInQuotaWindow`: the limit times 10 s), its time counted, then the answer's size (body and four bytes); throttled for
either, the size is taken back and the answer is empty (no error, session 0, no partition). A follower's Fetch (a replica id) is
exempt from both, as Kafka's is.
Test: `tests/client_quotas_enforced.rs::a_producer_over_its_byte_rate_is_answered_with_the_throttle_and_muted_for_it`,
`::a_fetch_over_its_byte_rate_is_answered_empty_and_its_bytes_are_given_back`, `quotas::tests::a_fetch_is_capped_at_what_the_span_less_one_window_allows`,
`::a_throttled_fetch_gives_its_bytes_back`

## QT-9. Every other request counts its time; the answer carries the throttle (source)
`RequestHandlerHelper.sendResponseMaybeThrottle`: each request's time counts against `request_percentage`, and every answer that
has a `throttle_time_ms` carries it (ignorable where the version has no such field, as in Kafka).
Test: `kafka-wire api::throttle::tests::the_throttle_is_written_where_the_version_has_it_and_left_out_where_it_has_not`

## QT-10. The controller mutation quota (source)
`ControllerMutationQuotaManager`: a token bucket that fills at the rate up to 11 s worth (`TokenBucket`). CreateTopics counts a
topic's partitions once it has passed every check (validate-only too), CreatePartitions the partitions added once their number has
been checked, DeleteTopics a topic's partitions once it is found and may be deleted. Strict from CreateTopics v6, DeleteTopics v5,
CreatePartitions v3: while the bucket is below zero the topic is THROTTLING_QUOTA_EXCEEDED (89) with no message
(`ApiError.fromThrowable` drops Kafka's generic one), and the one that empties it is taken. Below those versions every mutation
is taken and the client throttled. The answer carries the longer of the mutation throttle (counted down from when it was worked
out) and the request throttle. This is a different gate from telemetry's THROTTLING_QUOTA_EXCEEDED (W29), which is per client
instance and has no quota behind it.
Test: `quotas::tests::strict_mutations_take_the_one_that_empties_the_bucket_and_refuse_the_next`, `::permissive_mutations_are_taken_and_throttle_the_client`,
`::with_no_mutation_quota_every_mutation_is_taken`, `tests/client_quotas_enforced.rs::the_mutation_quota_refuses_the_topic_after_the_one_that_empties_it_from_v6_and_throttles_below`;
oracle steps `mutations: *`

## QT-11. The connection is muted for the throttle time (KIP-219)
The answer goes at once with its throttle time; the connection then reads nothing more from that client until the time is over
(what it sends meanwhile waits in the socket), as Kafka mutes the channel. A stop does not wait for it.
Test: `kafka-server connection::tests::a_throttled_client_is_not_read_from_until_its_throttle_is_over`

## QT-12. Nothing is metered while no quota is set
With no quota of the four set anywhere a request pays one atomic load; no meter, no clock, no allocation. The console's requests
(`Broker::handle`) are never metered; a client's come through `Broker::serve`.
Test: `tests/client_quotas_enforced.rs::with_no_quota_a_clients_request_is_served_as_the_consoles_and_never_muted`, `quotas::tests::each_quota_is_its_own_and_an_address_quota_switches_nothing_on`

Seen with Kafka's own tools (2026-11-15, the same machine, 100 000 B/s for client ids `perf-p` and `perf-c`, 2000 records of
1000 bytes): `kafka-producer-perf-test.sh` 0.15 MB/s, produce-throttle-time max 2162 ms, avg 95.6 ms against Kafka; 0.14 MB/s,
2160 ms, 93.1 ms against doze-kafka. `kafka-consumer-perf-test.sh` fetch rate 0.0927 MB/s, fetch-throttle-time max 9996 ms against
Kafka; 0.0896 MB/s, 9758 ms against doze-kafka (the averages differ, 150 against 893 ms, with how many empty long polls each
run made; the client's window is 30 s).

## Decided differences
- **QT-D1.** The request quota counts the time the broker spent working on the request (the time its future was being polled,
  so the wait of a long-poll Fetch or for a lock is left out), not a request thread's CPU time, and not the network thread's time
  writing the answer. ApiVersions and the SASL exchange are answered by the connection and are not metered (Kafka meters
  ApiVersions after a login).
- **QT-D2.** Address quotas (`ip`, `connection_creation_rate`) are validated, stored and described as Kafka does, and not enforced:
  no connection is delayed for its address's rate.
- **QT-D3.** Entries of DescribeClientQuotas are ordered by entity and their values by key, and AlterClientQuotas answers in the
  order entities first appear; Kafka's orders are its hash maps'. An entity's parts are listed client-id before user, which is
  the order Kafka's map happens to give.
- **QT-D4.** The windows are Kafka's defaults (11 of one second, for client and controller quotas alike); `quota.window.num` and
  `quota.window.size.seconds` cannot be set.
