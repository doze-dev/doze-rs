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

## Transactions

Run with `-only eos-franz-go`, `-only txn-franz-go`, `-only txn-sarama`, `-only txn-librdkafka`, `-only fence-franz-go`.

**txn-franz-go, txn-sarama, txn-librdkafka.** A transactional producer of that library (librdkafka's is a small C program, `interop/c/txnprod.c`, which `run.sh` builds against the librdkafka that kcat uses, since kcat has no transactions) writes three transactions (commit, abort,
commit): one of ten records, one of ten that is aborted, and one of five, to a topic of three partitions. Three readers then read the
topic from the start: franz-go, sarama and kcat, each reading committed records only and again reading everything.
The committed readers must see exactly the records of the two commits (15, each once); the others all 25. Each takes
about 9 s: the readers wait two seconds without a record before they say nothing more is coming.

**fence-franz-go.** A zombie: a transactional producer has written five records when another with the same transactional id
starts and commits five of its own. The broker aborts the first's transaction and fences it, so the first's commit fails; a
committed reader sees only the second's records, and one reading everything sees both.

**eos-franz-go.** Consume-transform-produce: franz-go's `GroupTransactSession` reads 200 input records in batches of
25 and writes them to an output topic, committing the input offsets inside each transaction. One transaction is
deliberately aborted and redone. The output must hold every record exactly once for a `read_committed` reader, and
every input offset must be committed.

kafka-go is left out of the readers: its `ReadCommitted` ignores the aborted list and hands the control batches
(markers) up as messages, so it cannot tell a transaction from its markers. sarama has no new-protocol consumer, and
its transactional producer is the classic protocol, which is all this broker serves.
