# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: topic-tabs.spec.ts >> Compact now on a compacted topic: a Dialog with the planner's verdict, and its outcome in the broker's words
- Location: tests/topic-tabs.spec.ts:63:1

# Error details

```
Error: expect(locator).toBeFocused() failed

Locator:  getByTestId('compact-now')
Expected: focused
Received: inactive
Timeout:  8000ms

Call log:
  - Expect "toBeFocused" getByTestId('compact-now') with timeout 8000ms
  - waiting for getByTestId('compact-now')
    20 × locator resolved to <button data-slot="button" data-size="default" data-variant="default" data-testid="compact-now" class="inline-flex shrink-0 items-center justify-center gap-2 rounded-md text-sm font-medium whitespace-nowrap transition-all outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 [&_svg]:pointer-events-none [&_svg]:shrin…>…</button>
       - unexpected value "inactive"

```

```yaml
- button "Compact now"
```

# Test source

```ts
  1   | // The other pages of a topic (slice R1a): Partitions, Consumers, Log (with the Lab's Compact now on a compacted topic),
  2   | // Producers and a transaction. Each asserts what the page says and what its controls do.
  3   | import { expect, overflowX, test } from "./fixtures";
  4   | 
  5   | test("Partitions: its own head, a row per partition, and Delete records asks for its dialog in the address", async ({ page }) => {
  6   |   await page.goto("/topics/orders");
  7   |   await page.getByTestId("act-partitions").click();
  8   |   await expect(page).toHaveURL(/\/topics\/orders\/partitions$/);
  9   |   await expect(page.locator("h1")).toHaveText("Partitions");
  10  |   await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toHaveText(/Topics\s*›\s*orders/);
  11  |   await expect(page.getByTestId("topic-tabs")).toHaveCount(0);
  12  |   const table = page.getByRole("table", { name: "Partitions" });
  13  |   await expect(table.locator("tbody tr[data-row]").first()).toBeVisible();
  14  |   const n = await table.locator("tbody tr[data-row]").count();
  15  |   expect(n).toBeGreaterThanOrEqual(3);
  16  |   await expect(table.locator("#r-0")).toContainText("orders-0");
  17  |   await expect(table.locator("#r-0")).toContainText(/clear|held/);
  18  |   await expect(page.getByTestId("list-offsets-call")).toContainText("ListOffsets v");
  19  |   await page.getByTestId("delete-records-1").click();
  20  |   await expect(page).toHaveURL(/dialog=delete-records/);
  21  |   await expect(page).toHaveURL(/partition=1/);
  22  |   await page.getByRole("navigation", { name: "Breadcrumb" }).getByRole("link", { name: "orders" }).click();
  23  |   await expect(page).toHaveURL(/\/topics\/orders\/messages/);
  24  |   await expect(page.locator("h1")).toHaveText("orders");
  25  | });
  26  | 
  27  | test("Consumers: the groups that committed on the topic, with their lag, and a group's link goes to it", async ({ page }) => {
  28  |   await page.goto("/topics/orders/consumers");
  29  |   const reader = page.getByTestId("reader").first();
  30  |   await expect(reader).toBeVisible();
  31  |   const group = (await reader.getAttribute("data-group"))!;
  32  |   await expect(reader.getByTestId("behind")).toHaveText(/^[\d,]+ behind$/);
  33  |   await expect(reader.getByRole("table")).toContainText("orders-0");
  34  |   const count = await page.getByTestId("reader").count();
  35  |   await expect(page.getByTestId("topic-tabs").getByRole("link", { name: /^Consumers/ })).toContainText(String(count));
  36  |   await reader.getByRole("link", { name: group }).click();
  37  |   await expect(page).toHaveURL(new RegExp(`/groups/${group}$`));
  38  | });
  39  | 
  40  | test("Log: the ruler, the cards and the segments; a segment opens in place; the partition is in the address", async ({ page }) => {
  41  |   await page.goto("/topics/orders/log");
  42  |   await expect(page.getByTestId("log")).toBeVisible();
  43  |   const strip = page.getByTestId("log").locator("[data-slot=stat-strip]");
  44  |   for (const label of ["Log", "Start offset", "End offset", "Last stable", "Records", "Segments", "Size"]) await expect(strip).toContainText(label);
  45  |   await expect(page.getByTestId("ruler")).toBeVisible();
  46  |   await expect(page.getByTestId("durability")).toContainText("Durability");
  47  |   await expect(page.getByTestId("planner")).toBeVisible();
  48  |   const seg = page.getByTestId("segment").first().getByRole("button");
  49  |   const was = await seg.getAttribute("aria-expanded");
  50  |   await seg.click();
  51  |   await expect(seg).toHaveAttribute("aria-expanded", was === "true" ? "false" : "true");
  52  |   await page.getByTestId("log-partition").getByRole("radio", { name: "1" }).click();
  53  |   await expect(page).toHaveURL(/partition=1/);
  54  |   await expect(page.getByTestId("log")).toContainText("partition 1 of");
  55  |   // A topic that only deletes has no Compact now.
  56  |   await expect(page.getByTestId("compact-now")).toHaveCount(0);
  57  |   for (const width of [1024, 1440]) {
  58  |     await page.setViewportSize({ width, height: 900 });
  59  |     expect(await overflowX(page), `${width}`).toBeLessThanOrEqual(0);
  60  |   }
  61  | });
  62  | 
  63  | test("Compact now on a compacted topic: a Dialog with the planner's verdict, and its outcome in the broker's words", async ({ page }) => {
  64  |   await page.goto("/topics/inventory.levels/log");
  65  |   await expect(page.getByTestId("log")).toBeVisible();
  66  |   const opener = page.getByTestId("compact-now");
  67  |   await opener.click();
  68  |   const dialog = page.getByTestId("compact-dialog");
  69  |   await expect(dialog).toBeVisible();
  70  |   await expect(dialog).toContainText("inventory.levels-0");
  71  |   if (await dialog.getByTestId("compact-run").count()) {
  72  |     // Due: run the round; the answer is a toast of what it did, or the refusal in the dialog.
  73  |     await dialog.getByTestId("compact-run").click();
  74  |     await expect(page.locator("[data-sonner-toast]").filter({ hasText: "Ran the cleaner on inventory.levels-0" }).or(dialog.getByTestId("compact-refused"))).toBeVisible();
  75  |   } else {
  76  |     // Not due: the planner's sentence says why, and nothing runs.
  77  |     await expect(dialog.getByTestId("compact-blocked")).toContainText(/planner finds nothing due|cleaner is off|nothing to clean|Lab is switched off/);
  78  |     await page.keyboard.press("Escape");
  79  |     await expect(dialog).toBeHidden();
> 80  |     await expect(opener).toBeFocused();
      |                          ^ Error: expect(locator).toBeFocused() failed
  81  |   }
  82  | });
  83  | 
  84  | test("Producers: the stats, the partition in the address, a producer's batches open in place; a transaction that is not there says so", async ({ page }) => {
  85  |   await page.goto("/topics/orders/producers");
  86  |   await expect(page.getByTestId("producers")).toBeVisible();
  87  |   const strip = page.getByTestId("producers").locator("[data-slot=stat-strip]");
  88  |   for (const label of ["Producers", "Open transactions", "Last stable", "High watermark", "Held back"]) await expect(strip).toContainText(label);
  89  |   await expect(page.getByTestId("aborted")).toContainText("Aborted transactions remembered:");
  90  |   const producer = page.getByTestId("producer").first();
  91  |   if (await producer.count()) {
  92  |     const toggle = producer.getByTestId("batches-toggle");
  93  |     const open = await toggle.getAttribute("aria-expanded");
  94  |     await toggle.click();
  95  |     await expect(toggle).toHaveAttribute("aria-expanded", open === "true" ? "false" : "true");
  96  |     await expect(producer.getByTestId("batches")).toHaveCount(open === "true" ? 0 : 1);
  97  |   }
  98  |   await page.getByTestId("producers-partition").getByRole("radio", { name: "orders-2" }).click();
  99  |   await expect(page).toHaveURL(/partition=2/);
  100 |   await expect(page.getByTestId("producers")).toContainText("Producers on orders-2");
  101 |   await page.goto("/topics/orders/transactions/no-such-transaction");
  102 |   await expect(page.getByText("No transaction called no-such-transaction")).toBeVisible();
  103 |   await page.getByRole("link", { name: "Back to the producers" }).click();
  104 |   await expect(page).toHaveURL(/\/topics\/orders\/producers$/);
  105 | });
  106 | 
```