// Slice R1b: a topic's Config tab: the filter and the chips in the address, the panels that open in place, and the
// dialogs that change a config (Check, Save, a list, the two lags, a refused value) and reset it.
import { expect, overflowX, test } from "./fixtures";
import { call, fresh, get, topic } from "./r1b-helpers";

const configUrl = (t: string, q = "") => `/topics/${encodeURIComponent(t)}/config${q}`;
const row = (page: import("@playwright/test").Page, key: string) => page.locator(`[data-testid=config-row][data-key="${key}"]`).first();

test("the chips and the filter are in the address, Back restores them, and nothing scrolls sideways", async ({ page, request, baseURL }, info) => {
  const name = fresh(info, "cfg");
  await topic(request, baseURL!, name, 1, "short");
  await page.goto(configUrl(name));
  // Where the values come from: labelled figures whose parts add up to the whole (CON-1345).
  const sources = page.getByTestId("config-sources");
  await expect(sources.getByTestId("src-all")).toBeVisible();
  const fig = async (id: string) => Number((await sources.getByTestId(id).locator("dd").innerText().catch(() => "0")).replace(/\D/g, "") || 0);
  const parts = (await Promise.all(["src-topic", "src-broker", "src-flag", "src-default"].map(fig))).reduce((a, b) => a + b, 0) + ((await sources.getByTestId("src-other").count()) ? await fig("src-other") : 0);
  expect(parts).toBe(await fig("src-all"));
  expect(await fig("src-topic")).toBeGreaterThan(0);
  await expect(page.getByTestId("section-changed")).toContainText("retention.ms");
  const chips = page.getByTestId("config-chips");
  await chips.getByRole("radio", { name: /Changed/ }).click();
  await expect(page).toHaveURL(/show=changed/);
  await expect(page.getByTestId("section-segments")).toHaveCount(0);
  await chips.getByRole("radio", { name: "Segments" }).click();
  await expect(page).toHaveURL(/show=segments/);
  await expect(row(page, "segment.bytes")).toBeVisible();
  await expect(page.getByTestId("section-cleanup")).toHaveCount(0);
  await page.goBack();
  await expect(page).toHaveURL(/show=changed/);
  await expect(chips.getByRole("radio", { name: /Changed/ })).toHaveAttribute("aria-checked", "true");
  await chips.getByRole("radio", { name: /All/ }).click();
  await expect(page).not.toHaveURL(/show=/);
  await page.getByTestId("config-filter").fill("dirty");
  await expect(page).toHaveURL(/q=dirty/);
  await expect(page.getByTestId("config-row")).toHaveCount(1);
  await expect(row(page, "min.cleanable.dirty.ratio")).toBeVisible();
  await page.getByRole("button", { name: "Clear the filter" }).click();
  await expect(page).not.toHaveURL(/q=/);
  for (const w of [1024, 1280, 1440, 1920]) {
    await page.setViewportSize({ width: w, height: 900 });
    expect(await overflowX(page), `overflow at ${w}`).toBe(0);
  }
});

test("Where from and Kafka's text open in place: the address and the scroll stay", async ({ page, request, baseURL }, info) => {
  const name = fresh(info, "cfgp");
  await topic(request, baseURL!, name, 1, "short");
  await page.goto(configUrl(name));
  const r = row(page, "min.cleanable.dirty.ratio");
  await r.scrollIntoViewIfNeeded();
  const url = page.url();
  const y = await page.evaluate(() => window.scrollY);
  await r.getByRole("button", { name: "Kafka's text" }).click();
  await expect(r.getByTestId("kafka-text")).toBeVisible();
  await expect(r.getByTestId("kafka-text")).toContainText("Type");
  expect(page.url()).toBe(url);
  expect(Math.abs((await page.evaluate(() => window.scrollY)) - y)).toBeLessThan(2);
  await r.getByRole("button", { name: "Kafka's text" }).click();
  await expect(r.getByTestId("kafka-text")).toBeHidden();
  const ret = row(page, "retention.ms");
  await ret.getByRole("button", { name: "Where from" }).click();
  await expect(ret.getByTestId("where-from")).toContainText("Dynamic topic");
  await expect(ret.getByTestId("where-from")).toContainText("takes over");
  expect(page.url()).toBe(url);
});

test("edit a value: Check asks Kafka and changes nothing, Save changes it, and the tab shows it without a reload", async ({ page, request, baseURL }, info) => {
  const name = fresh(info, "cfge");
  await topic(request, baseURL!, name, 1, "short");
  await page.goto(configUrl(name));
  const r = row(page, "retention.ms");
  await r.getByTestId("row-menu").click();
  await page.getByRole("menuitem", { name: "Edit" }).click();
  await expect(page).toHaveURL(/edit=retention\.ms/);
  const d = page.getByTestId("dialog-config-edit");
  await expect(d.getByTestId("config-value")).toHaveValue("3600000");
  await d.getByTestId("config-value").fill("7200000");
  await expect(d.getByTestId("the-call")).toContainText("7200000");
  await d.getByTestId("config-check").click();
  await expect(d.getByTestId("config-checked")).toContainText("Kafka would take this");
  const still = await get(request, `/api/v1/topics/${name}/config`);
  expect(still.body.rows.find((x: { name: string }) => x.name === "retention.ms").value).toBe("3600000");
  await d.getByTestId("config-save").click();
  await expect(d.getByTestId("config-saved")).toBeVisible();
  await expect(d.getByTestId("saved-now")).toContainText("7200000");
  await d.getByTestId("saved-done").click();
  await expect(d).toBeHidden();
  await expect(page).not.toHaveURL(/edit=/);
  await expect(r.getByTestId("config-value")).toHaveText("7200000");
});

