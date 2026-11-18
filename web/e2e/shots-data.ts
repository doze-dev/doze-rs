// Screenshots of the Data page and its dialogs (docs/port/data-control.md), light and dark, for a person to look at.
//   bun --bun shots-data.ts <console url> <kafka host:port> <out dir> [--seed]
// With --seed it first fills the broker: topics of every kind (one that keeps everything and passes 1 GiB, so the heads-up
// shows), records, a group's commits. Then a first look, more traffic, and a minute later the shots (so growth shows): Home,
// the page, and each dialog.
import { chromium, firefox, type Page } from "@playwright/test";
import { Kafka, logLevel } from "kafkajs";

const [url = "http://127.0.0.1:39094", kafkaAddr = "127.0.0.1:39092", out = "shots", ...flags] = process.argv.slice(2);
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function seed() {
  const kafka = new Kafka({ clientId: "shots-data", brokers: [kafkaAddr], logLevel: logLevel.NOTHING });
  const admin = kafka.admin();
  await admin.connect();
  await admin.createTopics({
    topics: [
      { topic: "audit.log", numPartitions: 4, configEntries: [{ name: "retention.ms", value: "-1" }] },
      { topic: "clickstream", numPartitions: 6, configEntries: [{ name: "retention.ms", value: "3600000" }] },
      { topic: "orders", numPartitions: 3, configEntries: [{ name: "retention.ms", value: "-1" }] },
      { topic: "payments.settled", numPartitions: 2 },
      { topic: "inventory.levels", numPartitions: 1, configEntries: [{ name: "cleanup.policy", value: "compact" }] },
      { topic: "customer.profiles", numPartitions: 1, configEntries: [{ name: "cleanup.policy", value: "compact,delete" }, { name: "retention.bytes", value: "268435456" }] },
    ],
  });
  await admin.disconnect();
  const p = kafka.producer({ allowAutoTopicCreation: false });
  await p.connect();
  const blob = "x".repeat(100_000);
  const fill = async (topic: string, bytes: number, partitions: number) => {
    for (let sent = 0; sent < bytes; sent += 9 * partitions * blob.length) {
      await p.send({ topic, acks: 1, messages: Array.from({ length: 9 * partitions }, (_, i) => ({ key: `${topic}-${sent}-${i}`, value: blob, partition: i % partitions })) });
    }
  };
  await fill("audit.log", 1.1 * 2 ** 30, 4);
  await fill("clickstream", 180 * 2 ** 20, 6);
  await fill("orders", 64 * 2 ** 20, 3);
  await fill("payments.settled", 12 * 2 ** 20, 2);
  await fill("inventory.levels", 3 * 2 ** 20, 1);
  await fill("customer.profiles", 6 * 2 ** 20, 1);
  const c = kafka.consumer({ groupId: "fulfilment" });
  await c.connect();
  await c.subscribe({ topic: "payments.settled", fromBeginning: true });
  let seen = 0;
  await c.run({ eachMessage: async () => void seen++ });
  const end = Date.now() + 20_000;
  while (seen < 100 && Date.now() < end) await sleep(200);
  await c.disconnect();
  await p.disconnect();
}

/// A first look, then more traffic and the shots a minute later: growth is measured between the two looks.
async function grow() {
  const kafka = new Kafka({ clientId: "shots-data", brokers: [kafkaAddr], logLevel: logLevel.NOTHING });
  const p = kafka.producer({ allowAutoTopicCreation: false });
  await p.connect();
  await fetch(`${url}/api/v1/data`, { headers: { "X-Console": "1" } });
  const blob = "x".repeat(100_000);
  for (const [topic, n] of [["clickstream", 400], ["orders", 60]] as const) {
    for (let i = 0; i < n; i += 9) await p.send({ topic, acks: 1, messages: Array.from({ length: 9 }, (_, j) => ({ key: `grow-${i}-${j}`, value: blob })) });
  }
  await p.disconnect();
  await sleep(62_000);
}

async function settle(p: Page) {
  await p.locator("h1").first().waitFor();
  await p.waitForLoadState("networkidle");
  await p.waitForTimeout(700);
}

if (flags.includes("--seed")) await seed();
await grow();

const shots: [string, string, ((p: Page) => Promise<void>) | null, boolean][] = [
  ["home", "/", null, false],
  ["page", "/data", null, true],
  ["empty", "/data?dialog=empty&topic=orders", null, false],
  ["empty-all", "/data?dialog=empty-all", async (p) => void (await p.getByTestId("confirm").fill("empty all topics")), false],
  ["erase", "/data?dialog=erase", async (p) => void (await p.getByTestId("confirm").fill("erase every")), false],
];
for (const [name, type] of [["chromium", chromium], ["firefox", firefox]] as const) {
  const b = await type.launch();
  for (const scheme of ["light", "dark"] as const) {
    for (const [state, path, act, full] of shots) {
      const ctx = await b.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: scheme });
      await ctx.addInitScript((s) => localStorage.setItem("doze-kafka.theme", s), scheme);
      const page = await ctx.newPage();
      await page.goto(url + path);
      await settle(page);
      if (act) await act(page);
      await page.waitForTimeout(300);
      await page.screenshot({ path: `${out}/data-${state}.${name}.${scheme}.png`, fullPage: full });
      await ctx.close();
    }
  }
  await b.close();
}
console.log("shots in", out);
