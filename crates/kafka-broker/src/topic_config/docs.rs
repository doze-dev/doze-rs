//! What Kafka 4.3.1 says each topic config means (DescribeConfigs v3 and later, when documentation is asked for), and the
//! order it lists them in (a Java hash map's: recorded in `topic-config-keys`, so it is the order here too).

/// The order Kafka lists the topic configs in.
pub(super) static ORDER: &[&str] = &[
    "compression.type",
    "remote.log.delete.on.disable",
    "leader.replication.throttled.replicas",
    "remote.storage.enable",
    "min.insync.replicas",
    "segment.jitter.ms",
    "remote.log.copy.disable",
    "local.retention.ms",
    "cleanup.policy",
    "flush.ms",
    "follower.replication.throttled.replicas",
    "compression.lz4.level",
    "segment.bytes",
    "retention.ms",
    "compression.gzip.level",
    "flush.messages",
    "compression.zstd.level",
    "max.compaction.lag.ms",
    "file.delete.delay.ms",
    "max.message.bytes",
    "min.compaction.lag.ms",
    "message.timestamp.type",
    "local.retention.bytes",
    "preallocate",
    "index.interval.bytes",
    "min.cleanable.dirty.ratio",
    "unclean.leader.election.enable",
    "retention.bytes",
    "delete.retention.ms",
    "message.timestamp.after.max.ms",
    "message.timestamp.before.max.ms",
    "segment.ms",
    "segment.index.bytes",
];

