import { choose, expect, test } from "./fixtures";

test.beforeEach(async ({ page }) => {
  await page.goto("/gallery");
  await expect(page.getByTestId("gallery")).toBeVisible();
});

test("Button: a click lands", async ({ page }) => {
  await page.getByTestId("btn-primary").click();
  await page.getByTestId("btn-primary").click();
  await expect(page.getByTestId("btn-out")).toHaveText("clicks: 2");
  await page.getByTestId("btn-primary").focus();
  await page.keyboard.press("Enter");
  await page.keyboard.press("Space");
  await expect(page.getByTestId("btn-out")).toHaveText("clicks: 4");
});

test("the form: every control takes a value with the mouse and the submit sends them all", async ({ page }) => {
  const form = page.getByTestId("g-form");
  // An empty submit shows Kafka's rules, in place.
  await page.getByTestId("f-submit").click();
  await expect(form).toContainText("Type a name.");
  await expect(form).toContainText("Choose a topic.");
  await expect(form).toContainText("Tick to confirm.");
  await expect(page.getByTestId("form-out")).toHaveText("nothing submitted");
  // Input with a value Kafka refuses, then one it takes.
  await page.getByTestId("f-name").fill("bad name!");
  await page.getByTestId("f-submit").click();
  await expect(form).toContainText("Letters, digits");
  await page.getByTestId("f-name").fill("orders.v2");
  await page.getByTestId("f-value").fill('{"id": 7}');
  // Select (Radix, not the browser's).
  await choose(page, page.getByTestId("f-partitions"), "6 partitions");
  await expect(page.getByTestId("f-partitions")).toHaveText("6 partitions");
  // Combobox: search, then pick.
  await page.getByRole("combobox", { name: "Copy configs from" }).click();
  await page.getByPlaceholder("Search topics…").fill("pay");
  await page.getByRole("option", { name: /payments\.settled/ }).click();
  await expect(page.getByRole("combobox", { name: "Copy configs from" })).toHaveText(/payments\.settled/);
  // MultiSelect: two ticks, a chip each, one removed by its chip.
  await page.getByRole("combobox", { name: "Topics to watch" }).click();
  await page.getByRole("option", { name: /^orders\s/ }).click();
  await page.getByRole("option", { name: /clickstream/ }).click();
  await page.getByRole("option", { name: /pick-lists/ }).click();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("multi-summary")).toHaveText("3 topics");
  await page.getByRole("button", { name: "Remove pick-lists" }).click();
  await expect(page.getByTestId("multi-summary")).toHaveText("orders, clickstream");
  // RadioGroup, Switch, Checkbox.
  await page.getByRole("radio", { name: "1", exact: true }).click();
  await page.getByTestId("f-compact").click();
  await expect(page.getByTestId("f-compact")).toHaveAttribute("aria-checked", "true");
  await page.getByTestId("f-confirm").click();
  await expect(page.getByTestId("f-confirm")).toHaveAttribute("aria-checked", "true");
  await page.getByTestId("f-submit").click();
  await expect(page.locator("[data-sonner-toast]").filter({ hasText: "Submitted" })).toBeVisible();
  const out = JSON.parse((await page.getByTestId("form-out").textContent()) ?? "{}");
  expect(out).toEqual({ name: "orders.v2", value: '{"id": 7}', partitions: "6", topic: "payments.settled", watch: ["orders", "clickstream"], compact: true, acks: "1", confirm: true });
});

test("Select and MultiSelect work with the keyboard alone", async ({ page }) => {
  await page.getByTestId("f-partitions").focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("option", { name: "3 partitions" })).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(page.getByRole("option", { name: "6 partitions" })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("f-partitions")).toHaveText("6 partitions");
  await expect(page.getByTestId("f-partitions")).toBeFocused();
  const ms = page.getByRole("combobox", { name: "Topics to watch" });
  await ms.focus();
  await page.keyboard.press("Enter");
  await page.keyboard.type("dlq");
  await page.keyboard.press("Enter");
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("multi-summary")).toHaveText("orders.dlq");
  await expect(ms).toBeFocused();
});

test("ToggleGroup and Tabs put their choice in the address, and Back restores it", async ({ page }) => {
  await page.getByTestId("density").getByRole("radio", { name: "Compact" }).click();
  await expect(page).toHaveURL(/density=compact/);
  await page.getByRole("tab", { name: "Partitions" }).click();
  await expect(page).toHaveURL(/tab=partitions/);
  await expect(page.getByRole("tabpanel")).toContainText("last stable offset");
  await page.getByRole("tab", { name: "Partitions" }).focus();
  await page.keyboard.press("ArrowRight");
  await expect(page).toHaveURL(/tab=config/);
  await expect(page.getByRole("tabpanel")).toContainText("where it comes from");
  await page.goBack();
  await expect(page).toHaveURL(/tab=partitions/);
  await expect(page.getByRole("tab", { name: "Partitions" })).toHaveAttribute("aria-selected", "true");
  await page.goto("/gallery?tab=config&density=compact");
  await expect(page.getByRole("tab", { name: "Config" })).toHaveAttribute("aria-selected", "true");
  await expect(page.getByTestId("density").getByRole("radio", { name: "Compact" })).toHaveAttribute("aria-checked", "true");
});

