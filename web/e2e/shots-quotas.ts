// Screenshots of the client quotas on the Access page and their two dialogs, light and dark, for a person to look at.
//   bun --bun shots-quotas.ts <console url> <kafka host:port> <out dir>
// It sets three quotas from the console's API, then runs a producer as client id `shots-p` that its 20 KiB/s quota holds back,
// so the table of who the quotas hold has a muted client, and takes the shots while it is muted.
import { chromium, type Page } from "@playwright/test";
import { Kafka, logLevel } from "kafkajs";

const [url = "http://127.0.0.1:39411", kafkaAddr = "127.0.0.1:39410", out = "shots"] = process.argv.slice(2);

async function setQuota(body: Record<string, unknown>) {
  const r = await fetch(`${url}/api/v1/access/quotas`, { method: "POST", headers: { "X-Console": "1", "Content-Type": "application/json", Origin: url }, body: JSON.stringify(body) });
  if (!r.ok) throw new Error(`${r.status} ${await r.text()}`);
}

async function traffic() {
  const kafka = new Kafka({ clientId: "shots-p", brokers: [kafkaAddr], logLevel: logLevel.NOTHING });
  const admin = kafka.admin();
  await admin.connect();
  await admin.createTopics({ topics: [{ topic: "payments", numPartitions: 1 }] }).catch(() => undefined);
  await admin.disconnect();
  const p = kafka.producer({ allowAutoTopicCreation: false });
  await p.connect();
  const blob = "x".repeat(10_000);
  for (let i = 0; i < 40; i++) await p.send({ topic: "payments", acks: 1, messages: [{ value: blob }] });
  await p.disconnect();
}

async function shot(page: Page, name: string, theme: "light" | "dark") {
  await page.emulateMedia({ colorScheme: theme });
  await page.waitForTimeout(400);
  await page.screenshot({ path: `${out}/${name}-${theme}.png`, fullPage: name === "access" });
}

await setQuota({ user: null, client: "shots-p", key: "producer_byte_rate", value: 20480 });
await setQuota({ user: "", client: null, key: "consumer_byte_rate", value: 5242880 });
await setQuota({ user: "orders", client: "reports", key: "request_percentage", value: 25 });
// The producer runs while the shots are taken, so it is muted in them.
const running = traffic();
await new Promise((r) => setTimeout(r, 4000));
const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
for (const theme of ["light", "dark"] as const) {
  await page.emulateMedia({ colorScheme: theme });
  await page.goto(`${url}/access`);
  await page.getByTestId("quotas").scrollIntoViewIfNeeded();
  await shot(page, "access", theme);
  await page.getByTestId("quotas").screenshot({ path: `${out}/quotas-${theme}.png` });
  await page.goto(`${url}/access?quota=new`);
  await page.getByTestId("quota-who").getByRole("radio", { name: "User and client id" }).click();
  await page.getByTestId("quota-user").fill("orders");
  await page.getByTestId("quota-client-default").click();
  await page.getByTestId("quota-value").fill("1048576");
  await shot(page, "quota-set", theme);
  await page.goto(`${url}/access`);
  await page.getByTestId("quotas").getByRole("row", { name: /shots-p/ }).getByTestId("quota-remove").click();
  await page.getByTestId("quota-remove-dialog").waitFor();
  await shot(page, "quota-remove", theme);
}
await browser.close();
await running;
console.log(`shots in ${out}`);
