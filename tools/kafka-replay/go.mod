// The doze-kafka replay oracle: replays 81 scenarios recorded against Apache Kafka 4.3.1 against a doze-kafka binary
// (CONFORMANCE_BIN) and compares every answer; -record records them again from Kafka in Docker. A local gate, not in CI;
// see README.md.
module doze-rs/tools/kafka-replay

go 1.27.0

require (
	github.com/golang/snappy v1.0.0
	github.com/klauspost/compress v1.20.2
	github.com/pierrec/lz4/v4 v4.1.33
	github.com/twmb/franz-go/pkg/kmsg v1.14.0
)
