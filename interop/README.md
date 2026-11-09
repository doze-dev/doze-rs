# Go client interop

Real Go Kafka clients against the broker: sarama, franz-go and kafka-go. Eight scenarios, each on a fresh topic of
three partitions made through sarama's admin client:

- a group of two members of one library (three scenarios);
- groups of two libraries, with each as the first joiner (and so the leader);
- a group of all three, with the `range` assignor and again with `round-robin`.

In each, every member joins and the group is waited for until sarama's `DescribeConsumerGroups` says Stable with all
of them; then 300 keyed records are produced, read until all have been seen, and the members leave (which commits).
Every member must have read something. Then 50 more are produced and a new consumer of the same group must read
exactly those 50. Records are keyed because franz-go's producer puts a whole unkeyed batch in one partition, which
leaves the other members nothing to read.

    interop/run.sh                       # builds the release broker, starts it on a free port, runs all
    interop/run.sh -only mixed           # one scenario (sarama, franz-go, kafka-go, franz-sarama, ..., mixed, mixed-rr)
    FRANZ_LOG=1 interop/run.sh -only franz-go    # franz-go's debug log (its join, balance and sync steps)

Against a broker already running: `cd interop/go && go run . -broker 127.0.0.1:9092`.