test("a list: append a policy", async ({ page, request, baseURL }, info) => {
  const name = fresh(info, "cfgl");
  await topic(request, baseURL!, name, 1, "event");
  await page.goto(configUrl(name, "?edit=cleanup.policy"));
  const d = page.getByTestId("dialog-config-edit");
  await expect(d.getByTestId("config-op").getByRole("radio", { name: /Append/ })).toHaveAttribute("aria-checked", "true");
  await d.getByTestId("config-value").fill("compact");
  await expect(d.getByTestId("the-call")).toContainText("APPEND");
  await d.getByTestId("config-save").click();
  await expect(d.getByTestId("saved-now")).toContainText("delete,compact");
  await page.keyboard.press("Escape");
  await expect(row(page, "cleanup.policy").getByTestId("config-value")).toHaveText("delete,compact");
});

test("the two lags together: min above max is refused in Kafka's words, Edit again, then saved", async ({ page, request, baseURL }, info) => {
  const name = fresh(info, "cfgg");
  await topic(request, baseURL!, name, 1, "compact");
  await page.goto(configUrl(name, "?edit=min.compaction.lag.ms"));
  const d = page.getByTestId("dialog-config-edit");
  await expect(d.getByRole("heading", { name: "Edit compaction lag" })).toBeVisible();
  await d.getByTestId("config-value").fill("50000");
  await d.getByTestId("config-other").fill("10000");
  await d.getByTestId("config-save").click();
  const refused = d.getByTestId("config-refused");
  await expect(refused.getByTestId("refusal-code")).toContainText("INVALID_CONFIG");
  const api = await call(request, baseURL!, `/api/v1/topics/${name}/config/edit`, { key: "min.compaction.lag.ms", op: "set", value: "50000", other: "10000" });
  await expect(refused.getByTestId("refusal-message")).toHaveText(api.body.message);
  await expect(refused.getByTestId("would-pass")).toBeVisible();
  await refused.getByTestId("edit-again").click();
  await d.getByTestId("config-value").fill("1000");
  await d.getByTestId("config-other").fill("60000");
  await d.getByTestId("config-save").click();
  await expect(d.getByTestId("saved-change")).toHaveCount(2);
});

test("a value Kafka will not take: refused in its own words, and nothing changed", async ({ page, request, baseURL }, info) => {
  const name = fresh(info, "cfgv");
  await topic(request, baseURL!, name, 1, "event");
  await page.goto(configUrl(name, "?edit=min.cleanable.dirty.ratio"));
  const d = page.getByTestId("dialog-config-edit");
  await d.getByTestId("config-value").fill("7");
  await d.getByTestId("config-check").click();
  const refused = d.getByTestId("config-refused");
  await expect(refused.getByTestId("refusal-code")).toContainText("INVALID_CONFIG (40)");
  const api = await call(request, baseURL!, `/api/v1/topics/${name}/config/edit`, { key: "min.cleanable.dirty.ratio", op: "set", value: "7", other: "", check: "1" });
  await expect(refused.getByTestId("refusal-message")).toHaveText(api.body.message);
  const still = await get(request, `/api/v1/topics/${name}/config`);
  expect(still.body.rows.find((x: { name: string }) => x.name === "min.cleanable.dirty.ratio").source).not.toBe("dynamic_topic");
});

test("reset: the topic follows the broker again", async ({ page, request, baseURL }, info) => {
  const name = fresh(info, "cfgr");
  await topic(request, baseURL!, name, 1, "short");
  await page.goto(configUrl(name));
  const r = row(page, "retention.ms");
  await r.getByTestId("row-menu").click();
  await page.getByRole("menuitem", { name: "Reset to the inherited value" }).click();
  await expect(page).toHaveURL(/reset=retention\.ms/);
  const d = page.getByTestId("dialog-config-reset");
  await expect(d.getByTestId("reset-now")).toContainText("3600000");
  // What takes over is the broker's (a flag, or Kafka's default): the row shows it after.
  await expect(d.getByTestId("reset-after")).not.toContainText("3600000");
  const after = (await d.getByTestId("reset-after").locator("code").innerText()).trim();
  await d.getByTestId("reset-submit").click();
  await expect(d).toBeHidden();
  await expect(page.locator("[data-sonner-toast]").filter({ hasText: "Reset retention.ms." })).toBeVisible();
  await expect(row(page, "retention.ms").getByTestId("source")).not.toHaveText("Dynamic topic");
  await expect(row(page, "retention.ms").getByTestId("config-value")).toHaveText(after);
});
