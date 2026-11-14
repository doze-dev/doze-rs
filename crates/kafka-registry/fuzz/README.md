# Fuzzing kafka-registry

One target per parser: `json_node` (the ordered JSON a schema is read as), `avro_schema` (an Avro schema with a reference,
its stored text and its comparison with itself), `registry_api` (any request to the REST API: method, path, query,
headers and body, against a registry that already holds schemas) and `wire` (the record-value header and Protobuf's
message indexes).

```
cd crates/kafka-registry/fuzz
cargo +nightly fuzz run json_node corpus/json_node -- -max_len=65536
cargo +nightly fuzz run avro_schema corpus/avro_schema -- -max_len=65536
cargo +nightly fuzz run registry_api corpus/registry_api -- -max_len=65536
cargo +nightly fuzz run wire corpus/wire -- -max_len=4096
```

The corpus directories are the fuzzer's own (ignored by git).
