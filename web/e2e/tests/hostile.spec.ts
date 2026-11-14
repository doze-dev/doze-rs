// Hostile text on the React pages (CON-113, CON-211), against a real broker: records whose key, value and headers are markup,
// bidirectional controls and a NUL, under the longest legal topic name, and a group whose id is markup. The API sends them as
// text (crates/kafka-console/tests/live_api_hostile.rs); here the pages show that text as text: no element is made from it,
// no script runs, the controls are written out, and a link to the group is one percent-encoded path segment.
import { Kafka, logLevel } from "kafkajs";
import { expect, test } from "./fixtures";
import { SEEDED } from "../playwright.config";

const KAFKA = process.env.E2E_KAFKA ?? SEEDED.kafka;
const MARKUP = `"><img src=x onerror="window.__pwned=1">`;
const SCRIPT = "</script><script>window.__pwned=2</script>";
const CONTROLS = "bidi ‮txet⁦ nul \u0000 bom ﻿";
let topic = "";
let group = "";

test.beforeAll(async ({}, info) => {
  test.setTimeout(60_000);
  // Every character a name may hold, and 249 bytes of it.
  const head = `r5.hostile_${info.project.name}-`;
  topic = head + "z".repeat(249 - head.length);
  group = `<img src=x onerror=window.__pwned=3> ${info.project.name} /?%`;
  const kafka = new Kafka({ clientId: "r5-hostile", brokers: [KAFKA], logLevel: logLevel.NOTHING });
  const admin = kafka.admin();
  await admin.connect();
  if (!(await admin.listTopics()).includes(topic)) {
    await admin.createTopics({ topics: [{ topic, numPartitions: 1 }], waitForLeaders: true });
    const p = kafka.producer();
    await p.connect();
    for (const s of [MARKUP, SCRIPT, CONTROLS]) await p.send({ topic, messages: [{ key: s, value: s, headers: { [s]: s } }] });
    await p.disconnect();
  }
  await admin.setOffsets({ groupId: group, topic, partitions: [{ partition: 0, offset: "1" }] });
  await admin.disconnect();
});

async function inert(page: import("@playwright/test").Page, where: string) {
  expect(await page.evaluate(() => (window as unknown as { __pwned?: number }).__pwned), `${where}: a script ran`).toBeUndefined();
  await expect(page.locator('img[src="x"]'), `${where}: an element was made from the text`).toHaveCount(0);
  const text = await page.locator("body").innerText();
  for (const c of ["‮", "⁦", "\u0000", "﻿"]) expect(text.includes(c), `${where}: ${JSON.stringify(c)} came through`).toBe(false);
}

test("hostile keys, values, headers and a 249-byte name are text on the topic pages", async ({ page }) => {
  await page.goto(`/topics?q=${encodeURIComponent(`r5.hostile_`)}`);
  await expect(page.getByText(topic).first()).toBeVisible();
  await inert(page, "list");
  await page.goto(`/topics/${topic}/messages`);
  await expect(page.getByText(MARKUP).first()).toBeVisible();
  await expect(page.getByText(SCRIPT).first()).toBeVisible();
  await inert(page, "messages");
  for (const offset of [0, 1, 2]) {
    await page.goto(`/topics/${topic}/messages?record=0:${offset}`);
    const sheet = page.getByRole("dialog");
    await expect(sheet).toBeVisible();
    await expect(sheet).toContainText(offset === 0 ? MARKUP : offset === 1 ? SCRIPT : "⟨U+202E⟩txet⟨U+2066⟩ nul ⟨U+0000⟩");
    await inert(page, `record ${offset}`);
  }
});

test("a group id that is markup is text on its pages, and its links are one encoded segment", async ({ page }) => {
  await page.goto(`/groups?q=${encodeURIComponent("<img")}`);
  const link = page.getByRole("link", { name: group });
  await expect(link).toBeVisible();
  const href = (await link.getAttribute("href")) ?? "";
  expect(href).toContain(encodeURIComponent("<img"));
  expect(href).not.toMatch(/[<> "]/);
  await inert(page, "groups");
  await link.click();
  await expect(page.getByRole("heading", { level: 1 })).toContainText(group);
  for (const tail of ["", "/lag", "/story"]) {
    await page.goto(`/groups/${encodeURIComponent(group)}${tail}`);
    await expect(page.getByRole("heading", { level: 1 })).toContainText(group);
    await inert(page, `group${tail}`);
    for (const a of await page.locator("a[href*='/groups/']").evaluateAll((as) => as.map((x) => x.getAttribute("href") ?? ""))) expect(a, tail).not.toMatch(/[<> "]/);
  }
});
