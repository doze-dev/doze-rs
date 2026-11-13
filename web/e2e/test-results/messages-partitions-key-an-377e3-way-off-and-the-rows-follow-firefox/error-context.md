# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: messages.spec.ts >> partitions, key and time: each is in the address, shown as a chip with a way off, and the rows follow
- Location: tests/messages.spec.ts:49:1

# Error details

```
Error: expect(page).toHaveURL(expected) failed

Expected pattern: /partition=1/
Received string:  "http://127.0.0.1:39094/topics/orders/messages?partition=%221%22"
Timeout: 8000ms

Call log:
  - Expect "toHaveURL" with timeout 8000ms
    19 × locator resolved to <html lang="en">…</html>
       - unexpected value "http://127.0.0.1:39094/topics/orders/messages?partition=%221%22"

```

```yaml
- link "doze-kafka, Home":
  - /url: /
  - text: doze-kafka
- list:
  - listitem:
    - link "Home":
      - /url: /
  - listitem:
    - link "Live":
      - /url: /live
  - listitem:
    - link "Topics":
      - /url: /topics
    - text: "3"
  - listitem:
    - link "Groups":
      - /url: /groups
    - text: "1"
  - listitem:
    - link "Access":
      - /url: /access
  - listitem:
    - link "Lab":
      - /url: /lab
  - listitem:
    - link "Settings":
      - /url: /settings
- list:
  - listitem:
    - link "Classic console":
      - /url: /classic/
- button "Toggle Sidebar"
- main:
  - button "Toggle Sidebar"
  - button "Jump to a topic or group ⌘K"
  - button "127.0.0.1:39092"
  - text: Kafka 4 profile
  - link "Open":
    - /url: /access
  - main:
    - navigation "Breadcrumb":
      - list:
        - listitem:
          - link "Topics":
            - /url: /topics
    - text: Updated 05:25:12 UTC
    - combobox "Refresh every": "Refresh: Off"
    - button "Refresh"
    - heading "orders" [level=1]
    - text: delete
    - link "Partitions":
      - /url: /topics/orders/partitions
    - button "Produce"
    - button "Delete topic"
    - term: Partitions
    - definition: "3"
    - text: all online
    - term: Retention
    - definition: for ever
    - text: delete
    - term: Records
    - definition: "12"
    - term: Size on disk
    - definition: 655 B
    - term: Last write
    - definition: 05:25:05
    - text: UTC No open transactions.
    - navigation "Topic":
      - list:
        - listitem:
          - link "Messages":
            - /url: /topics/orders/messages
        - listitem:
          - link "Log":
            - /url: /topics/orders/log
        - listitem:
          - link "Config":
            - /url: /topics/orders/config
        - listitem:
          - link "Consumers 1":
            - /url: /topics/orders/consumers
        - listitem:
          - link "Producers 0":
            - /url: /topics/orders/producers
    - combobox "Partitions": orders-1
    - button "Clear partitions"
    - search:
      - textbox "Find a key, or go to an offset":
        - /placeholder: Key, or offset
    - button "From time"
    - text: Reading as
    - radiogroup "Reading as":
      - radio "read_uncommitted" [checked]
      - radio "read_committed"
    - paragraph: The fetch returned 3 records, offsets 0 to 2, the end of the partition.
    - text: partition 1
    - button "Remove partition 1"
    - button "Clear all"
    - radiogroup "Snapshot or Live tail":
      - radio "Snapshot" [checked]
      - radio "Live tail"
    - combobox "Tail speed" [disabled]: Relaxed
    - text: Snapshot
    - table "Records, newest first":
      - rowgroup:
        - row "Partition Offset Timestamp Key Value Value size For read_committed":
          - columnheader "Partition":
            - button "Partition"
          - columnheader "Offset":
            - button "Offset"
          - columnheader "Timestamp":
            - button "Timestamp"
          - columnheader "Key":
            - button "Key"
          - columnheader "Value":
            - button "Value"
          - columnheader "Value size":
            - button "Value size"
          - columnheader "For read_committed":
            - button "For read_committed"
      - rowgroup:
        - 'row "1 2 05:25:05.199 orders-7 {\"i\":7,\"topic\":\"orders\"} 24 B Committed"':
          - cell "1"
          - cell "2":
            - button "2"
          - cell "05:25:05.199"
          - cell "orders-7"
          - 'cell "{\"i\":7,\"topic\":\"orders\"}"'
          - cell "24 B"
          - cell "Committed"
        - 'row "1 1 05:25:05.199 orders-6 {\"i\":6,\"topic\":\"orders\"} 24 B Committed"':
          - cell "1"
          - cell "1":
            - button "1"
          - cell "05:25:05.199"
          - cell "orders-6"
          - 'cell "{\"i\":6,\"topic\":\"orders\"}"'
          - cell "24 B"
          - cell "Committed"
        - 'row "1 0 05:25:05.199 orders-4 {\"i\":4,\"topic\":\"orders\"} 24 B Committed"':
          - cell "1"
          - cell "0":
            - button "0"
          - cell "05:25:05.199"
          - cell "orders-4"
          - 'cell "{\"i\":4,\"topic\":\"orders\"}"'
          - cell "24 B"
          - cell "Committed"
    - text: Newest first · 3 records
    - paragraph: 3 of 3 partitions online
    - button "How this read works"
  - heading "Jump to a topic or group" [level=2]
  - paragraph: Type a name, then Enter
- region "Notifications alt+T"
```

