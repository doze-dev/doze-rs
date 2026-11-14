// The Lab teaches (CON-1370 to CON-1375): How the Lab works shows on a first visit and stays folded once read; each recipe fills
// the New rehearsal dialog with the broker's own names and Start makes the rule it says; the preview sentence follows the form;
// a rule's "See its requests on Live" lands on Live filtered to its lab tag; Stop all ends every rule.
import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures";
import { apiGet, apiPost, kafkaAddress, traffic } from "./r4";

test.describe.configure({ mode: "serial" });

type Recipe = { id: string; tab: string; title: string; fields: Record<string, string> | null; target: string | null; how: string | null; needs: string | null };

async function clean(page: Page, baseURL: string) {
  await apiPost(page.request, baseURL, "/api/v1/lab/stop-all", {});
  await apiPost(page.request, baseURL, "/api/v1/lab/history/clear", {});
}

/** A returning visitor: How the Lab works folded, so the recipes are in view. */
async function returning(page: Page) {
  await page.addInitScript(() => localStorage.setItem("doze:lab-guide", "folded"));
}

const SHARE: Record<string, string> = { every: "Every request", "5": "1 in 5", "10": "1 in 10" };

test("Lab: How the Lab works shows on a first visit, folds when read, and stays folded after a reload", async ({ page }) => {
  await page.goto("/lab");
  const guide = page.getByTestId("lab-guide");
  await expect(guide).toHaveAttribute("data-state", "open");
  const body = page.getByTestId("lab-guide-body");
  await expect(body.getByRole("list", { name: "How to use it" }).locator("li")).toHaveCount(3);
  await expect(body).toContainText("Nothing is saved");
  await expect(body).toContainText("Stop all");
  await expect(page.getByRole("heading", { level: 2, name: "How the Lab works" })).toBeVisible();
  await page.getByTestId("lab-guide-dismiss").click();
  await expect(guide).toHaveAttribute("data-state", "folded");
  await expect(body).toBeHidden();
  await page.reload();
  await expect(page.getByTestId("lab-guide")).toHaveAttribute("data-state", "folded");
  await expect(page.getByTestId("lab-guide-body")).toBeHidden();
  // The one line opens it again, by keyboard, and that is remembered too.
  await page.getByTestId("lab-guide-toggle").focus();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("lab-guide")).toHaveAttribute("data-state", "open");
  await page.reload();
  await expect(page.getByTestId("lab-guide")).toHaveAttribute("data-state", "open");
});

