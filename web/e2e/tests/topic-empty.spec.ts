// Empty a topic from its page (CON-1590, CON-1591), against the real binary: records written through the Kafka protocol to
// three partitions and a group's commits below their ends; the dialog opened from the topic's actions and from the palette
// says what goes per partition and who falls behind; the name typed; one DeleteRecords; the group moved to the new start;
// the topic carries on. A topic that only compacts is refused before the button, as Kafka refuses it.
import { Kafka, logLevel } from "kafkajs";
import { SEEDED } from "../playwright.config";
import { expect, test } from "./fixtures";

const KAFKA = process.env.E2E_KAFKA ?? SEEDED.kafka;

async function world(topic: string, compact = false) {
  const k = new Kafka({ clientId: "e2e-empty", brokers: [KAFKA], logLevel: logLevel.NOTHING });
  const a = k.admin();
  await a.connect();
  await a.createTopics({ topics: [{ topic, numPartitions: 3, configEntries: compact ? [{ name: "cleanup.policy", value: "compact" }] : [] }] });
  const p = k.producer({ allowAutoTopicCreation: false });
  await p.connect();
  for (const partition of [0, 1, 2]) await p.send({ topic, messages: Array.from({ length: 4 + partition }, (_, i) => ({ partition, key: `k-${partition}-${i}`, value: `v${i}` })) });
  await p.disconnect();
  if (!compact) await a.setOffsets({ groupId: `${topic}-readers`, topic, partitions: [{ partition: 0, offset: "1" }, { partition: 2, offset: "6" }] });
  await a.disconnect();
}

test("Empty from the topic's actions: what goes per partition, the group below moved to the new start, the topic carries on", async ({ page }) => {
  const topic = `e2e-empty-${Date.now().toString(36)}`;
  await world(topic);
  await page.goto(`/topics/${topic}`);
  await page.getByTestId("act-empty").click();
  await expect(page).toHaveURL(/dialog=empty/);
  const d = page.getByTestId("dialog-empty");
  await expect(d).toBeVisible();
  await expect(d.getByTestId("empty-what")).toContainText("Removes 15 records");
  await expect(d.getByTestId("empty-part")).toHaveCount(3);
  await expect(d.getByTestId("empty-part").nth(1)).toContainText("5");
  // The group's commit on partition 0 is below the end (3 unread); its commit on 2 is at the end.
  await expect(d.getByTestId("empty-groups")).toContainText(`${topic}-readers (3 unread on 1 partition)`);
  await expect(d.getByTestId("empty-groups")).toContainText("OFFSET_OUT_OF_RANGE");
  const box = d.getByTestId("empty-group");
  await expect(box).toHaveAttribute("data-state", "checked");
  await expect(d.getByTestId("the-call")).toContainText("offset: -1");
  // The name, typed: the button waits for it.
  const go = d.getByTestId("empty-submit");
  await expect(go).toBeDisabled();
  await d.getByTestId("empty-confirm").fill(topic.slice(0, -1));
  await expect(go).toBeDisabled();
  await d.getByTestId("empty-confirm").fill(topic);
  await expect(go).toBeEnabled();
  await go.click();
  await expect(d.getByTestId("emptied")).toContainText(`${topic} is empty: 15 records are no longer served`);
  await expect(d.getByTestId("emptied")).toContainText(`${topic}-1 at 5`);
  await expect(d.getByTestId("emptied-moved")).toContainText(/moved to the new start \(\d+ partitions?\)/);
  await d.getByTestId("write-done").click();
  await expect(d).toHaveCount(0);
  await expect(page).not.toHaveURL(/dialog=/);
  // Again: nothing to empty, and no group below.
  await page.getByTestId("act-empty").click();
  await expect(page.getByTestId("empty-what")).toContainText("Nothing to remove");
  await expect(page.getByTestId("empty-submit")).toBeDisabled();
  await expect(page.getByTestId("empty-groups")).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("dialog-empty")).toHaveCount(0);
});

test("Empty from the palette; a topic that only compacts is refused before the button and pointed at the Data page", async ({ page }) => {
  const topic = `e2e-levels-${Date.now().toString(36)}`;
  await world(topic, true);
  await page.goto("/");
  await page.getByTestId("palette-trigger").click();
  await page.getByTestId("palette-input").fill(topic);
  await page.getByTestId("palette-empty").click();
  await expect(page).toHaveURL(new RegExp(`/topics/${topic}\\?dialog=empty`));
  const d = page.getByTestId("dialog-empty");
  await expect(d.getByTestId("refusal")).toContainText("POLICY_VIOLATION");
  await expect(d.getByTestId("empty-refused")).toContainText("only compact");
  await expect(d.getByTestId("empty-confirm")).toBeDisabled();
  await expect(d.getByTestId("empty-submit")).toBeDisabled();
  await d.getByRole("link", { name: "Empty it on the Data page" }).click();
  await expect(page).toHaveURL(new RegExp(`/data\\?dialog=empty&topic=${topic}`));
});
