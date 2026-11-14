// The other pages of a topic (slice R1a): Partitions, Consumers, Log (with the Lab's Compact now on a compacted topic),
// Producers and a transaction. Each asserts what the page says and what its controls do.
import { Kafka, logLevel } from "kafkajs";
import { expect, overflowX, test } from "./fixtures";
import { SEEDED } from "../playwright.config";

test("Partitions: its own head, a row per partition, and Delete records asks for its dialog in the address", async ({ page }) => {
  await page.goto("/topics/orders");
  await page.getByTestId("act-partitions").click();
  await expect(page).toHaveURL(/\/topics\/orders\/partitions$/);
  await expect(page.locator("h1")).toHaveText("Partitions");
  await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toHaveText(/Topics\s*›\s*orders/);
  await expect(page.getByTestId("topic-tabs")).toHaveCount(0);
  const table = page.getByRole("table", { name: "Partitions" });
  await expect(table.locator("tbody tr[data-row]").first()).toBeVisible();
  const n = await table.locator("tbody tr[data-row]").count();
  expect(n).toBeGreaterThanOrEqual(3);
  await expect(table.locator("#r-0")).toContainText("orders-0");
  await expect(table.locator("#r-0")).toContainText(/clear|held/);
  await expect(page.getByTestId("list-offsets-call")).toContainText("ListOffsets v");
  await page.getByTestId("delete-records-1").click();
  await expect(page).toHaveURL(/dialog=delete-records/);
  await expect(page).toHaveURL(/partition=1/);
  // R1b fills the overlay: the dialog opens on that partition, and Esc closes it.
  await expect(page.getByTestId("dialog-delete-records").getByTestId("records-partition")).toHaveText("1");
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("dialog-delete-records")).toBeHidden();
  await expect(page).not.toHaveURL(/dialog=/);
  await page.getByRole("navigation", { name: "Breadcrumb" }).getByRole("link", { name: "orders" }).click();
  await expect(page).toHaveURL(/\/topics\/orders\/messages/);
  await expect(page.locator("h1")).toHaveText("orders");
});

test("Consumers: the groups that committed on the topic, with their lag, and a group's link goes to it", async ({ page }) => {
  await page.goto("/topics/orders/consumers");
  const reader = page.getByTestId("reader").first();
  await expect(reader).toBeVisible();
  const group = (await reader.getAttribute("data-group"))!;
  await expect(reader.getByTestId("behind")).toHaveText(/^[\d,]+$/);
  await expect(reader.getByRole("table")).toContainText("orders-0");
  const count = await page.getByTestId("reader").count();
  await expect(page.getByTestId("topic-tabs").getByRole("link", { name: /^Consumers/ })).toContainText(String(count));
  await reader.getByRole("link", { name: group }).click();
  await expect(page).toHaveURL(new RegExp(`/groups/${group}$`));
});

test("Log: the ruler, the cards and the segments; a segment opens in place; the partition is in the address", async ({ page }) => {
  await page.goto("/topics/orders/log");
  await expect(page.getByTestId("log")).toBeVisible();
  const strip = page.getByTestId("log").locator("[data-slot=stat-strip]");
  await expect(page.getByTestId("log-state")).toHaveText(/^(Empty|Due|Not due|Retention)$/);
  for (const label of ["Start offset", "End offset", "Last stable", "Records", "Segments", "Size"]) await expect(strip).toContainText(label);
  await expect(page.getByTestId("ruler")).toBeVisible();
  await expect(page.getByTestId("durability")).toContainText("Durability");
  await expect(page.getByTestId("planner")).toBeVisible();
  const seg = page.getByTestId("segment").first().getByRole("button");
  const was = await seg.getAttribute("aria-expanded");
  await seg.click();
  await expect(seg).toHaveAttribute("aria-expanded", was === "true" ? "false" : "true");
  await page.getByTestId("log-partition").getByRole("radio", { name: "1" }).click();
  await expect(page).toHaveURL(/partition=1/);
  await expect(page.getByTestId("log")).toContainText("partition 1 of");
  // A topic that only deletes has no Compact now.
  await expect(page.getByTestId("compact-now")).toHaveCount(0);
  for (const width of [1024, 1440]) {
    await page.setViewportSize({ width, height: 900 });
    // Polled: crossing 1280 px the sidebar folds to icons with its own short transition.
    await expect.poll(() => overflowX(page), { message: `${width}` }).toBeLessThanOrEqual(0);
  }
});

