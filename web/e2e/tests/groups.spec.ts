// The Groups pages (slice R2), driven with the mouse and the keyboard against real groups on a real broker: the seeded world,
// or a broker with traffic (`E2E_URL`, with `E2E_KAFKA` its Kafka address). Each browser makes its own groups through the
// Kafka protocol so the writes it tries never meet another browser's: one that is empty with commits (reset, delete offsets,
// delete) and one with a member that stays (Kafka's refusals).
import { Kafka, logLevel, type Consumer } from "kafkajs";
import { boxes, expect, overflowX, test } from "./fixtures";
import { SEEDED } from "../playwright.config";

const KAFKA = process.env.E2E_KAFKA ?? SEEDED.kafka;
const TOPIC = "r2-orders";
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const kafka = (clientId: string) => new Kafka({ clientId, brokers: [KAFKA], logLevel: logLevel.NOTHING, retry: { retries: 5 } });

let busy: Consumer | undefined;
let idle = "";
let member = "";

/** Reads every record of the topic as `group`, committing, then leaves (the group is Empty with commits) or stays. */
async function consume(group: string, stay: boolean): Promise<Consumer> {
  const c = kafka(`${group}-app`).consumer({ groupId: group, sessionTimeout: 15_000, heartbeatInterval: 1_000 });
  await c.connect();
  await c.subscribe({ topic: TOPIC, fromBeginning: true });
  let seen = 0;
  await c.run({ eachMessage: async () => void seen++ });
  const end = Date.now() + 25_000;
  while (seen < 30 && Date.now() < end) await sleep(200);
  await sleep(1_500); // the commit after the last batch
  if (!stay) await c.disconnect();
  return c;
}

test.beforeAll(async ({}, info) => {
  test.setTimeout(90_000);
  const admin = kafka("r2-admin").admin();
  await admin.connect();
  const have = await admin.listTopics();
  if (!have.includes(TOPIC)) {
    await admin.createTopics({ topics: [{ topic: TOPIC, numPartitions: 3 }], waitForLeaders: true });
    const p = kafka("r2-producer").producer();
    await p.connect();
    await p.send({ topic: TOPIC, messages: Array.from({ length: 30 }, (_, i) => ({ key: `k${i}`, value: JSON.stringify({ i }) })) });
    await p.disconnect();
  }
  await admin.disconnect();
  idle = `r2-idle-${info.project.name}`;
  member = `r2-busy-${info.project.name}`;
  await consume(idle, false);
  busy = await consume(member, true);
});

test.afterAll(async () => {
  await busy?.disconnect().catch(() => undefined);
});

test("Groups: the counts are tiles, the name filter and the states are in the address, and a row opens its group", async ({ page }) => {
  await page.goto("/groups");
  await expect(page.locator("h1")).toHaveText("Groups");
  const strip = page.locator("[data-slot=stat-strip]");
  for (const label of ["Groups", "Stable", "Rebalancing", "Empty"]) await expect(strip).toContainText(label);
  await expect(page.getByTestId("groups-attention")).toBeVisible();
  // The name filter: typed, it is in the address and the rows contain it.
  await page.getByTestId("groups-filter").fill("r2-");
  await expect(page).toHaveURL(/q=r2-/);
  const links = page.getByTestId("group-link");
  await expect(links.filter({ hasText: idle })).toBeVisible();
  for (const t of await links.allInnerTexts()) expect(t).toContain("r2-");
  // The states: a MultiSelect with how many groups each holds; Empty keeps only the empty ones.
  await page.getByRole("combobox", { name: "States" }).click();
  const empty = page.getByRole("option", { name: /Empty/ });
  await expect(empty).toContainText(/\d/);
  await empty.click();
  await page.keyboard.press("Escape");
  await expect(page).toHaveURL(/state=/);
  await expect(page.getByTestId("multi-summary")).toHaveText("Empty");
  await expect(links.filter({ hasText: idle })).toBeVisible();
  await expect(links.filter({ hasText: member })).toHaveCount(0);
  for (const row of await page.locator("tbody tr").all()) await expect(row.getByTestId("group-phase")).toHaveText("Empty");
  // Back undoes the filter (replace keeps history short: Back leaves the page's first state).
  await page.reload();
  await expect(page.getByTestId("multi-summary")).toHaveText("Empty");
  await expect(page.getByTestId("groups-filter")).toHaveValue("r2-");
  await page.getByRole("button", { name: "Clear states" }).click();
  await expect(page).not.toHaveURL(/state=/);
  await expect(links.filter({ hasText: member })).toBeVisible();
  await links.filter({ hasText: idle }).click();
  await expect(page).toHaveURL(new RegExp(`/groups/${idle}$`));
  await expect(page.locator("h1")).toHaveText(idle);
});

