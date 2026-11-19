// Screenshots of a topic's Empty dialog (CON-1590) in light and dark: the preview with a group below the new start, the done
// state, and a topic that only compacts. Makes its own topics through the Kafka protocol.
//   bun --bun shots-empty.ts <console url> <kafka> <out dir>
import { chromium } from "@playwright/test";
import { Kafka, logLevel } from "kafkajs";
const [url = "http://127.0.0.1:39922", broker = "127.0.0.1:39920", out = "shots"] = process.argv.slice(2);
const k = new Kafka({ clientId: "orders-svc", brokers: [broker], logLevel: logLevel.NOTHING });
const a = k.admin();
await a.connect();
const tag = Date.now().toString(36);
const topics = { light: `orders.v2-${tag}`, dark: `orders.v3-${tag}` };
await a.createTopics({ topics: [...Object.values(topics).map((t) => ({ topic: t, numPartitions: 6 })), { topic: `levels-${tag}`, numPartitions: 1, configEntries: [{ name: "cleanup.policy", value: "compact" }] }] });
const p = k.producer({ allowAutoTopicCreation: false });
await p.connect();
for (const t of Object.values(topics)) for (let part = 0; part < 6; part++) await p.send({ topic: t, messages: Array.from({ length: 20 + part * 37 }, (_, i) => ({ partition: part, key: `ord-${i}`, value: JSON.stringify({ order_id: `ord-${i}`, state: "paid", note: "x".repeat(200) }) })) });
await p.send({ topic: `levels-${tag}`, messages: [{ key: "sku-1", value: "3" }] });
await p.disconnect();
for (const t of Object.values(topics)) {
  await a.setOffsets({ groupId: "fulfilment-svc", topic: t, partitions: [0, 1, 2, 3, 4, 5].map((x) => ({ partition: x, offset: String(x * 10) })) });
  await a.setOffsets({ groupId: "analytics", topic: t, partitions: [{ partition: 3, offset: "50" }] });
}
await a.disconnect();
const b = await chromium.launch();
for (const theme of ["light", "dark"] as const) {
  const ctx = await b.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: theme });
  const pg = await ctx.newPage();
  await pg.goto(url + "/");
  await pg.evaluate((th) => localStorage.setItem("doze-kafka.theme", th), theme);
  const t = topics[theme];
  await pg.goto(`${url}/topics/${t}`);
  await pg.getByTestId("act-empty").waitFor();
  await pg.waitForTimeout(400);
  await pg.screenshot({ path: `${out}/empty-entry.${theme}.png` });
  await pg.getByTestId("act-empty").click();
  await pg.getByTestId("empty-part").first().waitFor();
  await pg.getByTestId("empty-confirm").fill(t);
  await pg.waitForTimeout(500);
  await pg.screenshot({ path: `${out}/empty-dialog.${theme}.png` });
  await pg.getByTestId("empty-submit").click();
  await pg.getByTestId("emptied").waitFor();
  await pg.waitForTimeout(400);
  await pg.screenshot({ path: `${out}/empty-done.${theme}.png` });
  await pg.goto(`${url}/topics/levels-${tag}?dialog=empty`);
  await pg.getByTestId("refusal").waitFor();
  await pg.waitForTimeout(400);
  await pg.screenshot({ path: `${out}/empty-compact.${theme}.png` });
  await ctx.close();
}
await b.close();
console.log("shots in", out);