test("Lab: each recipe fills the dialog with the broker's names, and Start makes the rule it says", async ({ page, baseURL, browserName }) => {
  test.setTimeout(150_000);
  await clean(page, baseURL!);
  await returning(page);
  const client = `lab-recipe-${browserName}`;
  const app = await traffic(await kafkaAddress(page.request), client, "orders", 120);
  const group = `lab-recipe-group-${browserName}`;
  const consumer = app.kafka.consumer({ groupId: group, sessionTimeout: 30_000, rebalanceTimeout: 10_000 });
  await consumer.connect();
  await consumer.subscribe({ topic: "orders", fromBeginning: false });
  await consumer.run({ eachMessage: async () => {} });
  try {
    const ready = async () => ((await apiGet(page.request, "/api/v1/lab/guide")).recipes as Recipe[]).filter((r) => !r.needs).map((r) => r.id);
    await expect.poll(ready, { timeout: 40_000 }).toEqual(expect.arrayContaining(["slow-produce", "slow-fetch", "fail-leader", "fail-timeout", "drop-once", "drop-flap", "expire-member", "rebalance-group"]));
    // The rule recipes: the form says each field; the preview is the server's sentence for those fields; Start makes that rule.
    for (const id of ["slow-produce", "slow-fetch", "fail-leader", "fail-timeout"]) {
      await clean(page, baseURL!);
      await page.goto("/lab");
      const r = ((await apiGet(page.request, "/api/v1/lab/guide")).recipes as Recipe[]).find((x) => x.id === id)!;
      await page.getByTestId(`recipe-${id}`).click();
      await expect(page).toHaveURL(new RegExp(`new=${r.tab}.*recipe=${id}|recipe=${id}.*new=${r.tab}`));
      const dialog = page.getByTestId("lab-dialog");
      await expect(dialog.getByTestId("lab-recipe-band")).toContainText(r.title);
      const form = dialog.getByTestId(`lab-form-${r.tab}`);
      const f = r.fields!;
      await expect(form.getByTestId("multi-chips").first()).toHaveText(f.api);
      await expect(form.getByRole("combobox", { name: "Topics" })).toHaveText(f.topic);
      await expect(form.getByRole("combobox", { name: "Client" })).toHaveText(f.client || "Every client");
      await expect(form.getByTestId("lab-share")).toHaveText(SHARE[f.share]);
      if (r.tab === "slow") await expect(form.getByTestId("lab-delay")).toHaveValue(f.delay);
      else await expect(form.getByTestId("lab-code")).toContainText(`(${f.code})`);
      const checked = await apiPost(page.request, baseURL!, "/api/v1/lab/rules", { kind: r.tab, fields: f, check: true });
      await expect(dialog.getByTestId("lab-preview-sentence")).toHaveText(checked.body.form.preview.plain);
      await expect(dialog.getByTestId("lab-expect")).not.toBeEmpty();
      await dialog.getByTestId("lab-start").click();
      await expect(page.getByTestId("lab-rule-sheet")).toBeVisible();
      await expect(page).toHaveURL(/rule=\d+/);
      const ruleId = Number(new URL(page.url()).searchParams.get("rule"));
      const made = (await apiGet(page.request, `/api/v1/lab/rules/${ruleId}`)).rule;
      expect(made.api).toBe(f.api);
      expect(made.target).toContain(f.topic);
      if (r.tab === "slow") expect(made.delay_ms).toBe(Number(f.delay));
      else expect(made.code[0]).toBe(Number(f.code));
      expect(made.share).toBe(f.share === "every" ? "every request" : `1 in ${f.share}`);
      await page.keyboard.press("Escape");
    }
    await clean(page, baseURL!);
    // The once recipes: the target and the how are chosen; the rehearsal runs and says where to see it.
    for (const id of ["drop-once", "expire-member", "rebalance-group"]) {
      await expect.poll(async () => (await ready()).includes(id), { timeout: 30_000 }).toBe(true);
      await page.goto("/lab");
      const r = ((await apiGet(page.request, "/api/v1/lab/guide")).recipes as Recipe[]).find((x) => x.id === id)!;
      if (r.tab === "rebalance") {
        // After an expiry the group rebalances by itself: a forced one waits until it has settled.
        const settled = async () => ((await apiGet(page.request, "/api/v1/lab/once/rebalance")).choices as { value: string; desc: string }[]).some((c) => c.value === r.target && /stable/i.test(c.desc));
        await expect.poll(settled, { timeout: 30_000 }).toBe(true);
      }
      await page.getByTestId(`recipe-${id}`).click();
      const tab = page.getByTestId(`lab-once-${r.tab}`);
      const label = await apiGet(page.request, `/api/v1/lab/once/${r.tab}`);
      const shown = (label.choices as { value: string; label: string }[]).find((c) => c.value === r.target)?.label ?? r.target!;
      await expect(tab.getByRole("combobox")).toHaveText(shown);
      if (r.how) await expect(tab.getByRole("radio", { name: "Once, now" })).toHaveAttribute("aria-checked", "true");
      await page.getByTestId("lab-once-go").click();
      await expect(page.getByTestId("once-done")).toBeVisible({ timeout: 15_000 });
      await expect(page.getByTestId("once-see")).toBeVisible();
      await page.getByTestId("write-done").click();
      await expect(page.getByTestId("lab-dialog")).toBeHidden();
      if (r.tab === "rebalance") await expect(page.getByTestId("once-list")).toContainText(`Forced a rebalance of ${r.target}`);
      if (r.tab === "expire") await expect(page.getByTestId("once-list").getByTestId("once-story").first()).toBeVisible();
    }
    // Compact: the topic is chosen.
    const compact = ((await apiGet(page.request, "/api/v1/lab/guide")).recipes as Recipe[]).find((x) => x.id === "compact-now")!;
    if (!compact.needs) {
      await page.goto("/lab");
      await page.getByTestId("recipe-compact-now").click();
      await expect(page.getByTestId("lab-once-compact").getByRole("combobox", { name: "Compacted topic" })).toHaveText(compact.target!);
    }
  } finally {
    await consumer.disconnect().catch(() => {});
    await app.stop();
    await clean(page, baseURL!);
  }
});

