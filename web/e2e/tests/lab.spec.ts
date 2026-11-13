// The Lab (slice R4) against the real binary with traffic: a Slow rule for one client slows its requests and counts them; a Fail
// rule on Produce answers the client with the code; switch off and on; remove; the rehearsals that happen once; Compact now.
import { choose, expect, test } from "./fixtures";
import { apiGet, apiPost, kafkaAddress, noMemory, noNativeSelect, traffic, widths } from "./r4";

test.describe.configure({ mode: "serial" });

/** Every rule in force removed and the history cleared, through the API the page uses. */
async function clean(page: import("@playwright/test").Page, baseURL: string) {
  const lab = await apiGet(page.request, "/api/v1/lab");
  for (const r of lab.home.active) await apiPost(page.request, baseURL, `/api/v1/lab/rules/${r.id}/remove`, {});
  await apiPost(page.request, baseURL, "/api/v1/lab/history/clear", {});
}

test("Lab: the empty state, the six rehearsals, no native control, no overflow", async ({ page, baseURL }) => {
  await clean(page, baseURL!);
  await page.goto("/lab");
  await expect(page.locator("h1")).toHaveText("Lab");
  await expect(page.getByTestId("lab-home")).toHaveAttribute("data-state", "empty");
  await expect(page.getByText("Nothing is bent")).toBeVisible();
  await expect(page.getByTestId("rehearsal-cards").locator("button")).toHaveCount(6);
  await noMemory(page);
  await noNativeSelect(page);
  await widths(page);
});

test("Lab: a Slow rule for one client slows its requests; the rule counts them; the sheet lists them with the lab tag", async ({ page, baseURL, browserName }) => {
  const client = `lab-slow-${browserName}`;
  const app = await traffic(await kafkaAddress(page.request), client);
  try {
    await expect.poll(() => app.sent(), { timeout: 15_000 }).toBeGreaterThan(2);
    await page.goto("/lab");
    const start = page.getByTestId("lab-new");
    await start.click();
    await expect(page).toHaveURL(/new=slow/);
    const dialog = page.getByTestId("lab-dialog");
    const form = dialog.getByTestId("lab-form-slow");
    await expect(form).toBeVisible();
    // The client, from the apps with a connection open (Combobox, mouse).
    await form.getByRole("combobox", { name: "Client" }).click();
    await page.getByRole("option", { name: client, exact: true }).click();
    await expect(form.getByRole("combobox", { name: "Client" })).toHaveText(client);
    // Topics (MultiSelect).
    await form.getByRole("combobox", { name: "Topics" }).click();
    await page.getByRole("option", { name: /^orders/ }).click();
    await page.keyboard.press("Escape");
    await expect(form.getByTestId("multi-chips").filter({ hasText: "orders" })).toBeVisible();
    await expect(form.getByTestId("multi-chips").filter({ hasText: "Produce" })).toBeVisible();
    await form.getByTestId("lab-delay").fill("300");
    await choose(page, form.getByTestId("lab-share"), "Every request");
    await form.getByTestId("lab-ends").getByRole("radio", { name: "5 min" }).click();
    await noNativeSelect(page);
    // The server's preview says what it will do.
    await expect(dialog.getByTestId("lab-preview-sentence")).toContainText(`+300 ms on every Produce request to orders from ${client}`);
    await dialog.getByTestId("lab-start").click();
    // Made: the dialog gives way to the rule's own sheet.
    const sheet = page.getByTestId("lab-rule-sheet");
    await expect(sheet).toBeVisible();
    await expect(page).toHaveURL(/rule=\d+/);
    await expect(sheet).toContainText("Slow produce to orders");
    // Its hits grow as the app writes; the requests on the sheet carry the tag and took the delay.
    await expect
      .poll(
        async () => {
          await page.getByTestId("refresh-now").click({ force: true }).catch(() => {});
          const lab = await apiGet(page.request, "/api/v1/lab");
          return lab.home.active.find((r: { name: string }) => r.name === "Slow produce to orders")?.hits ?? 0;
        },
        { timeout: 20_000 },
      )
      .toBeGreaterThan(1);
    await page.reload();
    await expect(sheet.getByRole("table", { name: "Requests the rule touched" }).locator("tbody tr").first()).toBeVisible();
    await expect(sheet.getByTestId("lab-badge").first()).toBeVisible();
    const took = await sheet.getByRole("table", { name: "Requests the rule touched" }).locator("tbody tr").first().locator("td").nth(5).innerText();
    expect(Number(took.replace(/[^\d]/g, ""))).toBeGreaterThanOrEqual(300);
    await expect(sheet.getByTestId("tile-remaining")).toContainText(/\d:\d\d/);
    // The page's table says the same rule, its hits and its last hit as an absolute time.
    await page.keyboard.press("Escape");
    await expect(sheet).toBeHidden();
    const row = page.getByRole("table", { name: "Rules in force" }).locator("tbody tr", { hasText: "Slow produce to orders" });
    await expect(row).toBeVisible();
    await expect(row.locator("td").nth(6)).toHaveText(/\d\d:\d\d:\d\d/);

    // Switch it off and on with the Switch.
    const sw = row.getByTestId("rule-switch");
    await sw.click();
    await expect(sw).toHaveAttribute("aria-checked", "false");
    await expect.poll(async () => (await apiGet(page.request, "/api/v1/lab")).home.active.find((r: { name: string }) => r.name === "Slow produce to orders")?.enabled).toBe(false);
    await sw.click();
    await expect.poll(async () => (await apiGet(page.request, "/api/v1/lab")).home.active.find((r: { name: string }) => r.name === "Slow produce to orders")?.enabled).toBe(true);

    // Remove, with the confirm dialog; it moves to Ended.
    await row.getByTestId("rule-remove").click();
    await expect(page).toHaveURL(/remove=\d+/);
    await page.getByTestId("lab-remove-confirm").click();
    await expect(page.getByTestId("lab-remove-dialog")).toBeHidden();
    await expect(page.getByRole("table", { name: "Rules in force" }).locator("tbody tr", { hasText: "Slow produce to orders" })).toHaveCount(0);
    await expect(page.getByRole("table", { name: "Rules that ended" })).toContainText("Slow produce to orders");
    await expect(page.getByRole("table", { name: "Rules that ended" })).toContainText("Removed");
  } finally {
    await app.stop();
  }
});

