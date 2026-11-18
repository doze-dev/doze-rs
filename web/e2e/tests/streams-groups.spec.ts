// Streams groups (KIP-1071) on the Groups pages, against a real broker: each browser makes a streams group with one member
// that runs a word count over a topic of its own (raw StreamsGroupHeartbeat, ./streams.ts), and a second group whose input
// topic does not exist. The list names the type and the state, Members shows the topology, the internal topics the broker made,
// the member's tasks and stage, and the not-ready group says why, in the words its members are told.
import { Kafka, logLevel } from "kafkajs";
import { expect, overflowX, test } from "./fixtures";
import { StreamsMember } from "./streams";
import { SEEDED } from "../playwright.config";

const KAFKA = process.env.E2E_KAFKA ?? SEEDED.kafka;
const kafka = new Kafka({ clientId: "streams-e2e", brokers: [KAFKA], logLevel: logLevel.NOTHING, retry: { retries: 5 } });

let member: StreamsMember | undefined;
let lonely: StreamsMember | undefined;
let group = "";
let missing = "";
let input = "";

test.beforeAll(async ({}, info) => {
  test.setTimeout(60_000);
  group = `wordcount-${info.project.name}`;
  missing = `lonely-${info.project.name}`;
  input = `streams-lines-${info.project.name}`;
  const admin = kafka.admin();
  await admin.connect();
  if (!(await admin.listTopics()).includes(input)) await admin.createTopics({ topics: [{ topic: input, numPartitions: 2 }], waitForLeaders: true });
  await admin.disconnect();
  member = new StreamsMember(KAFKA, group, `${group}-thread-1`, input);
  await member.connect();
  await member.join();
  lonely = new StreamsMember(KAFKA, missing, `${missing}-thread-1`, `${missing}-nowhere`);
  await lonely.connect();
  await lonely.join(false);
});

test.afterAll(() => {
  member?.close();
  lonely?.close();
});

test("Groups: a streams group is listed with its type and opens to its topology and tasks", async ({ page }) => {
  await page.goto(`/groups?q=${group}`);
  const row = page.getByRole("row").filter({ has: page.getByTestId("group-link").filter({ hasText: group }) });
  await expect(row).toContainText("streams (KIP-1071)");
  await row.getByTestId("group-link").filter({ hasText: group }).click();
  await expect(page).toHaveURL(new RegExp(`/groups/${group}$`));
  await expect(page.locator("main")).toContainText("streams group (KIP-1071)");
  const g = page.getByTestId("streams-group");
  await expect(g).toBeVisible();
  for (const label of ["Group epoch", "Assignment epoch", "Topology epoch", "Tasks"]) await expect(g).toContainText(label);
  await expect(g.getByTestId("streams-subtopology")).toHaveCount(2);
  await expect(g.getByTestId("streams-subtopology").first()).toContainText(input);
  const internal = g.getByRole("table", { name: "Internal topics" });
  await expect(internal).toContainText(`${group}-words-repartition`);
  await expect(internal).toContainText(`${group}-counts-changelog`);
  await expect(internal).toContainText("made");
  const members = g.getByRole("table", { name: "Members" });
  await expect(members).toContainText(`process-${member!.member}`);
  await expect(members).toContainText("localhost:7070");
  await expect(members).toContainText("0_0–0_1");
  await expect(members).toContainText("In step");
  await expect(g).toContainText("This group, now.");
  expect(await overflowX(page)).toBeLessThanOrEqual(0);
});

test("Groups: a streams group whose input is missing says why it is not ready", async ({ page }) => {
  await page.goto(`/groups?q=${missing}`);
  const row = page.getByRole("row").filter({ has: page.getByTestId("group-link").filter({ hasText: missing }) });
  await expect(row).toContainText("Not ready");
  await page.goto(`/groups/${missing}`);
  const why = page.getByTestId("streams-not-ready");
  await expect(why).toContainText("MISSING_SOURCE_TOPICS");
  await expect(page.getByTestId("streams-not-ready-words")).toContainText(`${missing}-nowhere`);
  await expect(page.getByTestId("streams-group")).toContainText("tasks once ready");
  expect(await overflowX(page)).toBeLessThanOrEqual(0);
});
