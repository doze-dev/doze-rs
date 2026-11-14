// Traffic for the tail scenarios: against the seeded world (no E2E_URL) the test writes records itself through the Kafka
// protocol; against a broker that already has traffic (E2E_URL) it writes nothing and waits for that traffic.
import { Kafka, logLevel } from "kafkajs";
import { SEEDED } from "../playwright.config";

export const seeded = !process.env.E2E_URL;

/** Writes `n` records to `topic` (seeded world only). */
export async function produce(topic: string, n: number, tag = "e2e") {
  if (!seeded) return;
  const kafka = new Kafka({ clientId: "e2e-tail", brokers: [SEEDED.kafka], logLevel: logLevel.NOTHING });
  const p = kafka.producer({ allowAutoTopicCreation: false });
  await p.connect();
  await p.send({ topic, messages: Array.from({ length: n }, (_, i) => ({ key: `${tag}-${Date.now()}-${i}`, value: JSON.stringify({ tag, i, at: Date.now() }), headers: { "content-type": "application/json" } })) });
  await p.disconnect();
}
