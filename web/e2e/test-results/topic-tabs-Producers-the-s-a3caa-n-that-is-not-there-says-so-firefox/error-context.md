# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: topic-tabs.spec.ts >> Producers: the stats, the partition in the address, a producer's batches open in place; a transaction that is not there says so
- Location: tests/topic-tabs.spec.ts:84:1

# Error details

```
Error: expect(locator).toBeVisible() failed

Locator: getByText('No transaction called no-such-transaction')
Expected: visible
Error: strict mode violation: getByText('No transaction called no-such-transaction') resolved to 2 elements:
    1) <h3 class="text-[15px] font-semibold text-bright">No transaction called no-such-transaction</h3> aka getByRole('heading', { name: 'No transaction called no-such' })
    2) <div class="max-w-md text-[13.5px] text-muted-foreground">The broker knows no transaction called no-such-tr…</div> aka getByText('The broker knows no')

Call log:
  - Expect "toBeVisible" getByText('No transaction called no-such-transaction') with timeout 8000ms
  - waiting for getByText('No transaction called no-such-transaction')

```

# Page snapshot

```yaml
- generic [ref=f1e2]:
  - generic [ref=f1e3]:
    - generic [ref=f1e6]:
      - link "doze-kafka, Home" [ref=f1e8] [cursor=pointer]:
        - /url: /
        - generic [ref=f1e12]: doze-kafka
      - list [ref=f1e16]:
        - listitem [ref=f1e17]:
          - link "Home" [ref=f1e18] [cursor=pointer]:
            - /url: /
        - listitem [ref=f1e23]:
          - link "Live" [ref=f1e24] [cursor=pointer]:
            - /url: /live
        - listitem [ref=f1e28]:
          - link "Topics" [ref=f1e29] [cursor=pointer]:
            - /url: /topics
          - generic: "3"
        - listitem [ref=f1e35]:
          - link "Groups" [ref=f1e36] [cursor=pointer]:
            - /url: /groups
          - generic: "1"
        - listitem [ref=f1e43]:
          - link "Access" [ref=f1e44] [cursor=pointer]:
            - /url: /access
        - listitem [ref=f1e48]:
          - link "Lab" [ref=f1e49] [cursor=pointer]:
            - /url: /lab
        - listitem [ref=f1e55]:
          - link "Settings" [ref=f1e56] [cursor=pointer]:
            - /url: /settings
      - list [ref=f1e62]:
        - listitem [ref=f1e63]:
          - link "Classic console" [ref=f1e64] [cursor=pointer]:
            - /url: /classic/
      - button "Toggle Sidebar" [ref=f1e70]
    - main [ref=f1e71]:
      - generic [ref=f1e72]:
        - button "Toggle Sidebar" [ref=f1e73]
        - button "Jump to a topic or group ⌘K" [ref=f1e75]:
          - generic [ref=f1e79]: Jump to a topic or group
          - generic [ref=f1e80]: ⌘K
        - generic [ref=f1e81]:
          - button "127.0.0.1:39092" [ref=f1e82]
          - generic [ref=f1e87]: Kafka 4 profile
          - link "Open" [ref=f1e88] [cursor=pointer]:
            - /url: /access
      - main [ref=f1e90]:
        - generic [ref=f1e91]:
          - generic [ref=f1e92]:
            - generic [ref=f1e93]:
              - navigation "Breadcrumb" [ref=f1e94]:
                - list [ref=f1e95]:
                  - listitem [ref=f1e96]:
                    - link "Topics" [ref=f1e97] [cursor=pointer]:
                      - /url: /topics
              - generic [ref=f1e98]:
                - generic [ref=f1e99]: Updated 05:25:55 UTC
                - combobox "Refresh every" [ref=f1e100]:
                  - generic: "Refresh: Off"
                - button "Refresh" [ref=f1e101]
            - generic [ref=f1e102]:
              - generic [ref=f1e103]:
                - heading "orders" [level=1] [ref=f1e104]
                - generic [ref=f1e105]: delete
              - generic [ref=f1e107]:
                - link "Partitions" [ref=f1e108] [cursor=pointer]:
                  - /url: /topics/orders/partitions
                - button "Produce" [ref=f1e109]
                - button "Delete topic" [ref=f1e110]
          - generic [ref=f1e111]:
            - generic [ref=f1e112]:
              - term [ref=f1e113]: Partitions
              - definition [ref=f1e115]: "3"
              - generic "all online" [ref=f1e116]
            - generic [ref=f1e117]:
              - term [ref=f1e118]: Retention
              - definition [ref=f1e120]: for ever
              - generic "delete" [ref=f1e121]
            - generic [ref=f1e122]:
              - term [ref=f1e123]: Records
              - definition [ref=f1e125]: "17"
            - generic [ref=f1e126]:
              - term [ref=f1e127]: Size on disk
              - definition [ref=f1e129]: 1.4 KiB
            - generic [ref=f1e130]:
              - term [ref=f1e131]: Last write
              - definition [ref=f1e133]: 05:25:34
              - generic "UTC" [ref=f1e134]
          - generic [ref=f1e135]: No open transactions.
          - navigation "Topic" [ref=f1e140]:
            - list [ref=f1e141]:
              - listitem [ref=f1e142]:
                - link "Messages" [ref=f1e143] [cursor=pointer]:
                  - /url: /topics/orders/messages
              - listitem [ref=f1e144]:
                - link "Log" [ref=f1e145] [cursor=pointer]:
                  - /url: /topics/orders/log
              - listitem [ref=f1e146]:
                - link "Config" [ref=f1e147] [cursor=pointer]:
                  - /url: /topics/orders/config
              - listitem [ref=f1e148]:
                - link "Consumers 1" [ref=f1e149] [cursor=pointer]:
                  - /url: /topics/orders/consumers
                  - text: Consumers
                  - generic [ref=f1e150]: "1"
              - listitem [ref=f1e151]:
                - link "Producers 0" [ref=f1e152] [cursor=pointer]:
                  - /url: /topics/orders/producers
                  - text: Producers
                  - generic [ref=f1e153]: "0"
          - generic [ref=f1e155]:
            - heading "No transaction called no-such-transaction" [level=3] [ref=f1e160]
            - generic [ref=f1e161]: The broker knows no transaction called no-such-transaction, or the console may not describe it. A transactional id is forgotten after 7 days idle.
            - link "Back to the producers" [ref=f1e163] [cursor=pointer]:
              - /url: /topics/orders/producers
      - generic [ref=f1e164]:
        - heading "Jump to a topic or group" [level=2] [ref=f1e165]
        - paragraph [ref=f1e166]: Type a name, then Enter
  - region "Notifications alt+T"
```

# Test source

```ts
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
  80  |     await expect(opener).toBeFocused();
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
> 102 |   await expect(page.getByText("No transaction called no-such-transaction")).toBeVisible();
      |                                                                             ^ Error: expect(locator).toBeVisible() failed
  103 |   await page.getByRole("link", { name: "Back to the producers" }).click();
  104 |   await expect(page).toHaveURL(/\/topics\/orders\/producers$/);
  105 | });
  106 | 
```