// Starts the real binary (target/release/doze-kafka, or DOZE_KAFKA_BIN) on a fresh data directory and seeds a small world
// through the Kafka protocol: three topics, records in them, a consumer group with committed offsets. Returns the teardown,
// which stops only the process it started.
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { Kafka, logLevel } from "kafkajs";
import { SEEDED } from "./playwright.config";

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function waitFor(url: string, ms: number) {
  const end = Date.now() + ms;
  for (;;) {
    try {
      const r = await fetch(url);
      if (r.ok) return;
    } catch {
      // not yet
    }
    if (Date.now() > end) throw new Error(`${url} did not answer in ${ms} ms`);
    await sleep(100);
  }
}

export default async function setup() {
  const bin = process.env.DOZE_KAFKA_BIN ?? resolve(import.meta.dir, "../../target/release/doze-kafka");
  const data = mkdtempSync(join(tmpdir(), "doze-e2e-"));
  const child = spawn(bin, ["--listen", SEEDED.kafka, "--data-dir", data, "--console-addr", SEEDED.console.replace("http://", "")], { stdio: "ignore" });
  await waitFor(`${SEEDED.console}/healthz`, 15_000);

  const kafka = new Kafka({ clientId: "e2e-seed", brokers: [SEEDED.kafka], logLevel: logLevel.NOTHING });
  const admin = kafka.admin();
  await admin.connect();
  await admin.createTopics({
    topics: [
      { topic: "orders", numPartitions: 3 },
      { topic: "payments.settled", numPartitions: 2 },
      { topic: "inventory.levels", numPartitions: 1, configEntries: [{ name: "cleanup.policy", value: "compact" }] },
    ],
  });
  await admin.disconnect();
  const producer = kafka.producer({ allowAutoTopicCreation: false });
  await producer.connect();
  for (const topic of ["orders", "payments.settled", "inventory.levels"]) {
    await producer.send({ topic, messages: Array.from({ length: 12 }, (_, i) => ({ key: `${topic}-${i}`, value: JSON.stringify({ i, topic }) })) });
  }
  await producer.disconnect();
  const consumer = new Kafka({ clientId: "fulfilment", brokers: [SEEDED.kafka], logLevel: logLevel.NOTHING }).consumer({ groupId: "fulfilment" });
  await consumer.connect();
  await consumer.subscribe({ topic: "orders", fromBeginning: true });
  let seen = 0;
  await consumer.run({ eachMessage: async () => void seen++ });
  const end = Date.now() + 20_000;
  while (seen < 12 && Date.now() < end) await sleep(200);
  await consumer.disconnect();

  return async () => {
    child.kill("SIGTERM");
    await new Promise((r) => child.once("exit", r));
    rmSync(data, { recursive: true, force: true });
  };
}