test("Lab: a Fail rule on Produce answers the producer with the code it names", async ({ page, baseURL, browserName }) => {
  await clean(page, baseURL!);
  const client = `lab-fail-${browserName}`;
  const app = await traffic(await kafkaAddress(page.request), client, "payments.settled", 200);
  try {
    await expect.poll(() => app.sent(), { timeout: 15_000 }).toBeGreaterThan(1);
    await page.goto("/lab?new=fail");
    const form = page.getByTestId("lab-form-fail");
    await expect(form).toBeVisible();
    // APIs: Fetch out, Produce in (MultiSelect by mouse).
    await form.getByRole("combobox", { name: "APIs" }).click();
    await page.getByRole("option", { name: "Fetch" }).click();
    await page.getByRole("option", { name: "Produce" }).click();
    await page.keyboard.press("Escape");
    await expect(form.getByRole("combobox", { name: "APIs" })).toHaveText("Produce");
    await form.getByRole("combobox", { name: "Client" }).click();
    await page.getByRole("option", { name: client, exact: true }).click();
    await choose(page, form.getByTestId("lab-code"), "NOT_LEADER_OR_FOLLOWER (6)");
    await choose(page, form.getByTestId("lab-share"), "Every request");
    await expect(page.getByTestId("lab-preview-sentence")).toContainText("NOT_LEADER_OR_FOLLOWER");
    await page.getByTestId("lab-start").click();
    await expect(page.getByTestId("lab-rule-sheet")).toBeVisible();
    // The producer gets the code back (kafkajs names it by Kafka's name).
    await expect.poll(() => app.errors.filter((e) => e === "NOT_LEADER_OR_FOLLOWER").length, { timeout: 15_000 }).toBeGreaterThan(0);
    await page.reload();
    const sheet = page.getByTestId("lab-rule-sheet");
    await expect(sheet.getByRole("table", { name: "Requests the rule touched" })).toContainText("NOT_LEADER_OR_FOLLOWER");
  } finally {
    await app.stop();
    await clean(page, baseURL!);
  }
});