# Test source

```ts
  1   | // A topic's frame and its Messages (slice R1a): tabs as routes; every filter in the address (partitions, offset or key,
  2   | // time, isolation, Older); the record Sheet open, close, Esc, Back and a deep link; the Live tail on a broker with traffic.
  3   | import { boxes, choose, expect, overflowX, test } from "./fixtures";
  4   | import { produce } from "./kafka";
  5   | 
  6   | const rows = (page: import("@playwright/test").Page) => page.getByRole("table", { name: "Records, newest first" }).locator("tbody tr[data-row]");
  7   | 
  8   | test("the frame: one h1, the stats, the transaction region, five tabs as routes, and a tab switch moves nothing above it", async ({ page }) => {
  9   |   await page.goto("/topics/orders");
  10  |   await expect(page.locator("h1")).toHaveText("orders");
  11  |   await expect(page.locator("h1")).toHaveCount(1);
  12  |   await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toHaveText("Topics");
  13  |   const strip = page.locator("[data-slot=stat-strip]").first();
  14  |   for (const label of ["Partitions", "Retention", "Records", "Size on disk", "Last write"]) await expect(strip).toContainText(label);
  15  |   await expect(page.getByTestId("txn-region")).toBeVisible();
  16  |   const tabs = page.getByTestId("topic-tabs");
  17  |   await expect(tabs.getByRole("link")).toHaveText([/^Messages$/, /^Log$/, /^Config$/, /^Consumers\s*\d+$/, /^Producers\s*\d+$/]);
  18  |   const before = await boxes(page, ["h1", "[data-testid=topic-tabs]", "[data-testid=txn-region]"]);
  19  |   for (const [tab, url] of [
  20  |     ["Log", /\/topics\/orders\/log$/],
  21  |     ["Consumers", /\/topics\/orders\/consumers$/],
  22  |     ["Producers", /\/topics\/orders\/producers$/],
  23  |     ["Messages", /\/topics\/orders\/messages$/],
  24  |   ] as const) {
  25  |     await tabs.getByRole("link", { name: new RegExp(`^${tab}`) }).click();
  26  |     await expect(page).toHaveURL(url);
  27  |     await expect(tabs.getByRole("link", { name: new RegExp(`^${tab}`) })).toHaveAttribute("aria-current", "page");
  28  |     expect(await boxes(page, ["h1", "[data-testid=topic-tabs]", "[data-testid=txn-region]"]), tab).toEqual(before);
  29  |   }
  30  |   // A name that is no topic says so.
  31  |   await page.goto("/topics/no-such-topic-here");
  32  |   await expect(page.locator("h1")).toHaveText("No such topic");
  33  | });
  34  | 
  35  | test("isolation: the ToggleGroup is in the address, says what read_committed holds back, and Back restores it", async ({ page }) => {
  36  |   await page.goto("/topics/orders/messages");
  37  |   await expect(rows(page).first()).toBeVisible();
  38  |   const iso = page.getByTestId("isolation");
  39  |   await iso.getByRole("radio", { name: "read_committed" }).click();
  40  |   await expect(page).toHaveURL(/isolation=read_committed/);
  41  |   await expect(page.getByTestId("isolation-note")).toContainText("Reading as read_committed.");
  42  |   await expect(page.getByTestId("explainers").getByRole("button", { name: "How this read works" })).toHaveAttribute("aria-expanded", "true");
  43  |   await iso.getByRole("radio", { name: "read_uncommitted" }).click();
  44  |   await expect(page).not.toHaveURL(/isolation=/);
  45  |   await expect(page.getByTestId("isolation-note")).toHaveCount(0);
  46  |   // The toolbar never moved.
  47  | });
  48  | 
  49  | test("partitions, key and time: each is in the address, shown as a chip with a way off, and the rows follow", async ({ page }) => {
  50  |   await page.goto("/topics/orders/messages");
  51  |   await expect(rows(page).first()).toBeVisible();
  52  |   const toolbar = await boxes(page, ["[data-testid=toolbar]", "[data-testid=filter-line]"]);
  53  |   // Partitions: the MultiSelect.
  54  |   await page.getByRole("combobox", { name: "Partitions" }).click();
  55  |   await page.getByRole("option", { name: /^orders-1\s/ }).click();
  56  |   await page.keyboard.press("Escape");
> 57  |   await expect(page).toHaveURL(/partition=1/);
      |                      ^ Error: expect(page).toHaveURL(expected) failed
  58  |   await expect(page.getByTestId("chip-p1")).toBeVisible();
  59  |   await expect(rows(page).first()).toBeVisible();
  60  |   for (const r of await rows(page).all()) expect(await r.locator("td").first().innerText()).toBe("1");
  61  |   expect(await boxes(page, ["[data-testid=toolbar]", "[data-testid=filter-line]"])).toEqual(toolbar);
  62  |   await page.getByRole("button", { name: "Remove partition 1" }).click();
  63  |   await expect(page).not.toHaveURL(/partition=/);
  64  |   // A key: the first row's, typed into the one box.
  65  |   const key = (await rows(page).first().locator("td").nth(3).innerText()).trim();
  66  |   await page.getByTestId("find").fill(key);
  67  |   await page.getByTestId("find").press("Enter");
  68  |   await expect(page).toHaveURL(new RegExp(`key=${encodeURIComponent(key).replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}`));
  69  |   await expect(page.getByTestId("chip-key")).toContainText(key);
  70  |   await expect(page.getByTestId("reach")).toBeVisible();
  71  |   await expect(rows(page).first()).toBeVisible();
  72  |   for (const r of await rows(page).all()) expect((await r.locator("td").nth(3).innerText()).trim()).toBe(key);
  73  |   // Digits are an offset.
  74  |   await page.getByTestId("find").fill("0");
  75  |   await page.getByTestId("find").press("Enter");
  76  |   await expect(page).toHaveURL(/offset=0/);
  77  |   await expect(page).not.toHaveURL(/key=/);
  78  |   await expect(page.getByTestId("chip-offset")).toBeVisible();
  79  |   // A time, from the Popover's field.
  80  |   await page.getByTestId("time-trigger").click();
  81  |   await page.getByRole("button", { name: "24 hours ago" }).click();
  82  |   await expect(page.getByTestId("time-input")).toHaveValue(/^\d{4}-\d\d-\d\d \d\d:\d\d$/);
  83  |   await page.getByTestId("time-apply").click();
  84  |   await expect(page).toHaveURL(/time=/);
  85  |   await expect(page.getByTestId("chip-time")).toBeVisible();
  86  |   await page.getByRole("button", { name: "Clear all" }).click();
  87  |   await expect(page).not.toHaveURL(/offset=|time=|key=/);
  88  |   expect(await boxes(page, ["[data-testid=toolbar]", "[data-testid=filter-line]"])).toEqual(toolbar);
  89  | });
  90  | 
  91  | test("the record Sheet: a row opens it with its own address, Esc and Back close it, a deep link opens it", async ({ page }) => {
  92  |   await page.goto("/topics/orders/messages");
  93  |   const first = rows(page).first();
  94  |   await expect(first).toBeVisible();
  95  |   const opener = first.getByTestId("open-record");
  96  |   await opener.click();
  97  |   await expect(page).toHaveURL(/record=\d+%3A\d+|record=\d+:\d+/);
  98  |   const sheet = page.getByTestId("record-sheet");
  99  |   await expect(sheet).toBeVisible();
  100 |   await expect(sheet).toContainText("One record, completely.");
  101 |   await expect(sheet.getByTestId("record-value")).toBeVisible();
  102 |   // JSON is laid out, and As sent shows it as it came.
  103 |   if (await sheet.getByTestId("json-view").count()) {
  104 |     await expect(sheet.getByTestId("record-value")).toContainText("\n");
  105 |     await sheet.getByTestId("json-view").getByRole("radio", { name: "As sent" }).click();
  106 |     await expect(sheet.getByTestId("record-value")).not.toContainText("\n  ");
  107 |   }
  108 |   await page.keyboard.press("Escape");
  109 |   await expect(sheet).toBeHidden();
  110 |   await expect(page).not.toHaveURL(/record=/);
  111 |   await expect(opener).toBeFocused();
  112 |   // Back closes it too.
  113 |   await opener.click();
  114 |   await expect(sheet).toBeVisible();
  115 |   const url = page.url();
  116 |   await page.goBack();
  117 |   await expect(sheet).toBeHidden();
  118 |   await expect(page).not.toHaveURL(/record=/);
  119 |   // A deep link opens the page with the record over it; closing stays on the page.
  120 |   await page.goto(url);
  121 |   await expect(page.getByTestId("record-sheet")).toBeVisible();
  122 |   await expect(page.locator("h1")).toHaveText("orders");
  123 |   await page.getByTestId("record-sheet").getByRole("button", { name: "Close" }).click();
  124 |   await expect(page.getByTestId("record-sheet")).toBeHidden();
  125 |   await expect(page).toHaveURL(/\/topics\/orders\/messages$/);
  126 |   // A record that is not there says so in the Sheet.
  127 |   await page.goto("/topics/orders/messages?record=0:999999999");
  128 |   await expect(page.getByTestId("record-missing")).toContainText("There is no record at offset 999,999,999");
  129 | });
  130 | 
  131 | test("Older asks for more records, in the address", async ({ page }) => {
  132 |   await page.goto("/topics/orders/messages?limit=5");
  133 |   await expect(rows(page)).toHaveCount(5);
  134 |   await page.getByTestId("older").click();
  135 |   await expect(page).toHaveURL(/limit=10/);
  136 |   await expect(rows(page)).toHaveCount(10);
  137 | });
  138 | 
  139 | test("the Live tail: new rows at the top, no row rewritten, the header stable, held behind a pill while hovered", async ({ page }) => {
  140 |   test.setTimeout(120_000);
  141 |   await page.goto("/topics/orders/messages");
  142 |   await expect(rows(page).first()).toBeVisible();
  143 |   await page.getByTestId("mode").getByRole("radio", { name: "Live tail" }).click();
  144 |   await expect(page).toHaveURL(/mode=tail/);
  145 |   await choose(page, page.getByTestId("speed"), "Realtime");
  146 |   await expect(page.getByTestId("conn")).toHaveAttribute("data-state", "live", { timeout: 15_000 });
  147 |   await page.mouse.move(5, 5);
  148 |   const head = await page.getByRole("table", { name: "Records, newest first" }).locator("thead").elementHandle();
  149 |   const top = rows(page).first();
  150 |   const topId = await top.getAttribute("data-row");
  151 |   const topEl = await top.elementHandle();
  152 |   const topText = await top.innerText();
  153 |   const box = await boxes(page, ["[data-testid=toolbar]", "[data-testid=filter-line]", "thead"]);
  154 |   await produce("orders", 3, "tail-a");
  155 |   await expect(rows(page).first()).not.toHaveAttribute("data-row", topId!, { timeout: 45_000 });
  156 |   expect(await head!.evaluate((el) => el.isConnected)).toBe(true);
  157 |   expect(await topEl!.evaluate((el) => el.isConnected)).toBe(true);
```