test("Lab: the preview sentence follows the form, field by field", async ({ page }) => {
  await returning(page);
  await page.goto("/lab?new=slow");
  const dialog = page.getByTestId("lab-dialog");
  const form = dialog.getByTestId("lab-form-slow");
  const sentence = dialog.getByTestId("lab-preview-sentence");
  await form.getByTestId("lab-delay").fill("1234");
  await expect(sentence).toContainText("will wait 1234 ms longer before the broker answers");
  await form.getByTestId("lab-jitter").fill("50");
  await expect(sentence).toContainText("will wait 1234 ms longer, plus up to 50 ms at random,");
  await form.getByRole("combobox", { name: "Topics" }).click();
  await page.getByRole("option", { name: /^orders\s*\d+ partitions?$/ }).click();
  await page.keyboard.press("Escape");
  await expect(sentence).toContainText("Produce requests to orders");
  await form.getByTestId("lab-hits").fill("7");
  await expect(sentence).toContainText("It ends after 7 hits or 10 min, whichever comes first, or when you remove it.");
  // A pattern for the client, typed: the rule names it.
  await form.getByRole("combobox", { name: "Client" }).click();
  await page.keyboard.type("pay*");
  await page.getByTestId("combobox-pattern").click();
  await expect(form.getByRole("combobox", { name: "Client" })).toHaveText("pay*");
  await expect(sentence).toContainText("to orders from pay* will wait");
  // The lesson opens on demand with the settings that decide it.
  await dialog.getByTestId("lab-lesson-toggle").click();
  await expect(dialog.getByTestId("lab-lesson-body")).toContainText("request.timeout.ms");
});

test("Lab: a rule's sheet leads to Live filtered to its lab tag, and every row there carries it", async ({ page, baseURL, browserName }) => {
  await clean(page, baseURL!);
  await returning(page);
  const client = `lab-watch-${browserName}`;
  const app = await traffic(await kafkaAddress(page.request), client, "orders", 120);
  try {
    const made = await apiPost(page.request, baseURL!, "/api/v1/lab/rules", { kind: "slow", fields: { api: "Produce", topic: "orders", client, delay: "20", share: "every", ends: "5" } });
    expect(made.body.outcome).toBe("made");
    const id = made.body.id as number;
    await expect.poll(async () => (await apiGet(page.request, `/api/v1/lab/rules/${id}`)).rule.hits, { timeout: 20_000 }).toBeGreaterThan(1);
    await page.goto(`/lab?rule=${id}`);
    const sheet = page.getByTestId("lab-rule-sheet");
    await expect(sheet.getByTestId("lab-watch")).toContainText("lab tag");
    await sheet.getByTestId("watch-live").click();
    await expect(page).toHaveURL(new RegExp(`/live\\?lab=${id}$`));
    await expect(page.getByTestId("lab-filter")).toHaveText(`Lab rule ${id}`);
    const rows = page.getByTestId("tape").locator("tbody tr");
    await expect(rows.first()).toBeVisible();
    const n = await rows.count();
    expect(n).toBeGreaterThan(0);
    for (let i = 0; i < n; i++) await expect(rows.nth(i)).toContainText("lab");
    // The chip removes the filter.
    await page.getByTestId("lab-filter").click();
    await expect(page).not.toHaveURL(/lab=/);
    await expect(page.getByTestId("lab-filter")).toHaveCount(0);
    // The table's own way to Live, from the Lab.
    await page.goto("/lab");
    await page.getByRole("table", { name: "Rules in force" }).getByTestId("rule-watch").first().click();
    await expect(page).toHaveURL(new RegExp(`/live\\?lab=${id}$`));
  } finally {
    await app.stop();
    await clean(page, baseURL!);
  }
});

test("Lab: Stop all ends every rule in force, and is off when there is none", async ({ page, baseURL }) => {
  await clean(page, baseURL!);
  await returning(page);
  for (const fields of [
    { api: "Produce", topic: "orders", delay: "10", share: "every", ends: "5" },
    { api: "Fetch", topic: "orders", delay: "10", share: "4", ends: "5" },
  ]) {
    expect((await apiPost(page.request, baseURL!, "/api/v1/lab/rules", { kind: "slow", fields })).body.outcome).toBe("made");
  }
  await page.goto("/lab");
  await expect(page.getByRole("table", { name: "Rules in force" }).locator("tbody tr")).toHaveCount(2);
  await page.getByTestId("lab-stop-all").click();
  await expect(page).toHaveURL(/stop=all/);
  await page.getByTestId("lab-stop-confirm").click();
  await expect(page.getByTestId("lab-stopped")).toContainText("2 rules ended.");
  await page.getByTestId("write-done").click();
  await expect(page.getByTestId("lab-stop-dialog")).toBeHidden();
  expect((await apiGet(page.request, "/api/v1/lab")).home.active).toEqual([]);
  await expect(page.getByRole("table", { name: "Rules that ended" }).locator("tbody tr")).toHaveCount(2);
  await expect(page.getByTestId("lab-stop-all")).toBeDisabled();
  await clean(page, baseURL!);
});