test("A group: the state, the figures as tiles, the tabs are routes, and Refresh moves nothing", async ({ page }) => {
  await page.goto(`/groups/${member}`);
  await expect(page.locator("h1")).toHaveText(member);
  await expect(page.locator("h1")).toHaveCount(1);
  await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toHaveText("Groups");
  await expect(page.getByTestId("group-pills")).toContainText("Stable");
  const strip = page.locator("[data-slot=stat-strip]").first();
  for (const label of ["State", "Members", "Generation", "Lag", "Committed", "Last commit"]) await expect(strip).toContainText(label);
  await expect(page.getByTestId("member-link")).toHaveCount(1);
  await expect(page.getByTestId("wire-dock")).toBeVisible();
  const tabs = page.getByTestId("group-tabs");
  for (const [name, path] of [["Lag", "/lag"], ["Story", "/story"], ["Wire", "/wire"], ["Members", ""]] as const) {
    await tabs.getByRole("link", { name: new RegExp(`^${name}`) }).click();
    await expect(page).toHaveURL(new RegExp(`/groups/${member}${path}$`));
    await expect(tabs.getByRole("link", { name: new RegExp(`^${name}`) })).toHaveAttribute("aria-current", "page");
  }
  await page.goBack();
  await expect(page).toHaveURL(new RegExp(`/groups/${member}/wire$`));
  await page.goBack();
  await expect(page).toHaveURL(new RegExp(`/groups/${member}/story$`));
  await expect(page.getByTestId("generation").first()).toBeVisible();
  // Refresh: the data is fetched again and nothing on the page moves.
  const selectors = ["h1", "[data-slot=stat-strip]", "[data-testid=group-tabs]", "[data-testid=story-timeline]"];
  const before = await boxes(page, selectors);
  const stamp = page.getByTestId("refresh-stamp");
  await page.waitForTimeout(1_100);
  const first = await stamp.textContent();
  await page.getByTestId("refresh-now").click();
  await expect(stamp).not.toHaveText(first ?? "");
  expect(await boxes(page, selectors)).toEqual(before);
});

test("A member opens as a Sheet with its own address: Esc closes it, the focus returns, Back and a deep link work", async ({ page }) => {
  await page.goto(`/groups/${member}`);
  const link = page.getByTestId("member-link").first();
  await link.click();
  const sheet = page.getByTestId("member-sheet");
  await expect(sheet).toBeVisible();
  await expect(page).toHaveURL(/member=/);
  await expect(sheet).toContainText(`${member}-app`);
  await expect(sheet.getByTestId("member-holds")).toContainText(TOPIC);
  await page.keyboard.press("Escape");
  await expect(sheet).toBeHidden();
  await expect(page).not.toHaveURL(/member=/);
  await expect(link).toBeFocused();
  // Opened again, Back closes it.
  await link.click();
  await expect(sheet).toBeVisible();
  const url = page.url();
  await page.goBack();
  await expect(sheet).toBeHidden();
  // A deep link opens the page with the Sheet on it; closing it stays on the page.
  await page.goto(url);
  await expect(sheet).toBeVisible();
  await sheet.getByRole("button", { name: "Close" }).first().click();
  await expect(sheet).toBeHidden();
  await expect(page).toHaveURL(new RegExp(`/groups/${member}$`));
});

