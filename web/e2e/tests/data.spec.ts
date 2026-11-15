// Data (docs/port/data-control.md): what is using the disk and why, Empty topic, Empty all topics and Erase everything, driven
// with the mouse and the keyboard against the real broker; every action is checked by what the broker then says. The last
// test erases everything for real and seeds the world again, so the specs after it find what they count on.
import { Kafka, logLevel } from "kafkajs";
import { SEEDED } from "../playwright.config";
import { seedWorld } from "../seed-world";
import { cutHeaders, expect, overflowX, test } from "./fixtures";
import { seeded } from "./kafka";
import { fresh, get, produce, setConfig, topic } from "./r1b-helpers";

test.skip(!seeded, "the Data spec empties and erases: it runs on the seeded world only");

test("the page says what is on disk, largest first, why each is that size, and where the space can be given back", async ({ page, request, baseURL }, info) => {
  const name = fresh(info, "disk");
  await topic(request, baseURL!, name, 2);
  // The "event" template keeps seven days; this one keeps everything.
  await setConfig(request, baseURL!, name, "retention.ms", "-1");
  for (let i = 0; i < 6; i++) await produce(request, baseURL!, name, `k${i}`, "x".repeat(400), "0");
  await page.goto("/");
  await page.getByTestId("sidebar").getByRole("link", { name: "Data", exact: true }).click();
  await expect(page).toHaveURL(/\/data$/);
  await expect(page.locator("h1")).toHaveText("Data");
  await expect(page.getByTestId("total")).toHaveText(/\d+(\.\d)? (B|KiB|MiB|GiB)/);
  await expect(page.getByTestId("kinds").locator("[data-kind=topics]")).toContainText("Topics");
  await expect(page.getByTestId("kinds").locator("[data-kind=metadata]")).toContainText("meta.redb");
  await expect(page.getByTestId("volume")).toContainText("free of");
  const row = page.locator(`tr[data-row="${name}"]`);
  await expect(row).toBeVisible();
  await expect(row.getByTestId("why")).toHaveText("No retention limit: it keeps everything written to it, for ever.");
  await expect(row.getByTestId("unbounded")).toHaveText("Keeps everything");
  // The compacted topic of the seeded world says why it is its size in its own words.
  await expect(page.locator('tr[data-row="inventory.levels"]').getByTestId("why")).toContainText("Compacted: it keeps the latest value of every key");
  // What the broker's defaults keep (the Settings spec may have set log.retention.ms, which stays: the words follow it).
  await expect(page.getByTestId("data-notes")).toContainText(/A topic that sets no retention of its own (keeps everything|takes the broker's: Retention)/);
  await expect(page.getByTestId("give-back")).toContainText("doze-kafka data");
  for (const w of [1024, 1440]) {
    await page.setViewportSize({ width: w, height: 900 });
    expect(await overflowX(page), `overflow at ${w}`).toBe(0);
  }
  expect(await cutHeaders(page)).toEqual([]);
  // Settings' data section leads here too.
  await page.goto("/settings#data");
  await page.getByTestId("settings-data-link").click();
  await expect(page).toHaveURL(/\/data$/);
});

test("Empty a topic from its row: what goes and what stays, then it is empty and carries on from its end", async ({ page, request, baseURL }, info) => {
  const name = fresh(info, "empty");
  await topic(request, baseURL!, name, 1);
  for (let i = 0; i < 5; i++) expect(await produce(request, baseURL!, name, `k${i}`, `v${i}`, "0")).toBe(i);
  await page.goto("/data");
  await page.getByTestId(`empty-${name}`).click();
  await expect(page).toHaveURL(new RegExp(`dialog=empty&topic=${name}`));
  const d = page.getByTestId("dialog-empty");
  await expect(d.getByRole("heading", { level: 2 })).toContainText(`Empty ${name}?`);
  await expect(d.getByTestId("empty-records")).toContainText("5");
  await expect(d.getByTestId("keeps")).toContainText("The topic, its partitions and its configs stay.");
  // Escape closes it, and the address forgets it; the row opens it again.
  await page.keyboard.press("Escape");
  await expect(d).toBeHidden();
  await expect(page).not.toHaveURL(/dialog=/);
  await page.getByTestId(`empty-${name}`).click();
  await d.getByTestId("empty-submit").click();
  await expect(d.getByTestId("emptied")).toContainText(`Emptied ${name}: 5 records in 1 partition`);
  await d.getByTestId("write-done").click();
  await expect(d).toBeHidden();
  // The broker agrees: the next record takes the next offset, and the page shows it emptied.
  expect(await produce(request, baseURL!, name, "after", "v", "0")).toBe(5);
  const data = await get(request, "/api/v1/data");
  const row = data.body.view.topics.find((t: { name: string }) => t.name === name);
  expect(row.bytes).toBeLessThan(200);
});

test("Empty is refused, said before the button, while a transaction is open on the topic", async ({ page, request, baseURL }, info) => {
  const name = fresh(info, "txn");
  await topic(request, baseURL!, name, 1);
  const kafka = new Kafka({ clientId: "e2e-txn", brokers: [SEEDED.kafka], logLevel: logLevel.NOTHING });
  const producer = kafka.producer({ transactionalId: `${name}-tx`, idempotent: true, maxInFlightRequests: 1 });
  await producer.connect();
  const txn = await producer.transaction();
  try {
    await txn.send({ topic: name, messages: [{ key: "k", value: "in a transaction" }] });
    await page.goto(`/data?dialog=empty&topic=${encodeURIComponent(name)}`);
    const d = page.getByTestId("dialog-empty");
    await expect(d.getByTestId("blocked")).toContainText("A transaction is under way");
    await expect(d.getByTestId("empty-submit")).toBeDisabled();
    await page.keyboard.press("Escape");
  } finally {
    await txn.abort();
    await producer.disconnect();
  }
  await page.goto(`/data?dialog=empty&topic=${encodeURIComponent(name)}`);
  await expect(page.getByTestId("dialog-empty").getByTestId("empty-submit")).toBeEnabled();
});

test("Empty all topics and Erase everything wait for their phrase, then do it; the world is seeded again after", async ({ page, request, baseURL }, info) => {
  // Seeding the world again waits for a consumer group to commit.
  test.setTimeout(120_000);
  const name = fresh(info, "all");
  await topic(request, baseURL!, name, 1);
  await produce(request, baseURL!, name, "k", "v", "0");
  try {
    await page.goto("/data");
    await page.getByTestId("open-empty-all").click();
    const all = page.getByTestId("dialog-empty-all");
    await expect(all.getByTestId("keeps")).toBeVisible();
    const submit = all.getByTestId("empty-all-submit");
    await expect(submit).toBeDisabled();
    await all.getByTestId("confirm").fill("empty all");
    await expect(submit).toBeDisabled();
    await all.getByTestId("confirm").fill("empty all topics");
    await submit.click();
    await expect(all.getByTestId("emptied-all")).toContainText(/Emptied \d+ topics/);
    await all.getByTestId("write-done").click();
    expect(await produce(request, baseURL!, name, "k", "v", "0")).toBe(1);

    await page.getByTestId("open-erase").click();
    const erase = page.getByTestId("dialog-erase");
    await expect(erase.getByTestId("removes")).toContainText("Every topic");
    await expect(erase.getByTestId("keeps")).toContainText("ACLs");
    await expect(erase.getByTestId("erase-submit")).toBeDisabled();
    await erase.getByTestId("confirm").fill("erase everything");
    await erase.getByTestId("erase-submit").click();
    await expect(erase.getByTestId("erased")).toContainText(/Erased \d+ topics/);
    await erase.getByTestId("write-done").click();
    await expect(page.getByText("No topics: the data directory holds the metadata store and nothing else.")).toBeVisible();
    const topics = await get(request, "/api/v1/topics");
    expect(topics.body.view.total).toBe(0);
  } finally {
    await seedWorld();
  }
  const again = await get(request, "/api/v1/topics");
  expect(again.body.view.total).toBe(3);
});
