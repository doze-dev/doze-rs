# Behaviour catalogue

One file per area (`wire.md`, `storage.md`, `produce-fetch.md`, `groups-classic.md`, …). It is
the port's contract: not Go code translated, but what a Kafka-compatible local broker must do,
distilled from the Go source, its docs and the recorded real-Kafka scenarios.

Each entry is one behaviour, with the Rust test that covers it:

```
### produce to an unknown topic with auto-create off
Returns UNKNOWN_TOPIC_OR_PARTITION for that partition; other partitions in the request are
unaffected. Source: conformance recording `produce-unknown-topic`.
Test: `kafka-broker::produce::tests::unknown_topic_is_per_partition`
```

A phase starts by writing its area's catalogue and ends when every entry has a passing test.
The independent audit scores the phase against this file, not against its author's memory.