/// Each config's documentation, as Kafka words it.
pub(super) static DOCS: &[(&str, &str)] = &[
    (
        "compression.type",
        "Specify the final compression type for a given topic. This configuration accepts the standard compression codecs ('gzip', 'snappy', 'lz4', 'zstd'). It additionally accepts 'uncompressed' which is equivalent to no compression; and 'producer' which means retain the original compression codec set by the producer.",
    ),
    (
        "remote.log.delete.on.disable",
        "Determines whether tiered data for a topic should be deleted after tiered storage is disabled on a topic. This configuration should be enabled when trying to set <code>remote.storage.enable</code> from true to false",
    ),
    (
        "leader.replication.throttled.replicas",
        "A list of replicas for which log replication should be throttled on the leader side. The list should describe a set of replicas in the form [PartitionId]:[BrokerId],[PartitionId]:[BrokerId]:... or alternatively the wildcard '*' can be used to throttle all replicas for this topic.",
    ),
    (
        "remote.storage.enable",
        "To enable tiered storage for a topic, set this configuration to true. To disable tiered storage for a topic that has it enabled, set this configuration to false. When disabling, you must also set <code>remote.log.delete.on.disable</code> to true.",
    ),
    (
        "min.insync.replicas",
        "Specifies the <i>minimum</i> number of in-sync replicas (including the leader) required for a write to succeed when a producer sets <code>acks</code> to \"all\" (or \"-1\"). In the <code>acks=all</code> case, every in-sync replica must acknowledge a write for it to be considered successful. E.g., if a topic has <code>replication.factor</code> of 3 and the ISR set includes all three replicas, then all three replicas must acknowledge an <code>acks=all</code> write for it to succeed, even if <code>min.insync.replicas</code> happens to be less than 3. If <code>acks=all</code> and the current ISR set contains fewer than <code>min.insync.replicas</code> members, then the producer will raise an exception (either <code>NotEnoughReplicas</code> or <code>NotEnoughReplicasAfterAppend</code>).<br> Regardless of the <code>acks</code> setting, the messages will not be visible to the consumers until they are replicated to all in-sync replicas and the <code>min.insync.replicas</code> condition is met.<br> When used together, <code>min.insync.replicas</code> and <code>acks</code> allow you to enforce greater durability guarantees. A typical scenario would be to create a topic with a replication factor of 3, set <code>min.insync.replicas</code> to 2, and produce with <code>acks</code> of \"all\". This ensures that a majority of replicas must persist a write before it's considered successful by the producer and it's visible to consumers.<p>Note that when the Eligible Leader Replicas feature is enabled, the semantics of this config changes. Please refer to <a href=\"#eligible_leader_replicas\">the ELR section</a> for more info.</p>",
    ),
    (
        "segment.jitter.ms",
        "The maximum random jitter subtracted from the scheduled segment roll time to avoid thundering herds of segment rolling",
    ),
    (
        "remote.log.copy.disable",
        "Determines whether tiered data for a topic should become read only, and no more data uploading on a topic. Once this config is set to true, the local retention configuration (i.e. local.retention.ms/bytes) becomes irrelevant, and all data expiration follows the topic-wide retention configuration(i.e. retention.ms/bytes).",
    ),
    (
        "local.retention.ms",
        "The number of milliseconds to keep the local log segment before it gets deleted. Default value is -2, it represents <code>retention.ms</code> value is to be used. The effective value should always be less than or equal to <code>retention.ms</code> value.",
    ),
    (
        "cleanup.policy",
        "This config designates the retention policy to use on log segments. The \"delete\" policy (which is the default) will discard old segments when their retention time or size limit has been reached. The \"compact\" policy will enable <a href=\"#compaction\">log compaction</a>, which retains the latest value for each key. It is also possible to specify both policies in a comma-separated list (e.g. \"delete,compact\"). In this case, old segments will be discarded per the retention time and size configuration, while retained segments will be compacted. An empty list means infinite retention - no cleanup policies will be applied and log segments will be retained indefinitely. Note that with remote storage enabled, local retention limits (log.local.retention.ms and log.local.retention.bytes) are still applied to local segments.",
    ),
    (
        "flush.ms",
        "This setting allows specifying a time interval at which we will force an fsync of data written to the log. For example if this was set to 1000 we would fsync after 1000 ms had passed. Note that this setting depends on the broker-level configuration <code>log.flush.scheduler.interval.ms</code>, which controls how frequently the flush check occurs. In general we recommend you not set this and use replication for durability and allow the operating system's background flush capabilities as it is more efficient.",
    ),
    (
        "follower.replication.throttled.replicas",
        "A list of replicas for which log replication should be throttled on the follower side. The list should describe a set of replicas in the form [PartitionId]:[BrokerId],[PartitionId]:[BrokerId]:... or alternatively the wildcard '*' can be used to throttle all replicas for this topic.",
    ),
    (
        "compression.lz4.level",
        "The compression level to use if compression.type is set to <code>lz4</code>.",
    ),
    (
        "segment.bytes",
        "This configuration controls the segment file size for the log. Retention and cleaning is always done a file at a time so a larger segment size means fewer files but less granular control over retention.",
    ),
    (
        "retention.ms",
        "This configuration controls the maximum time we will retain a log before we will discard old log segments to free up space if we are using the \"delete\" retention policy. This represents an SLA on how soon consumers must read their data. If set to -1, no time limit is applied. Additionally, retention.ms configuration operates independently of \"segment.ms\" and \"segment.bytes\" configurations. Moreover, it triggers the rolling of new segment if the retention.ms condition is satisfied.",
    ),
    (
        "compression.gzip.level",
        "The compression level to use if compression.type is set to <code>gzip</code>.",
    ),
    (
        "flush.messages",
        "This setting allows specifying an interval at which we will force an fsync of data written to the log. For example if this was set to 1 we would fsync after every message; if it were 5 we would fsync after every five messages. In general we recommend you not set this and use replication for durability and allow the operating system's background flush capabilities as it is more efficient. This setting can be overridden on a per-topic basis (see <a href=\"#topicconfigs\">the per-topic configuration section</a>).",
    ),
    (
        "compression.zstd.level",
        "The compression level to use if compression.type is set to <code>zstd</code>.",
    ),
    (
        "max.compaction.lag.ms",
        "The maximum time a message will remain ineligible for compaction in the log. Only applicable for logs that are being compacted.",
    ),
    (
        "file.delete.delay.ms",
        "The time to wait before deleting a file from the filesystem",
    ),
    (
        "max.message.bytes",
        "The largest record batch size allowed by Kafka (after compression if compression is enabled).",
    ),
    (
        "min.compaction.lag.ms",
        "The minimum time a message will remain uncompacted in the log. Only applicable for logs that are being compacted.",
    ),
    (
        "message.timestamp.type",
        "Define whether the timestamp in the message is message create time or log append time.",
    ),
    (
        "local.retention.bytes",
        "The maximum size of local log segments that can grow for a partition before it deletes the old segments. Default value is -2, it represents <code>retention.bytes</code> value to be used. The effective value should always be less than or equal to <code>retention.bytes</code> value.",
    ),
    (
        "preallocate",
        "True if we should preallocate the file on disk when creating a new log segment.",
    ),
    (
        "index.interval.bytes",
        "This setting controls how frequently Kafka adds entries to its offset index and, conditionally, to its time index. The default setting ensures that we index a message roughly every 4096 bytes. More frequent indexing allows reads to jump closer to the exact position in the log but results in larger index files. You probably don't need to change this.<p> Note: the time index will be inserted only when the timestamp is greater than the last indexed timestamp.</p>",
    ),
    (
        "min.cleanable.dirty.ratio",
        "This configuration controls how frequently the log compactor will attempt to clean the log (assuming <a href=\"#compaction\">log compaction</a> is enabled). By default we will avoid cleaning a log where more than 50% of the log has been compacted. This ratio bounds the maximum space wasted in the log by duplicates (at 50% at most 50% of the log could be duplicates). A higher ratio will mean fewer, more efficient cleanings but will mean more wasted space in the log. If the max.compaction.lag.ms or the min.compaction.lag.ms configurations are also specified, then the log compactor considers the log to be eligible for compaction as soon as either: (i) the dirty ratio threshold has been met and the log has had dirty (uncompacted) records for at least the min.compaction.lag.ms duration, or (ii) if the log has had dirty (uncompacted) records for at most the max.compaction.lag.ms period.",
    ),
    (
        "unclean.leader.election.enable",
        "Indicates whether to enable replicas not in the ISR set to be elected as leader as a last resort, even though doing so may result in data loss.<p>Note: In KRaft mode, when enabling this config dynamically, it needs to wait for the unclean leader electionthread to trigger election periodically (default is 5 minutes). Please run <code>kafka-leader-election.sh</code> with <code>unclean</code> option to trigger the unclean leader election immediately if needed.</p>",
    ),
    (
        "retention.bytes",
        "This configuration controls the maximum size a partition (which consists of log segments) can grow to before we will discard old log segments to free up space if we are using the \"delete\" retention policy. By default there is no size limit only a time limit. Since this limit is enforced at the partition level, multiply it by the number of partitions to compute the topic retention in bytes. Additionally, retention.bytes configuration operates independently of \"segment.ms\" and \"segment.bytes\" configurations. Moreover, it triggers the rolling of new segment if the retention.bytes is configured to zero.",
    ),
    (
        "delete.retention.ms",
        "The amount of time to retain delete tombstone markers for <a href=\"#compaction\">log compacted</a> topics. This setting also gives a bound on the time in which a consumer must complete a read if they begin from offset 0 to ensure that they get a valid snapshot of the final stage (otherwise delete tombstones may be collected before they complete their scan).",
    ),
    (
        "message.timestamp.after.max.ms",
        "This configuration sets the allowable timestamp difference between the message timestamp and the broker's timestamp. The message timestamp can be later than or equal to the broker's timestamp, with the maximum allowable difference determined by the value set in this configuration. If message.timestamp.type=CreateTime, the message will be rejected if the difference in timestamps exceeds this specified threshold. This configuration is ignored if message.timestamp.type=LogAppendTime.",
    ),
    (
        "message.timestamp.before.max.ms",
        "This configuration sets the allowable timestamp difference between the broker's timestamp and the message timestamp. The message timestamp can be earlier than or equal to the broker's timestamp, with the maximum allowable difference determined by the value set in this configuration. If message.timestamp.type=CreateTime, the message will be rejected if the difference in timestamps exceeds this specified threshold. This configuration is ignored if message.timestamp.type=LogAppendTime.",
    ),
    (
        "segment.ms",
        "This configuration controls the period of time after which Kafka will force the log to roll even if the segment file isn't full to ensure that retention can delete or compact old data.",
    ),
    (
        "segment.index.bytes",
        "This configuration controls the size of the index that maps offsets to file positions. We preallocate this index file and shrink it only after log rolls. You generally should not need to change this setting.",
    ),
];
