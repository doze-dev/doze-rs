`api-versions.json` is `conformance/recordings/api-versions.json` of doze-kafka at `4081fa6`: the ApiVersions
(v0) answer of a real broker of each Kafka major 1 to 4, recorded from real images by the Go repository's
conformance suite. It is the ground truth for what a profile may offer.

Major 4 was later replaced by Apache Kafka 4.4.0 (the `kafka_2.13-4.4.0.tgz` release, one node in KRaft combined
mode, formatted with its defaults), as `kafka-broker-api-versions.sh` lists it: 4.4.0 serves the streams group APIs at
versions 4.3.1 did not (StreamsGroupHeartbeat and StreamsGroupDescribe 0-1, StreamsGroupTopologyDescriptionUpdate 0), and
4.4.0 is the version the broker stands for.

`apiversions-client-software.json` and `apiversions-unsupported-version.json` are the same-named files in
`conformance/recordings/` at the same commit: real Apache Kafka 4.3.1 answering ApiVersions at versions it does
and does not serve and with client software names it accepts and refuses. `recorded_api_versions_exchanges.rs`
replays every ApiVersions step through the request reader, the answer and the response writer.
