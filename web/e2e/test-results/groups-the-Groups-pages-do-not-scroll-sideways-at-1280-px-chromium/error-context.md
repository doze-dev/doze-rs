# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: groups.spec.ts >> the Groups pages do not scroll sideways at 1280 px
- Location: tests/groups.spec.ts:323:3

# Error details

```
"beforeAll" hook timeout of 90000ms exceeded.
```

# Test source

```ts
  1   | // The Groups pages (slice R2), driven with the mouse and the keyboard against real groups on a real broker: the seeded world,
  2   | // or a broker with traffic (`E2E_URL`, with `E2E_KAFKA` its Kafka address). Each browser makes its own groups through the
  3   | // Kafka protocol so the writes it tries never meet another browser's: one that is empty with commits (reset, delete offsets,
  4   | // delete) and one with a member that stays (Kafka's refusals).
  5   | import { Kafka, logLevel, type Consumer } from "kafkajs";
  6   | import { boxes, expect, overflowX, test } from "./fixtures";
  7   | import { SEEDED } from "../playwright.config";
  8   | 
  9   | const KAFKA = process.env.E2E_KAFKA ?? SEEDED.kafka;
  10  | const TOPIC = "r2-orders";
  11  | const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
  12  | const kafka = (clientId: string) => new Kafka({ clientId, brokers: [KAFKA], logLevel: logLevel.NOTHING, retry: { retries: 5 } });
  13  | 
  14  | let busy: Consumer | undefined;
  15  | let idle = "";
  16  | let member = "";
  17  | 
  18  | /** Reads every record of the topic as `group`, committing, then leaves (the group is Empty with commits) or stays. */
  19  | async function consume(group: string, stay: boolean): Promise<Consumer> {
  20  |   const c = kafka(`${group}-app`).consumer({ groupId: group, sessionTimeout: 15_000, heartbeatInterval: 1_000 });
  21  |   await c.connect();
  22  |   await c.subscribe({ topic: TOPIC, fromBeginning: true });
  23  |   let seen = 0;
  24  |   await c.run({ eachMessage: async () => void seen++ });
  25  |   const end = Date.now() + 25_000;
  26  |   while (seen < 30 && Date.now() < end) await sleep(200);
  27  |   await sleep(1_500); // the commit after the last batch
  28  |   if (!stay) await c.disconnect();
  29  |   return c;
  30  | }
  31  | 
> 32  | test.beforeAll(async ({}, info) => {
      |      ^ "beforeAll" hook timeout of 90000ms exceeded.
  33  |   test.setTimeout(90_000);
  34  |   const admin = kafka("r2-admin").admin();
  35  |   await admin.connect();
  36  |   const have = await admin.listTopics();
  37  |   if (!have.includes(TOPIC)) {
  38  |     await admin.createTopics({ topics: [{ topic: TOPIC, numPartitions: 3 }], waitForLeaders: true });
  39  |     const p = kafka("r2-producer").producer();
  40  |     await p.connect();
  41  |     await p.send({ topic: TOPIC, messages: Array.from({ length: 30 }, (_, i) => ({ key: `k${i}`, value: JSON.stringify({ i }) })) });
  42  |     await p.disconnect();
  43  |   }
  44  |   await admin.disconnect();
  45  |   idle = `r2-idle-${info.project.name}`;
  46  |   member = `r2-busy-${info.project.name}`;
  47  |   await consume(idle, false);
  48  |   busy = await consume(member, true);
  49  | });
  50  | 
  51  | test.afterAll(async () => {
  52  |   await busy?.disconnect().catch(() => undefined);
  53  | });
  54  | 
  55  | test("Groups: the counts are tiles, the name filter and the states are in the address, and a row opens its group", async ({ page }) => {
  56  |   await page.goto("/groups");
  57  |   await expect(page.locator("h1")).toHaveText("Groups");
  58  |   const strip = page.locator("[data-slot=stat-strip]");
  59  |   for (const label of ["Groups", "Stable", "Rebalancing", "Empty"]) await expect(strip).toContainText(label);
  60  |   await expect(page.getByTestId("groups-attention")).toBeVisible();
  61  |   // The name filter: typed, it is in the address and the rows contain it.
  62  |   await page.getByTestId("groups-filter").fill("r2-");
  63  |   await expect(page).toHaveURL(/q=r2-/);
  64  |   const links = page.getByTestId("group-link");
  65  |   await expect(links.filter({ hasText: idle })).toBeVisible();
  66  |   for (const t of await links.allInnerTexts()) expect(t).toContain("r2-");
  67  |   // The states: a MultiSelect with how many groups each holds; Empty keeps only the empty ones.
  68  |   await page.getByRole("combobox", { name: "States" }).click();
  69  |   const empty = page.getByRole("option", { name: /Empty/ });
  70  |   await expect(empty).toContainText(/\d/);
  71  |   await empty.click();
  72  |   await page.keyboard.press("Escape");
  73  |   await expect(page).toHaveURL(/state=/);
  74  |   await expect(page.getByTestId("multi-summary")).toHaveText("Empty");
  75  |   await expect(links.filter({ hasText: idle })).toBeVisible();
  76  |   await expect(links.filter({ hasText: member })).toHaveCount(0);
  77  |   for (const row of await page.locator("tbody tr").all()) await expect(row.getByTestId("group-phase")).toHaveText("Empty");
  78  |   // Back undoes the filter (replace keeps history short: Back leaves the page's first state).
  79  |   await page.reload();
  80  |   await expect(page.getByTestId("multi-summary")).toHaveText("Empty");
  81  |   await expect(page.getByTestId("groups-filter")).toHaveValue("r2-");
  82  |   await page.getByRole("button", { name: "Clear states" }).click();
  83  |   await expect(page).not.toHaveURL(/state=/);
  84  |   await expect(links.filter({ hasText: member })).toBeVisible();
  85  |   await links.filter({ hasText: idle }).click();
  86  |   await expect(page).toHaveURL(new RegExp(`/groups/${idle}$`));
  87  |   await expect(page.locator("h1")).toHaveText(idle);
  88  | });
  89  | 
  90  | test("A group: the state, the figures as tiles, the tabs are routes, and Refresh moves nothing", async ({ page }) => {
  91  |   await page.goto(`/groups/${member}`);
  92  |   await expect(page.locator("h1")).toHaveText(member);
  93  |   await expect(page.locator("h1")).toHaveCount(1);
  94  |   await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toHaveText("Groups");
  95  |   await expect(page.getByTestId("group-pills")).toContainText("Stable");
  96  |   const strip = page.locator("[data-slot=stat-strip]").first();
  97  |   for (const label of ["State", "Members", "Generation", "Lag", "Committed", "Last commit"]) await expect(strip).toContainText(label);
  98  |   await expect(page.getByTestId("member-link")).toHaveCount(1);
  99  |   await expect(page.getByTestId("wire-dock")).toBeVisible();
  100 |   const tabs = page.getByTestId("group-tabs");
  101 |   for (const [name, path] of [["Lag", "/lag"], ["Story", "/story"], ["Wire", "/wire"], ["Members", ""]] as const) {
  102 |     await tabs.getByRole("link", { name: new RegExp(`^${name}`) }).click();
  103 |     await expect(page).toHaveURL(new RegExp(`/groups/${member}${path}$`));
  104 |     await expect(tabs.getByRole("link", { name: new RegExp(`^${name}`) })).toHaveAttribute("aria-current", "page");
  105 |   }
  106 |   await page.goBack();
  107 |   await expect(page).toHaveURL(new RegExp(`/groups/${member}/wire$`));
  108 |   await page.goBack();
  109 |   await expect(page).toHaveURL(new RegExp(`/groups/${member}/story$`));
  110 |   await expect(page.getByTestId("generation").first()).toBeVisible();
  111 |   // Refresh: the data is fetched again and nothing on the page moves.
  112 |   const selectors = ["h1", "[data-slot=stat-strip]", "[data-testid=group-tabs]", "[data-testid=story-timeline]"];
  113 |   const before = await boxes(page, selectors);
  114 |   const stamp = page.getByTestId("refresh-stamp");
  115 |   await page.waitForTimeout(1_100);
  116 |   const first = await stamp.textContent();
  117 |   await page.getByTestId("refresh-now").click();
  118 |   await expect(stamp).not.toHaveText(first ?? "");
  119 |   expect(await boxes(page, selectors)).toEqual(before);
  120 | });
  121 | 
  122 | test("A member opens as a Sheet with its own address: Esc closes it, the focus returns, Back and a deep link work", async ({ page }) => {
  123 |   await page.goto(`/groups/${member}`);
  124 |   const link = page.getByTestId("member-link").first();
  125 |   await link.click();
  126 |   const sheet = page.getByTestId("member-sheet");
  127 |   await expect(sheet).toBeVisible();
  128 |   await expect(page).toHaveURL(/member=/);
  129 |   await expect(sheet).toContainText(`${member}-app`);
  130 |   await expect(sheet.getByTestId("member-holds")).toContainText(TOPIC);
  131 |   await page.keyboard.press("Escape");
  132 |   await expect(sheet).toBeHidden();
```