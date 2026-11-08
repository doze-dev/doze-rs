`api-versions.json` is `conformance/recordings/api-versions.json` of doze-kafka at `4081fa6`: the ApiVersions
(v0) answer of a real broker of each Kafka major 1 to 4, recorded from real images by the Go repository's
conformance suite. It is the ground truth for what a profile may offer.

`apiversions-client-software.json` and `apiversions-unsupported-version.json` are the same-named files in
`conformance/recordings/` at the same commit: real Apache Kafka 4.3.1 answering ApiVersions at versions it does
and does not serve and with client software names it accepts and refuses. `recorded_api_versions_exchanges.rs`
replays every ApiVersions step through the request reader, the answer and the response writer.