test("Lag: one table per topic with the lag as a bar, and when the commits expire as a choice in the address", async ({ page }) => {
  await page.goto(`/groups/${idle}/lag`);
  const block = page.getByTestId("lag-topic").filter({ has: page.getByRole("link", { name: TOPIC }) });
  await expect(block).toBeVisible();
  await expect(block.locator("tbody tr")).toHaveCount(3);
  await expect(block.getByTestId("topic-behind")).toHaveText("0 behind");
  await expect(page.getByTestId("lag-explain")).toBeVisible();
  await page.getByTestId("lag-view").getByRole("radio", { name: "When do these expire?" }).click();
  await expect(page).toHaveURL(/expiring=1/);
  await expect(page.getByText("Retention window")).toBeVisible();
  await expect(block.getByRole("columnheader", { name: "Forgotten at" })).toBeVisible();
  await expect(page.getByTestId("lag-explain")).toHaveAttribute("data-state", "expiring");
  await page.goBack();
  await expect(page).not.toHaveURL(/expiring=1/);
  await expect(block.getByRole("columnheader", { name: "Lag" })).toBeVisible();
});

test("Story: the generations newest first; a rebalance opens as a Sheet, the flapping rule as another", async ({ page }) => {
  await page.goto(`/groups/${member}/story`);
  const gens = page.getByTestId("generation");
  await expect(gens.first()).toContainText("Member joined");
  await expect(page.locator("[data-slot=stat-strip]").nth(1)).toContainText("Rebalances kept");
  const open = page.getByTestId("open-rebalance").first();
  await open.click();
  const sheet = page.getByTestId("rebalance-sheet");
  await expect(sheet).toBeVisible();
  await expect(page).toHaveURL(/generation=\d+/);
  await expect(sheet).toContainText("Began");
  await expect(sheet).toContainText("Members");
  await page.keyboard.press("Escape");
  await expect(sheet).toBeHidden();
  await expect(page).not.toHaveURL(/generation=/);
  await expect(open).toBeFocused();
  // A quiet group's page says so; one with more than one rebalance has the flapping card.
  const rule = page.getByTestId("check-rule");
  if (await rule.count()) {
    await rule.click();
    await expect(page.getByTestId("rule-sheet")).toBeVisible();
    await expect(page).toHaveURL(/rule=1/);
    await page.goBack();
    await expect(page.getByTestId("rule-sheet")).toBeHidden();
  } else await expect(page.getByTestId("story-quiet")).toContainText("A quiet story is a good one");
});

test("Wire: the Live slice's dock of the group, filtered by kind and by text", async ({ page }) => {
  await page.goto(`/groups/${member}/wire`);
  await expect(page.getByTestId("group-tabs").getByRole("link", { name: /^Wire/ })).toHaveAttribute("aria-current", "page");
  const dock = page.getByTestId("wire-dock");
  await expect(dock).toHaveAttribute("data-group", member);
  const table = dock.getByRole("table", { name: "Requests that touched this group" });
  await expect(table.locator("tbody tr").first()).toBeVisible();
  const kinds = dock.getByTestId("group-wire-kind");
  await kinds.getByRole("radio", { name: /^Commits/ }).click();
  await expect(kinds.getByRole("radio", { name: /^Commits/ })).toHaveAttribute("aria-checked", "true");
  await expect(table.locator("tbody tr").first()).toContainText(/Offset/);
  await kinds.getByRole("radio", { name: /^All/ }).click();
  await dock.getByLabel("Filter by member or client").fill(`${member}-app`);
  await expect(table.locator("tbody tr").first()).toContainText(`${member}-app`);
});

