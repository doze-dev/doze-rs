// Cases the generated corpus (../corpus.ts) cannot make for kafka, because its sample values are not Kafka's: real
// topic configs, topic names and partition counts Kafka refuses, versions outside 1-4, retention as days or negative,
// and the Larder example's block. Appended to the generated cases:
//
//   bun tools/module-oracle/kafka/extra.ts >> tools/module-oracle/kafka/corpus.jsonl

const block = (version: string, body: string[], name = "t") =>
  `kafka "${name}" {\n${[`  version = "${version}"`, ...body.map((l) => "  " + l)].join("\n")}\n}\n`;

const cases: [string, string, string][] = [
  ["topic-config-real", "4", block("4", [`topic "orders" {\n    partitions = 3\n    config = { "cleanup.policy" = "compact", "retention.ms" = "86400000" }\n  }`])],
  ["topic-partitions-zero", "4", block("4", [`topic "orders" {\n    partitions = 0\n  }`])],
  ["topic-partitions-negative", "4", block("4", [`topic "orders" {\n    partitions = -2\n  }`])],
  ["topic-partitions-over-max", "4", block("4", [`topic "orders" {\n    partitions = 5000\n  }`])],
  ["topic-name-space", "4", block("4", [`topic "has space" {}`])],
  ["topic-name-dot", "4", block("4", [`topic "." {}`])],
  ["topic-name-too-long", "4", block("4", [`topic "${"a".repeat(250)}" {}`])],
  ["topic-name-dotted", "4", block("4", [`topic "users.signed-up_v2" {}`])],
  ["topic-config-unknown", "4", block("4", [`topic "orders" {\n    config = { "retention.msec" = "1000" }\n  }`])],
  ["topic-config-bad-value", "4", block("4", [`topic "orders" {\n    config = { "cleanup.policy" = "shrink" }\n  }`])],
  ["retention-days", "4", block("4", [`retention = "7d"`])],
  ["retention-negative", "4", block("4", [`retention = "-1h"`])],
  ["retention-zero", "4", block("4", [`retention = "0s"`])],
  ["retention-compound", "4", block("4", [`retention = "1h30m"`])],
  ["default-partitions-zero", "4", block("4", [`default_partitions = 0`])],
  ["default-partitions-negative", "4", block("4", [`default_partitions = -3`])],
  ["retention-bytes-unbounded", "4", block("4", [`retention_bytes = -1`])],
  ["version-5", "5", block("5", [])],
  ["version-dotted", "3.7", block("3.7", [])],
  [
    "larder",
    "4",
    `kafka "kafka" {\n  version            = 4\n  port               = 9092\n  auto_create_topics = false\n\n  topic "users.signed-up" { partitions = 3 }\n  topic "orders.placed" { partitions = 3 }\n  topic "payments.settled" { partitions = 3 }\n  topic "payments.failed" { partitions = 3 }\n}\n`,
  ],
];

cases.forEach(([kind, version, hcl], i) =>
  console.log(JSON.stringify({ id: `kafka/${String(100 + i).padStart(3, "0")}-${kind}`, engine: "kafka", version, hcl })),
);