test("Dialog: opens, keeps the focus inside, closes with Esc to its opener, and its action works", async ({ page, browserName }) => {
  const opener = page.getByTestId("open-dialog");
  await opener.click();
  const dialog = page.getByTestId("dialog");
  await expect(dialog).toBeVisible();
  await expect(page.getByTestId("dialog-input")).toBeFocused();
  await expect(page.getByTestId("dialog-confirm")).toBeDisabled();
  for (let i = 0; i < 6; i++) await page.keyboard.press("Tab");
  // Safari's Tab skips buttons by default (a setting of the system), so it can leave for the browser's own controls; what
  // must hold in every engine is that no element of the page behind the dialog gets the focus.
  const inside = await dialog.evaluate((d) => d.contains(document.activeElement) || !document.getElementById("root")!.contains(document.activeElement));
  expect(inside).toBe(true);
  if (browserName !== "webkit") expect(await dialog.evaluate((d) => d.contains(document.activeElement))).toBe(true);
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await expect(opener).toBeFocused();
  await opener.click();
  await page.getByTestId("dialog-input").fill("orders.dlq");
  await page.getByTestId("dialog-confirm").click();
  await expect(dialog).toBeHidden();
  await expect(page.getByTestId("overlay-out")).toContainText("deleted: orders.dlq");
  await opener.click();
  await page.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toBeHidden();
});

test("Sheet: a right drawer the full height of the window, with its own address; Back and Esc close it", async ({ page }) => {
  await page.getByTestId("open-sheet").click();
  const sheet = page.getByTestId("sheet");
  await expect(sheet).toBeVisible();
  await expect(page).toHaveURL(/record=r7/);
  await sheet.evaluate((e) => Promise.all(e.getAnimations({ subtree: true }).map((a) => a.finished)));
  const box = await sheet.boundingBox();
  const vp = page.viewportSize()!;
  expect(Math.round(box!.y)).toBe(0);
  expect(Math.round(box!.height)).toBe(vp.height);
  expect(Math.round(box!.x + box!.width)).toBe(vp.width);
  await page.goBack();
  await expect(sheet).toBeHidden();
  await expect(page).not.toHaveURL(/record=/);
  // From a table row, closed with Esc.
  await page.getByRole("row").filter({ hasText: "1,000,021" }).click();
  await expect(sheet).toBeVisible();
  await expect(page).toHaveURL(/record=r3/);
  await page.keyboard.press("Escape");
  await expect(sheet).toBeHidden();
  await expect(page).not.toHaveURL(/record=/);
  // A deep link opens it; closing it stays on the gallery.
  await page.goto("/gallery?record=r5");
  await expect(sheet).toBeVisible();
  await page.getByRole("button", { name: "Close" }).click();
  await expect(sheet).toBeHidden();
  await expect(page).toHaveURL(/\/gallery$/);
});

test("DropdownMenu: a choice with the mouse, another with the keyboard", async ({ page }) => {
  await page.getByTestId("open-menu").click();
  await page.getByRole("menuitem", { name: "Add partitions" }).click();
  await expect(page.getByTestId("overlay-out")).toContainText("menu: Add partitions");
  await expect(page.getByTestId("open-menu")).toBeFocused();
  // The keyboard part with the pointer out of the way (a menu item under the pointer takes the focus back on a move).
  const h = await page.locator("h1").boundingBox();
  await page.mouse.move(h!.x + 5, h!.y + 5);
  await page.keyboard.press("Enter");
  await expect(page.getByRole("menu")).toBeVisible();
  // At a person's pace: Radix settles the first item's focus as the menu finishes opening.
  await page.getByRole("menu").evaluate((m) => Promise.all(m.getAnimations().map((a) => a.finished)));
  await page.waitForTimeout(100);
  // The keyboard opens it on its first item (Produce): two down is Delete records.
  await expect(page.getByRole("menuitem", { name: "Produce" })).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await expect(page.getByRole("menuitem", { name: "Delete records" })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("overlay-out")).toContainText("menu: Delete records");
});