test("Reset offsets: a GET preview, then the reset; the Lag tab shows where the group now reads", async ({ page }) => {
  await page.goto(`/groups/${idle}/lag`);
  await page.getByTestId("open-reset").click();
  const dialog = page.getByTestId("dialog-reset");
  await expect(dialog).toBeVisible();
  await expect(page).toHaveURL(/dialog=reset/);
  await expect(dialog).toContainText("has no members");
  // A value that is not a number: the preview says so in the console's words, and Reset is off.
  await dialog.getByRole("radio", { name: /A specific offset/ }).click();
  await dialog.getByTestId("reset-value").fill("abc");
  await dialog.getByTestId("reset-preview-button").click();
  await expect(dialog.getByTestId("reset-error")).toHaveText("The offset must be a whole number.");
  await expect(dialog.getByTestId("reset-submit")).toBeDisabled();
  // A time, chosen with the picker (never the browser's own widget): a day of the grid and a time typed.
  await dialog.getByRole("radio", { name: /A time/ }).click();
  await dialog.getByTestId("datetime-trigger").click();
  const picker = page.getByTestId("datetime-popover");
  await picker.getByRole("button", { name: /^1 / }).click();
  await picker.getByTestId("datetime-clock").fill("00:00:00");
  await picker.getByTestId("datetime-done").click();
  await expect(dialog.getByTestId("datetime-trigger")).toContainText(/\d{4}-\d{2}-01 00:00:00 UTC/);
  await dialog.getByTestId("reset-preview-button").click();
  await expect(dialog.getByTestId("reset-error")).toHaveCount(0);
  // Earliest: every partition goes to its log start.
  await dialog.getByRole("radio", { name: /Earliest/ }).click();
  await expect(dialog.getByTestId("reset-submit")).toBeDisabled();
  await dialog.getByTestId("reset-preview-button").click();
  await expect(dialog.getByTestId("reset-after")).toHaveText(["0", "0", "0"]);
  await expect(dialog.getByTestId("reset-submit")).toHaveText(/Reset 3 offsets/);
  await dialog.getByTestId("reset-submit").click();
  await expect(page.locator("[data-sonner-toast]").filter({ hasText: "Reset 3 offsets" })).toBeVisible();
  await expect(dialog).toBeHidden();
  await expect(page).not.toHaveURL(/dialog=/);
  const block = page.getByTestId("lag-topic").filter({ has: page.getByRole("link", { name: TOPIC }) });
  await expect(block.locator("tbody tr td:nth-child(3)")).toHaveText(["0", "0", "0"]);
  await expect(block.getByTestId("topic-behind")).not.toHaveText("0 behind");
  // Back to the end, with the keyboard: Esc cancels, Enter in the dialog previews.
  await page.getByTestId("open-reset").click();
  await expect(dialog).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await page.getByTestId("open-reset").click();
  await dialog.getByRole("radio", { name: /Latest/ }).click();
  await dialog.getByTestId("reset-preview-button").click();
  await dialog.getByTestId("reset-submit").click();
  await expect(block.getByTestId("topic-behind")).toHaveText("0 behind");
});

test("Reset offsets of a group with a member is refused, in Kafka's name and words, and nothing changes", async ({ page }) => {
  await page.goto(`/groups/${member}?dialog=reset`);
  const dialog = page.getByTestId("dialog-reset");
  await expect(dialog.getByTestId("reset-not-empty")).toContainText("This group is not empty.");
  await dialog.getByRole("radio", { name: /Earliest/ }).click();
  await dialog.getByTestId("reset-preview-button").click();
  await expect(dialog.getByTestId("reset-after").first()).toHaveText("0");
  await dialog.getByTestId("reset-submit").click();
  const refusal = dialog.getByTestId("refusal");
  await expect(refusal).toBeVisible();
  await expect(refusal.getByTestId("refusal-code")).toHaveText(/^[A-Z_]+ \(\d+\)$/);
  await expect(refusal.getByTestId("refusal-words")).toHaveText(/^".+"$/);
  await expect(dialog).toContainText("What would pass");
  await expect(dialog).toContainText("The call that was refused");
  await dialog.getByTestId("dialog-cancel").click();
  await expect(dialog).toBeHidden();
  await page.getByTestId("group-tabs").getByRole("link", { name: /^Lag/ }).click();
  await expect(page.getByTestId("lag-topic").first().getByTestId("topic-behind")).toHaveText("0 behind");
});

