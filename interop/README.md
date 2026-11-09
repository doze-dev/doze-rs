# Go client interop

Real Go Kafka clients against the broker: sarama, franz-go and kafka-go. Each produces 300 records, reads them
through a consumer group of two members, then (after 50 more are produced) starts a new consumer in the same group
and checks it reads exactly the 50 that were not committed.

    interop/run.sh                 # builds the release broker, starts it on a free port, runs all three
    interop/run.sh -only kafka-go

The first produce is tried twice because a topic is made by the first request that names it, which a real Kafka
answers "unknown topic" and kafka-go does not retry. Against a broker already running:
`cd interop/go && go run . -broker 127.0.0.1:9092`.
