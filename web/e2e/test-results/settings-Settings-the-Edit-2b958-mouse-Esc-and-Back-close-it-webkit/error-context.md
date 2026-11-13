# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: settings.spec.ts >> Settings: the Edit dialog has its own address, its Select and toggle work by mouse, Esc and Back close it
- Location: tests/settings.spec.ts:48:1

# Error details

```
Error: expect(locator).toBeFocused() failed

Locator:  getByTestId('edit-log.retention.ms')
Expected: focused
Received: inactive
Timeout:  8000ms

Call log:
  - Expect "toBeFocused" getByTestId('edit-log.retention.ms') with timeout 8000ms
  - waiting for getByTestId('edit-log.retention.ms')
    20 × locator resolved to <button data-size="sm" data-slot="button" data-variant="outline" aria-label="Edit log.retention.ms" data-testid="edit-log.retention.ms" class="inline-flex shrink-0 items-center justify-center text-sm font-medium whitespace-nowrap transition-all outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 [&_svg]:pointer-ev…>…</button>
       - unexpected value "inactive"

```

```yaml
- button "Edit log.retention.ms": Edit
```

# Test source

```ts
  1   | // Settings (slice R4): the five dynamic broker configs and their Edit dialog (Select, ToggleGroup, Check, Save, Kafka's refusal),
  2   | // the version profile, intervals, weight, data directory and flags.
  3   | import { choose, expect, test } from "./fixtures";
  4   | import { noMemory, noNativeSelect, widths } from "./r4";
  5   | 
  6   | test("Settings: every section, figures as tiles, the five configs, no memory figure, no native control, no overflow", async ({ page }) => {
  7   |   await page.goto("/settings");
  8   |   await expect(page.locator("h1")).toHaveText("Settings");
  9   |   const table = page.getByRole("table", { name: "Dynamic broker configs" });
  10  |   for (const key of ["log.segment.bytes", "log.index.interval.bytes", "message.max.bytes", "log.retention.bytes", "log.retention.ms"]) await expect(table).toContainText(key);
  11  |   for (const h of ["Broker configs", "Version profile", "Intervals", "Weight", "Data", "Flags"]) await expect(page.getByRole("heading", { name: h, exact: true })).toBeVisible();
  12  |   await expect(page.locator("[data-slot=stat-strip]").first()).toContainText("Set at run time");
  13  |   await expect(page.getByRole("table", { name: "APIs offered" })).toContainText("Produce");
  14  |   await expect(page.getByRole("table", { name: "Intervals" })).toContainText("--log-retention-check-interval");
  15  |   await expect(page.locator("[data-slot=stat]").filter({ hasText: "Binary" })).toContainText(/MB/);
  16  |   await noMemory(page);
  17  |   await noNativeSelect(page);
  18  |   await widths(page);
  19  | });
  20  | 
  21  | test("Settings: the data directory wraps its whole path inside its box and copies it", async ({ page, browserName }) => {
  22  |   await page.goto("/settings");
  23  |   const path = page.getByTestId("data-card").getByTestId("path-text");
  24  |   await expect(path).toHaveText(/\//);
  25  |   const [box, inner] = [await page.getByTestId("data-card").locator("[data-slot=path-value]").boundingBox(), await path.boundingBox()];
  26  |   expect(inner!.x + inner!.width).toBeLessThanOrEqual(box!.x + box!.width + 1);
  27  |   // The text is all there, not cut with an ellipsis.
  28  |   expect(await path.evaluate((el) => el.scrollWidth <= el.clientWidth + 1)).toBe(true);
  29  |   if (browserName === "chromium") {
  30  |     await page.getByTestId("data-card").getByRole("button", { name: /Copy path/ }).click();
  31  |     await expect(page.locator("[data-sonner-toast]").filter({ hasText: "Path copied" })).toBeVisible();
  32  |     expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(await path.innerText());
  33  |   }
  34  | });
  35  | 
  36  | test("Settings: Show all APIs, the flags toggle", async ({ page }) => {
  37  |   await page.goto("/settings");
  38  |   const apis = page.getByRole("table", { name: "APIs offered" }).locator("tbody tr");
  39  |   await expect(apis).toHaveCount(8);
  40  |   await page.getByTestId("show-apis").click();
  41  |   expect(await apis.count()).toBeGreaterThan(8);
  42  |   const flags = page.getByRole("table", { name: "Flags" });
  43  |   await page.getByTestId("flags-which").getByRole("radio", { name: /^All/ }).click();
  44  |   await expect(flags).toContainText("--data-dir");
  45  |   expect(await flags.locator("tbody tr").count()).toBeGreaterThan(10);
  46  | });
  47  | 
  48  | test("Settings: the Edit dialog has its own address, its Select and toggle work by mouse, Esc and Back close it", async ({ page }) => {
  49  |   await page.goto("/settings");
  50  |   const edit = page.getByTestId("edit-log.retention.ms");
  51  |   await edit.click();
  52  |   await expect(page).toHaveURL(/edit=log.retention.ms/);
  53  |   const dialog = page.getByTestId("settings-edit");
  54  |   await expect(dialog.getByRole("heading")).toContainText("log.retention.ms");
  55  |   await expect(dialog.getByTestId("call-box")).toContainText("IncrementalAlterConfigs");
  56  |   // The config Select (shadcn, not native) moves the dialog to another config.
  57  |   await choose(page, dialog.getByTestId("cfg-key"), "log.segment.bytes");
  58  |   await expect(page).toHaveURL(/edit=log.segment.bytes/);
  59  |   await expect(dialog.getByRole("heading")).toContainText("log.segment.bytes");
  60  |   await noNativeSelect(page);
  61  |   // Applies to: This broker.
  62  |   await dialog.getByTestId("cfg-scope").getByRole("radio", { name: /This broker/ }).click();
  63  |   await expect(page).toHaveURL(/scope=this/);
  64  |   await expect(dialog.getByTestId("call-box")).toContainText("this broker");
  65  |   // The unit hint follows what is typed.
  66  |   await dialog.getByTestId("cfg-value").fill("268435456");
  67  |   await expect(dialog.getByTestId("cfg-unit")).toHaveText("B = 256 MiB");
  68  |   await page.keyboard.press("Escape");
  69  |   await expect(dialog).toBeHidden();
  70  |   await expect(page).not.toHaveURL(/edit=/);
> 71  |   await expect(edit).toBeFocused();
      |                      ^ Error: expect(locator).toBeFocused() failed
  72  |   // A deep link opens it; Back from an opened one closes it.
  73  |   await page.goto("/settings?edit=message.max.bytes");
  74  |   await expect(page.getByTestId("settings-edit").getByRole("heading")).toContainText("message.max.bytes");
  75  |   await page.goto("/settings");
  76  |   await page.getByTestId("edit-log.retention.bytes").click();
  77  |   await expect(page.getByTestId("settings-edit")).toBeVisible();
  78  |   await page.goBack();
  79  |   await expect(page.getByTestId("settings-edit")).toBeHidden();
  80  |   await expect(page).toHaveURL(/\/settings$/);
  81  | });
  82  | 
  83  | test("Settings: a value Kafka refuses is refused in Kafka's words; Check asks the broker; Save changes the table", async ({ page }) => {
  84  |   await page.goto("/settings?edit=log.segment.bytes");
  85  |   const dialog = page.getByTestId("settings-edit");
  86  |   await dialog.getByTestId("cfg-value").fill("1000");
  87  |   await dialog.getByTestId("cfg-check").click();
  88  |   await expect(dialog.getByTestId("refusal")).toBeVisible();
  89  |   await expect(dialog.getByTestId("refusal-name")).toHaveText(/^[A-Z_]+$/);
  90  |   await expect(dialog.getByTestId("refusal-message")).not.toBeEmpty();
  91  |   // A value it takes: Check (validate_only) changes nothing; Save does.
  92  |   await page.goto("/settings?edit=log.retention.ms");
  93  |   await dialog.getByTestId("cfg-value").fill("259200000");
  94  |   await expect(dialog.getByTestId("cfg-unit")).toHaveText("ms = 3 d");
  95  |   await dialog.getByTestId("cfg-check").click();
  96  |   await expect(dialog.getByTestId("cfg-checked")).toContainText("259200000");
  97  |   await dialog.getByTestId("cfg-save").click();
  98  |   await expect(dialog).toBeHidden();
  99  |   await expect(page.getByTestId("saved-card")).toContainText("log.retention.ms");
  100 |   const row = page.getByRole("table", { name: "Dynamic broker configs" }).locator("tr", { hasText: "log.retention.ms" });
  101 |   await expect(row).toContainText("259200000");
  102 |   await expect(row).toContainText("Dynamic default broker");
  103 |   // The broker kept it: a fresh load says the same.
  104 |   await page.reload();
  105 |   await expect(page.getByRole("table", { name: "Dynamic broker configs" }).locator("tr", { hasText: "log.retention.ms" })).toContainText("259200000");
  106 | });
  107 | 
```