test("Popover: a field inside it applies, Esc closes it", async ({ page }) => {
  await page.getByTestId("open-popover").click();
  await page.getByTestId("popover-input").fill("2026-11-13 14:30");
  await page.getByTestId("popover-apply").click();
  await expect(page.getByTestId("popover-input")).toBeHidden();
  await expect(page.getByTestId("overlay-out")).toContainText("from: 2026-11-13 14:30");
  await page.getByTestId("open-popover").click();
  await expect(page.getByTestId("popover-input")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("popover-input")).toBeHidden();
  await expect(page.getByTestId("open-popover")).toBeFocused();
});

test("Tooltip: on hover and on keyboard focus", async ({ page }) => {
  await page.getByTestId("tooltip-trigger").hover();
  await expect(page.getByRole("tooltip")).toContainText("every replica");
  const h1 = await page.locator("h1").boundingBox();
  await page.mouse.move(h1!.x + 10, h1!.y + 10, { steps: 5 });
  await expect(page.getByRole("tooltip")).toBeHidden();
  await page.getByTestId("open-popover").focus();
  await page.keyboard.press("Tab");
  await expect(page.getByRole("tooltip")).toBeVisible();
});

test("Toast, StatStrip, CodeValue and EmptyState", async ({ page }) => {
  await page.getByTestId("toast-btn").click();
  await expect(page.locator("[data-sonner-toast]").filter({ hasText: "slice R1" })).toBeVisible();
  const g = page.getByTestId("g-display");
  await expect(g.locator("[data-slot=stat]")).toHaveCount(3);
  const value = g.locator("[data-slot=stat-value]").nth(1);
  await expect(value).toHaveText("1,234,567");
  expect(await value.evaluate((e) => getComputedStyle(e).fontVariantNumeric)).toContain("tabular-nums");
  const code = g.locator("[data-slot=code-value] code");
  expect(await code.evaluate((e) => e.scrollWidth > e.clientWidth)).toBe(true);
  await g.locator("[data-slot=code-value] button").click();
  await expect(page.locator("[data-sonner-toast]").filter({ hasText: "Path copied" })).toBeVisible();
  await expect(g.locator("[data-slot=empty-state]")).toContainText("No records yet");
});

test("DataTable: fixed columns, ellipsis with the whole value, sorting, a sticky header, and a virtual body past 500 rows", async ({ page }) => {
  const table = page.getByTestId("g-table").locator("[data-slot=data-table]");
  await expect(table.locator("tbody tr[data-row]")).toHaveCount(20);
  const key = table.locator("tbody tr[data-row='r0'] td").nth(3);
  expect(await key.evaluate((e) => e.scrollWidth > e.clientWidth)).toBe(true);
  await expect(key).toHaveAttribute("title", /with-a-very-long-key/);
  const widths = await table.locator("thead th").evaluateAll((ths) => ths.map((t) => Math.round(t.getBoundingClientRect().width)));
  // Sort by Bytes (descending on the second click): the rows reorder, the columns do not move.
  const bytes = table.getByRole("button", { name: "Bytes" });
  const sorted = async (dir: "ascending" | "descending") => {
    await expect(table.locator("thead th").nth(4)).toHaveAttribute("aria-sort", dir);
    const nums = (await table.locator("tbody tr[data-row] td:nth-child(5)").allTextContents()).map(Number);
    expect(nums).toEqual([...nums].sort((a, b) => (dir === "ascending" ? a - b : b - a)));
  };
  await bytes.click();
  await sorted("descending");
  await bytes.click();
  await sorted("ascending");
  expect(await table.locator("thead th").evaluateAll((ths) => ths.map((t) => Math.round(t.getBoundingClientRect().width)))).toEqual(widths);
  // 1,000 rows: virtual (far fewer rows in the DOM), the header stays at the top while the body scrolls.
  await page.getByTestId("rows").getByRole("radio", { name: "1,000" }).click();
  await expect(page).toHaveURL(/rows=1000/);
  await expect(table).toHaveAttribute("data-virtual", "true");
  const inDom = await table.locator("tbody tr[data-row]").count();
  expect(inDom).toBeGreaterThan(5);
  expect(inDom).toBeLessThan(80);
  const headTop = await table.locator("thead").evaluate((h) => h.getBoundingClientRect().top);
  await table.evaluate((t) => (t.scrollTop = 20_000));
  // The rows in view are now far down the list (the spacer above them is most of the height) and still few.
  await expect.poll(() => table.locator("tbody > tr").first().evaluate((e) => e.getBoundingClientRect().height)).toBeGreaterThan(15_000);
  await expect.poll(() => table.locator("tbody tr[data-row]").first().getAttribute("aria-rowindex").then(Number)).toBeGreaterThan(400);
  expect(await table.locator("thead").evaluate((h) => h.getBoundingClientRect().top)).toBe(headTop);
  expect(await table.locator("tbody tr[data-row]").count()).toBeLessThan(80);
});
