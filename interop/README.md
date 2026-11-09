# Client interop

Real Kafka clients against the broker: sarama, franz-go and kafka-go (Go libraries, in `go/`), and librdkafka
through `kcat` (a process of its own, which is also what lets one be killed without warning). Needs `go` and `kcat`.

    interop/run.sh                       # builds the release broker, starts it on a free port, runs everything
    interop/run.sh -only churn           # one scenario by name
    FRANZ_LOG=1 interop/run.sh -only franz-go    # franz-go's debug log (its join, balance and sync steps)

Against a broker already running: `cd interop/go && go run . -broker 127.0.0.1:9092`.

## The scenarios

Every one is on a fresh topic of four partitions made through sarama's admin client.

**Groups.** Two members of one library (four scenarios); two libraries with each as the first joiner, and so the
leader; all four together, with the `range` assignor and again with `round-robin`. Every member joins, and the test
waits until sarama's `DescribeConsumerGroups` says Stable with all of them; then 300 keyed records are produced and
read until all have been seen, and the members leave (which commits). Every member must have read something. Then
50 more are produced and a new consumer of the same group must read exactly those 50.

**churn.** All four libraries in one group while 2000 records are produced over ten seconds. franz-go leaves; a
librdkafka member is killed with SIGKILL, so the broker is told nothing and its seat must go when its 6 s session runs
out (checked: between 4 s and 7.5 s later); a new sarama member joins. Every record must still be read (some twice:
what the killed member read and had not committed), and at the end every partition's committed offset must be its end.

## What the clients taught

- Records are keyed, because franz-go's producer puts a whole unkeyed batch in one partition, which leaves the other
  members nothing to read.
- `kcat -o beginning` starts at offset 0 on every run whatever is committed; the resume needs
  `-X auto.offset.reset=earliest`. kcat reads no option after the first topic.
- kafka-go does not retry a first Metadata answer for a topic that does not exist yet (a recorded Kafka answers it
  the same way); franz-go asks for auto-creation only when told to. The topic is therefore made first.

## The new group protocol (KIP-848)

franz-go (`kgo.ServerSideBalancer()`; its `GroupProtocol` option is only the classic protocol's name) and librdkafka
(`-X group.protocol=consumer`) also run as consumer-protocol members (`franz-go-848`, `librdkafka-848`). Scenarios:
the same two libraries in a group of their own; `upgrade-848` (classic members first, then new-protocol ones join: an
upgrade) and `classic-joins-848` (the reverse); `churn-848`. A graceful shutdown of a group of both stops the
new-protocol members last, as the last of them gone turns the group classic again and a classic member's final commit
would be under a generation that is gone. kafka-go and sarama stay classic: kafka-go writes subscription version 1
without the owned partitions, which a coordinator that reads subscriptions cannot read, so it is left out of groups
with the new protocol. `-members franz-go-848,sarama` runs a group of the named clients, in join order.