test("Log of an empty partition: it says so, and gives the command that writes the first record (CON-533)", async ({ page }, info) => {
  const topic = `r5-empty-${info.project.name}`;
  const kafka = new Kafka({ clientId: "r5-empty", brokers: [process.env.E2E_KAFKA ?? SEEDED.kafka], logLevel: logLevel.NOTHING });
  const admin = kafka.admin();
  await admin.connect();
  if (!(await admin.listTopics()).includes(topic)) await admin.createTopics({ topics: [{ topic, numPartitions: 1 }], waitForLeaders: true });
  await admin.disconnect();
  await page.goto(`/topics/${topic}/log`);
  await expect(page.getByTestId("log-state")).toHaveText("Empty");
  await expect(page.getByText("This log has nothing in it")).toBeVisible();
  const cmd = page.locator("pre").filter({ hasText: "kcat -P" });
  await expect(cmd).toContainText(`kcat -P -b `);
  await expect(cmd).toContainText(`-t ${topic}`);
  await expect(page.getByRole("button", { name: "Produce a record" })).toBeVisible();
});

test("Compact now on a compacted topic: a Dialog with the planner's verdict, and its outcome in the broker's words", async ({ page }) => {
  await page.goto("/topics/inventory.levels/log");
  await expect(page.getByTestId("log")).toBeVisible();
  const opener = page.getByTestId("compact-now");
  await opener.click();
  const dialog = page.getByTestId("compact-dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("inventory.levels-0");
  if (await dialog.getByTestId("compact-run").count()) {
    // Due: run the round; the answer is a toast of what it did, or the refusal in the dialog.
    await dialog.getByTestId("compact-run").click();
    await expect(dialog.getByTestId("compact-done").or(dialog.getByTestId("compact-refused"))).toBeVisible();
  } else {
    // Not due: the planner's sentence says why, and nothing runs.
    await expect(dialog.getByTestId("compact-blocked")).toContainText(/planner finds nothing due|cleaner is off|nothing to clean|Lab is switched off/);
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await expect(opener).toBeFocused();
  }
});

test("Producers: the stats, the partition in the address, a producer's batches open in place; a transaction that is not there says so", async ({ page }) => {
  await page.goto("/topics/orders/producers");
  await expect(page.getByTestId("producers")).toBeVisible();
  const strip = page.getByTestId("producers").locator("[data-slot=stat-strip]");
  for (const label of ["Producers", "Open transactions", "Last stable", "High watermark", "Held back"]) await expect(strip).toContainText(label);
  await expect(page.getByTestId("aborted")).toContainText("Aborted transactions remembered:");
  const producer = page.getByTestId("producer").first();
  if (await producer.count()) {
    const toggle = producer.getByTestId("batches-toggle");
    const open = await toggle.getAttribute("aria-expanded");
    await toggle.click();
    await expect(toggle).toHaveAttribute("aria-expanded", open === "true" ? "false" : "true");
    await expect(producer.getByTestId("batches")).toHaveCount(open === "true" ? 0 : 1);
  }
  await page.getByTestId("producers-partition").getByRole("radio", { name: "orders-2" }).click();
  await expect(page).toHaveURL(/partition=2/);
  await expect(page.getByTestId("producers")).toContainText("Producers on orders-2");
  await page.goto("/topics/orders/transactions/no-such-transaction");
  await expect(page.getByRole("heading", { name: "No transaction called no-such-transaction" })).toBeVisible();
  await page.getByRole("link", { name: "Back to the producers" }).click();
  await expect(page).toHaveURL(/\/topics\/orders\/producers$/);
});
