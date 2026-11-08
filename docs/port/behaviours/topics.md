# Behaviours: topics (CreateTopics, DeleteTopics, Metadata)

From the Go source (`internal/broker/{topics,metadata,configs,topiclimits}.go`, `internal/settings`, at `4081fa6`) and
the recordings `createtopics-validation`, `createtopics-collisions`, `metadata-autocreate`, `topic-config-keys`
(Apache Kafka 4.3.1). Wording is Kafka's wherever the recording has it.

### T1. Each topic of a CreateTopics is answered on its own
A refused topic does not stop the others. A name listed twice is answered once with INVALID_REQUEST "Duplicate topic
name." and neither is created. Every answer has a non-null config list; a refused topic answers -1 partitions and
-1 replication factor and a zero topic id.
Test: `tests/create_topics::each_request_is_answered_as_apache_kafka_answers_it`, `a_name_listed_twice_is_refused_once_and_neither_is_created_and_the_rest_are_not_held_up`

### T2. Names and collisions
Kafka's name rules with Kafka's messages (INVALID_TOPIC_EXCEPTION). `a.b` and `a_b` collide (message "Topic 'x'
collides with existing topic: y"), except two that arrive in one request. **Differs from Kafka:** names that differ only
in letter case are refused (Kafka accepts both); they would share a directory on a case-insensitive file system.
Test: `each_request_is_answered_as_apache_kafka_answers_it` (name steps), `names_of_one_request_are_held_against_the_topics_before_it_for_dot_and_underscore_only`

### T3. Partitions and replicas
-1 is the broker default; zero or less is INVALID_PARTITIONS; above `max_partitions` is INVALID_PARTITIONS. A
replication factor of 0 or below -1 is INVALID_REPLICATION_FACTOR. **Differs from Kafka:** a factor above one is
accepted and answered as one (Connect and Streams ask for three by default, and there is one node). A manual
assignment sets the partition count itself: with a replication factor or a count also given it is INVALID_REQUEST;
gaps, repeats, empty lists and other brokers are INVALID_REPLICA_ASSIGNMENT.
Test: same steps; `a_manual_assignment_cannot_also_say_how_many_partitions_or_replicas`, `a_topic_may_have_the_limit_of_partitions_and_no_more`

### T4. Topic configs
Only Kafka's topic config keys, with values Kafka would accept (INVALID_CONFIG, Kafka's words; the first bad key in
name order). **Differs from Kafka:** `segment.bytes` may be as low as 1024 (Kafka 1048576). The answer lists the
configuration in force: defaults, with the topic's own marked as dynamic topic config.
Test: `topic_config::tests::*`, `tests/create_topics::the_configuration_in_force_is_answered_with_what_the_topic_set_marked_as_its_own`

### T5. A validation makes nothing
ValidateOnly runs every check and answers with a made-up topic id, creating nothing.
Test: `tests/create_topics::a_validation_makes_nothing_but_answers_as_if_it_had`

### T6. A topic is registered and has its directories, or neither
The registration is undone if a partition directory cannot be made; directories left by a topic that no longer exists
are removed before the new one is made (a new topic never adopts old records).
Test: `a_topic_is_registered_only_if_its_partition_directories_could_be_made`, `tests/topics::a_directory_left_by_a_topic_that_no_longer_exists_is_not_adopted_by_a_new_one`

### T7. Durability
A topic acknowledged is there after any power cut, with the data directory it lives in; a delete acknowledged is
gone. (A topic registered but with its directories not yet flushed is possible after a cut in the middle of a create;
the partition creates its directory when it is first opened, slice 3d.)
Test: `tests/topics::a_power_cut_after_any_file_operation_of_a_create_*`, `*_of_a_delete_*`, `a_topic_that_was_acknowledged_is_there_after_a_power_cut_with_its_directories`

### T8. DeleteTopics
By name (every version) or by id (v6 and later). Unknown, or a name that could not be a topic's: UNKNOWN_TOPIC_OR_PARTITION.
The registration goes first and then the data; a failure to clear the disk is logged, not reported. A topic of the same
name afterwards is a new topic with a new id.
Test: `tests/topics::deleting_a_topic_takes_its_registration_and_directories_and_frees_the_name_for_a_new_topic`, `a_topic_can_be_deleted_by_its_id`

### T9. Metadata
This broker is the only broker, the controller, and every partition's leader and only replica (epoch 0). No topic list
means all topics, by name; an empty list means none from v1 and all in v0. An unknown topic is UNKNOWN_TOPIC_OR_PARTITION;
a name that can never be a topic is INVALID_TOPIC_EXCEPTION; an unknown id is UNKNOWN_TOPIC_ID. A topic a client asks to
have created (always before v4) is created after the answer, which says it does not exist; the next request finds it.
Test: `tests/topics::metadata_describes_*`, `naming_no_topics_*`, `an_unknown_topic_is_unknown_*`, `a_name_that_can_never_be_a_topic_is_invalid_*`

### Not yet
Authorisation (Phase 8); `CreatePartitions`, `DescribeConfigs`, `AlterConfigs` and acting on the configs (Phase 7; the
broker acts on none of them yet); auto-creation honouring `--auto-create-topics` from the command line (3g).
