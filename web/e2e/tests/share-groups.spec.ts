// Share groups (KIP-932) on the Groups pages, against a real broker: each browser makes its own share group with one member
// that reads a topic of its own, acquires twenty records and acknowledges twelve of them three ways (accept, release,
// reject). The list names the group's type, Members shows the share group's member and epochs, and Lag shows the
// share-partition: where it starts, what is in flight by state, how often it was delivered, and the group's limits.
import { Kafka, logLevel } from "kafkajs";
import { expect, overflowX, test } from "./fixtures";
import { ShareMember } from "./share";
import { SEEDED } from "../playwright.config";

const KAFKA = process.env.E2E_KAFKA ?? SEEDED.kafka;
const kafka = new Kafka({ clientId: "share-e2e", brokers: [KAFKA], logLevel: logLevel.NOTHING, retry: { retries: 5 } });

let member: ShareMember | undefined;
let group = "";

test.beforeAll(async ({}, info) => {
  test.setTimeout(60_000);
  group = `share-${info.project.name}`;
  const topic = `share-orders-${info.project.name}`;
  const admin = kafka.admin();
  await admin.connect();
  if (!(await admin.listTopics()).includes(topic)) await admin.createTopics({ topics: [{ topic, numPartitions: 1 }], waitForLeaders: true });
  await admin.disconnect();
  member = new ShareMember(KAFKA, group, `${group}-member-0000000000000000`.slice(0, 36));
  await member.connect();
  await member.join(topic);
  // The first fetch makes the share-partition, starting at the log's end (share.auto.offset.reset is latest).
  expect((await member.fetch()).error).toBe(0);
  const p = kafka.producer();
  await p.connect();
  await p.send({ topic, messages: Array.from({ length: 20 }, (_, i) => ({ key: `k${i}`, value: JSON.stringify({ i }) })) });
  await p.disconnect();
  let first = -1;
  for (let i = 0; i < 20 && first < 0; i++) {
    const got = await member.fetch();
    if (got.acquired.length > 0) first = got.acquired[0][0];
  }
  expect(first).toBeGreaterThanOrEqual(0);
  // Five accepted (the start moves past them), five released (available again), two rejected (archived).
  expect(await member.acknowledge(first, [1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 3, 3])).toBe(0);
});

test.afterAll(() => member?.close());

test("Groups: a share group is listed with its type and opens to its member", async ({ page }) => {
  await page.goto(`/groups?q=${group}`);
  const row = page.getByRole("row").filter({ has: page.getByTestId("group-link").filter({ hasText: group }) });
  await expect(row).toContainText("share (KIP-932)");
  await row.getByTestId("group-link").filter({ hasText: group }).click();
  await expect(page).toHaveURL(new RegExp(`/groups/${group}$`));
  await expect(page.locator("main")).toContainText("share group (KIP-932)");
  const members = page.getByTestId("share-group");
  await expect(members).toBeVisible();
  for (const label of ["Group epoch", "Assignment epoch", "Assignor", "Shared partitions"]) await expect(members).toContainText(label);
  await expect(members.getByRole("table", { name: "Members" })).toContainText(member!.member);
  await expect(members).toContainText("This group, now.");
  expect(await overflowX(page)).toBeLessThanOrEqual(0);
});

test("Groups: a share group's Lag tab is its share-partitions, with the records in flight by state", async ({ page }) => {
  await page.goto(`/groups/${group}/lag`);
  const parts = page.getByTestId("share-partitions");
  await expect(parts).toBeVisible();
  const table = parts.getByRole("table", { name: `Share-partitions of ${group}` });
  const row = table.getByRole("row").filter({ hasText: `share-orders-` });
  // The five accepted records are behind the start; the rest are in flight: five available again, two archived, and the
  // eight still held (or available, once their 30 s lock has run out).
  await expect(row).toContainText("5");
  const flight = row.getByTestId("share-flight");
  await expect(flight).toContainText("archived");
  await expect(flight).toContainText("available");
  await expect(row).toContainText("× 1");
  await expect(parts.getByTestId("share-explain")).toContainText("A share group commits no offset.");
  const limits = parts.getByTestId("share-limits");
  await expect(limits).toContainText("2 archived now");
  await expect(limits).toContainText("latest");
  await expect(limits).toContainText(`--entity-name ${group}`);
  expect(await overflowX(page)).toBeLessThanOrEqual(0);
});
