// Settings (slice R4): the five dynamic broker configs and their Edit dialog (Select, ToggleGroup, Check, Save, Kafka's refusal),
// the version profile, intervals, weight, data directory and flags.
import { choose, expect, test } from "./fixtures";
import { noMemory, noNativeSelect, widths } from "./r4";

test("Settings: every section, figures as tiles, the five configs, no memory figure, no native control, no overflow", async ({ page }) => {
  await page.goto("/settings");
  await expect(page.locator("h1")).toHaveText("Settings");
  const table = page.getByRole("table", { name: "Dynamic broker configs" });
  for (const key of ["log.segment.bytes", "log.index.interval.bytes", "message.max.bytes", "log.retention.bytes", "log.retention.ms"]) await expect(table).toContainText(key);
  for (const h of ["Broker configs", "Version profile", "Intervals", "Weight", "Data", "Flags"]) await expect(page.getByRole("heading", { name: h, exact: true })).toBeVisible();
  await expect(page.locator("[data-slot=stat-strip]").first()).toContainText("Set at run time");
  await expect(page.getByRole("table", { name: "APIs offered" })).toContainText("Produce");
  await expect(page.getByRole("table", { name: "Intervals" })).toContainText("--log-retention-check-interval");
  await expect(page.locator("[data-slot=stat]").filter({ hasText: "Binary" })).toContainText(/MB/);
  await noMemory(page);
  await noNativeSelect(page);
  await widths(page);
});

test("Settings: the data directory wraps its whole path inside its box and copies it", async ({ page, browserName }) => {
  await page.goto("/settings");
  const path = page.getByTestId("data-card").getByTestId("path-text");
  await expect(path).toHaveText(/\//);
  const [box, inner] = [await page.getByTestId("data-card").locator("[data-slot=path-value]").boundingBox(), await path.boundingBox()];
  expect(inner!.x + inner!.width).toBeLessThanOrEqual(box!.x + box!.width + 1);
  // The text is all there, not cut with an ellipsis.
  expect(await path.evaluate((el) => el.scrollWidth <= el.clientWidth + 1)).toBe(true);
  if (browserName === "chromium") {
    await page.getByTestId("data-card").getByRole("button", { name: /Copy path/ }).click();
    await expect(page.locator("[data-sonner-toast]").filter({ hasText: "Path copied" })).toBeVisible();
    expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(await path.innerText());
  }
});

test("Settings: Show all APIs, the flags toggle", async ({ page }) => {
  await page.goto("/settings");
  const apis = page.getByRole("table", { name: "APIs offered" }).locator("tbody tr");
  await expect(apis).toHaveCount(8);
  await page.getByTestId("show-apis").click();
  expect(await apis.count()).toBeGreaterThan(8);
  const flags = page.getByRole("table", { name: "Flags" });
  await page.getByTestId("flags-which").getByRole("radio", { name: /^All/ }).click();
  await expect(flags).toContainText("--data-dir");
  expect(await flags.locator("tbody tr").count()).toBeGreaterThan(10);
});

test("Settings: the Edit dialog has its own address, its Select and toggle work by mouse, Esc and Back close it", async ({ page }) => {
  await page.goto("/settings");
  const edit = page.getByTestId("edit-log.retention.ms");
  await edit.click();
  await expect(page).toHaveURL(/edit=log.retention.ms/);
  const dialog = page.getByTestId("settings-edit");
  await expect(dialog.getByRole("heading")).toContainText("log.retention.ms");
  await expect(dialog.getByTestId("call-box")).toContainText("IncrementalAlterConfigs");
  // The config Select (shadcn, not native) moves the dialog to another config.
  await choose(page, dialog.getByTestId("cfg-key"), "log.segment.bytes");
  await expect(page).toHaveURL(/edit=log.segment.bytes/);
  await expect(dialog.getByRole("heading")).toContainText("log.segment.bytes");
  await noNativeSelect(page);
  // Applies to: This broker.
  await dialog.getByTestId("cfg-scope").getByRole("radio", { name: /This broker/ }).click();
  await expect(page).toHaveURL(/scope=this/);
  await expect(dialog.getByTestId("call-box")).toContainText("this broker");
  // The unit hint follows what is typed.
  await dialog.getByTestId("cfg-value").fill("268435456");
  await expect(dialog.getByTestId("cfg-unit")).toHaveText("B = 256 MiB");
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await expect(page).not.toHaveURL(/edit=/);
  await expect(edit).toBeFocused();
  // A deep link opens it; Back from an opened one closes it.
  await page.goto("/settings?edit=message.max.bytes");
  await expect(page.getByTestId("settings-edit").getByRole("heading")).toContainText("message.max.bytes");
  await page.goto("/settings");
  await page.getByTestId("edit-log.retention.bytes").click();
  await expect(page.getByTestId("settings-edit")).toBeVisible();
  await page.goBack();
  await expect(page.getByTestId("settings-edit")).toBeHidden();
  await expect(page).toHaveURL(/\/settings$/);
});

test("Settings: a value Kafka refuses is refused in Kafka's words; Check asks the broker; Save changes the table", async ({ page }) => {
  await page.goto("/settings?edit=log.segment.bytes");
  const dialog = page.getByTestId("settings-edit");
  await dialog.getByTestId("cfg-value").fill("1000");
  await dialog.getByTestId("cfg-check").click();
  await expect(dialog.getByTestId("refusal")).toBeVisible();
  await expect(dialog.getByTestId("refusal-name")).toHaveText(/^[A-Z_]+$/);
  await expect(dialog.getByTestId("refusal-message")).not.toBeEmpty();
  // A value it takes: Check (validate_only) changes nothing; Save does.
  await page.goto("/settings?edit=log.retention.ms");
  await dialog.getByTestId("cfg-value").fill("259200000");
  await expect(dialog.getByTestId("cfg-unit")).toHaveText("ms = 3 d");
  await dialog.getByTestId("cfg-check").click();
  await expect(dialog.getByTestId("cfg-checked")).toContainText("259200000");
  await dialog.getByTestId("cfg-save").click();
  await expect(dialog).toBeHidden();
  await expect(page.getByTestId("saved-card")).toContainText("log.retention.ms");
  const row = page.getByRole("table", { name: "Dynamic broker configs" }).locator("tr", { hasText: "log.retention.ms" });
  await expect(row).toContainText("259200000");
  await expect(row).toContainText("Dynamic default broker");
  // The broker kept it: a fresh load says the same.
  await page.reload();
  await expect(page.getByRole("table", { name: "Dynamic broker configs" }).locator("tr", { hasText: "log.retention.ms" })).toContainText("259200000");
});