test("Lab: a field the rule cannot take is said under it, in the server's words", async ({ page }) => {
  await page.goto("/lab?new=slow");
  const form = page.getByTestId("lab-form-slow");
  await form.getByTestId("lab-delay").fill("0");
  await expect(page.getByTestId("lab-preview-none")).toContainText("Type a delay from 1 to 60,000 ms.");
  await page.getByTestId("lab-start").click();
  await expect(form.getByTestId("err-delay")).toHaveText("Type a delay from 1 to 60,000 ms.");
  await form.getByTestId("lab-hits").fill("0");
  await page.getByTestId("lab-start").click();
  await expect(form.getByTestId("err-hits")).toContainText("Type a number of hits from 1");
  // Tabs are in the address; Esc closes and Back does not reopen.
  await page.getByTestId("lab-tabs").getByRole("tab", { name: /Fail/ }).click();
  await expect(page).toHaveURL(/new=fail/);
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("lab-dialog")).toBeHidden();
  await expect(page).not.toHaveURL(/new=/);
});

test("Lab: drop a connection, force a rebalance and expire a member, once each; each leaves a line", async ({ page, baseURL, browserName }) => {
  await clean(page, baseURL!);
  const address = await kafkaAddress(page.request);
  const client = `lab-once-${browserName}`;
  const app = await traffic(address, client, "orders", 200);
  const group = `lab-group-${browserName}`;
  const consumer = app.kafka.consumer({ groupId: group, sessionTimeout: 30_000, rebalanceTimeout: 10_000 });
  await consumer.connect();
  await consumer.subscribe({ topic: "orders", fromBeginning: false });
  await consumer.run({ eachMessage: async () => {} });
  try {
    await expect.poll(async () => (await apiGet(page.request, `/api/v1/lab/once/rebalance`)).choices?.some((c: { value: string }) => c.value === group), { timeout: 30_000 }).toBe(true);
    await page.goto("/lab");
    // Drop, once.
    await page.getByTestId("start-drop").click();
    await expect(page).toHaveURL(/new=drop/);
    const drop = page.getByTestId("lab-once-drop");
    await drop.getByRole("combobox", { name: "The app" }).click();
    await page.getByRole("option", { name: client, exact: true }).click();
    await expect(drop.getByTestId("once-call")).toContainText(`client=${client} once`);
    await page.getByTestId("lab-once-go").click();
    await expect(page.getByTestId("lab-dialog")).toBeHidden();
    // Rebalance.
    await page.getByTestId("start-rebalance").click();
    const reb = page.getByTestId("lab-once-rebalance");
    await reb.getByRole("combobox", { name: "The group" }).click();
    await page.getByRole("option", { name: group, exact: true }).click();
    await expect(reb.getByTestId("once-call")).toContainText(`group=${group}`);
    await page.getByTestId("lab-once-go").click();
    await expect(page.getByTestId("lab-dialog")).toBeHidden();
    await expect(page.getByTestId("once-list")).toContainText(`Forced a rebalance of ${group}`);
    // Expire a member (after the group settles again).
    await expect.poll(async () => (await apiGet(page.request, `/api/v1/lab/once/expire`)).choices?.some((c: { label: string }) => c.label.startsWith(`${group} ·`)), { timeout: 30_000 }).toBe(true);
    await page.getByTestId("start-expire").click();
    const exp = page.getByTestId("lab-once-expire");
    await exp.getByRole("combobox", { name: "The member" }).click();
    await page.getByRole("option", { name: new RegExp(`^${group} ·`) }).first().click();
    await page.getByTestId("lab-once-go").click();
    await expect(page.getByTestId("lab-dialog")).toBeHidden();
    await expect(page.getByTestId("once-list")).toContainText(`Ran out the session of`);
  } finally {
    await consumer.disconnect().catch(() => {});
    await app.stop();
  }
});

test("Lab: Compact now on a compacted topic says what the planner finds, and runs only when a round is due", async ({ page }) => {
  await page.goto("/lab?new=compact");
  const tab = page.getByTestId("lab-once-compact");
  await expect(tab).toBeVisible();
  await expect(tab.getByRole("combobox", { name: "Compacted topic" })).toHaveText(/inventory\.levels|\w/);
  const body = tab.getByTestId("compact-body");
  await expect(body).toBeVisible();
  const blocked = await body.getByTestId("compact-blocked").count();
  if (blocked) await expect(tab.getByTestId("compact-run")).toBeDisabled();
  else await expect(body.getByTestId("compact-due")).toContainText("dirty");
});