test("Delete group with a member is refused as NON_EMPTY_GROUP in Kafka's words", async ({ page }) => {
  await page.goto(`/groups/${member}`);
  await page.getByTestId("open-delete").click();
  const dialog = page.getByTestId("dialog-delete");
  await expect(dialog).toContainText(`Delete ${member}?`);
  await expect(dialog.getByTestId("delete-not-empty")).toContainText("NON_EMPTY_GROUP");
  await dialog.getByTestId("delete-submit").click();
  await expect(dialog.getByTestId("refusal-code")).toHaveText("NON_EMPTY_GROUP (68)");
  await expect(dialog.getByTestId("refusal-words")).toHaveText('"The group is not empty."');
  await expect(dialog).toContainText("Every member has to leave");
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await expect(page.getByTestId("dialog-cancel")).toHaveCount(0);
  await expect(page.getByTestId("open-delete")).toBeFocused();
  await expect(page.locator("h1")).toHaveText(member);
});

test("Delete offsets forgets the commits of the topics ticked; Delete group then removes the empty group", async ({ page }) => {
  await page.goto(`/groups/${idle}/lag`);
  await page.getByTestId("open-delete-offsets").click();
  const dialog = page.getByTestId("dialog-delete-offsets");
  const box = dialog.getByTestId("offsets-topic");
  await expect(box).toHaveCount(1);
  await expect(box).toBeChecked();
  await box.click();
  await expect(dialog.getByTestId("offsets-submit")).toBeDisabled();
  await dialog.getByText(TOPIC, { exact: true }).click();
  await expect(box).toBeChecked();
  await expect(dialog.getByTestId("offsets-submit")).toHaveText(/Delete 3 offsets/);
  await dialog.getByTestId("offsets-submit").click();
  await expect(page.locator("[data-sonner-toast]").filter({ hasText: "Deleted 3 committed offsets" })).toBeVisible();
  await expect(dialog).toBeHidden();
  await page.getByTestId("open-delete").click();
  const del = page.getByTestId("dialog-delete");
  await expect(del).toContainText(`Delete ${idle}?`);
  await del.getByTestId("delete-submit").click();
  await expect(page).toHaveURL(/\/groups$/);
  await expect(page.locator("[data-sonner-toast]").filter({ hasText: `Deleted ${idle}.` })).toBeVisible();
  await expect(page.getByTestId("group-link").filter({ hasText: idle })).toHaveCount(0);
  // The API now says there is no such group (a page that asked would log the 404 in Safari).
  const gone = await page.request.get(`/api/v1/groups/${idle}`, { headers: { "X-Console": "1" } });
  expect(gone.status()).toBe(404);
  expect((await gone.json()).code).toBe("GROUP_NOT_FOUND");
});

for (const width of [1024, 1280, 1440, 1920]) {
  test(`the Groups pages do not scroll sideways at ${width} px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    for (const path of ["/groups", `/groups/${member}`, `/groups/${member}/lag`, `/groups/${member}/lag?expiring=1`, `/groups/${member}/story`, `/groups/${member}/wire`, `/groups/${member}?dialog=reset`, `/groups/${member}/story?rule=1`]) {
      await page.goto(path);
      await expect(page.locator("h1")).toBeVisible();
      await page.waitForLoadState("networkidle");
      expect(await overflowX(page), `${path} at ${width}`).toBeLessThanOrEqual(0);
    }
  });
}
