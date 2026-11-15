// The seeded world every spec counts on, through the Kafka protocol: three topics, records in them, a consumer group with
// committed offsets. Made by global-setup.ts on a fresh data directory, and made again by the Data spec after it erases
// everything for real.
import { Kafka, logLevel } from "kafkajs";
import { SEEDED } from "./playwright.config";

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

export async function seedWorld() {
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
}
