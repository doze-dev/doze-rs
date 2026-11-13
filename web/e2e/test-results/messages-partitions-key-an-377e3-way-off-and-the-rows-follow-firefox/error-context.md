# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: messages.spec.ts >> partitions, key and time: each is in the address, shown as a chip with a way off, and the rows follow
- Location: tests/messages.spec.ts:49:1

# Error details

```
Error: expect(received).toEqual(expected) // deep equality

- Expected  - 2
+ Received  + 2

  Array [
    Array [
      304,
-     493,
+     454,
      1088,
      36,
    ],
    Array [
      304,
-     541,
+     502,
      1088,
      36,
    ],
  ]
```

# Page snapshot

```yaml
- generic [active] [ref=e1]:
  - generic [ref=e2]:
    - generic [ref=e3]:
      - generic [ref=e6]:
        - link "doze-kafka, Home" [ref=e8] [cursor=pointer]:
          - /url: /
          - generic [ref=e12]: doze-kafka
        - list [ref=e16]:
          - listitem [ref=e17]:
            - link "Home" [ref=e18] [cursor=pointer]:
              - /url: /
          - listitem [ref=e23]:
            - link "Live" [ref=e24] [cursor=pointer]:
              - /url: /live
          - listitem [ref=e28]:
            - link "Topics" [ref=e29] [cursor=pointer]:
              - /url: /topics
            - generic: "3"
          - listitem [ref=e35]:
            - link "Groups" [ref=e36] [cursor=pointer]:
              - /url: /groups
            - generic: "1"
          - listitem [ref=e43]:
            - link "Access" [ref=e44] [cursor=pointer]:
              - /url: /access
          - listitem [ref=e48]:
            - link "Lab" [ref=e49] [cursor=pointer]:
              - /url: /lab
          - listitem [ref=e55]:
            - link "Settings" [ref=e56] [cursor=pointer]:
              - /url: /settings
        - list [ref=e62]:
          - listitem [ref=e63]:
            - link "Classic console" [ref=e64] [cursor=pointer]:
              - /url: /classic/
        - button "Toggle Sidebar" [ref=e70]
      - main [ref=e71]:
        - generic [ref=e72]:
          - button "Toggle Sidebar" [ref=e73]
          - button "Jump to a topic or group ⌘K" [ref=e75]:
            - generic [ref=e79]: Jump to a topic or group
            - generic [ref=e80]: ⌘K
          - generic [ref=e81]:
            - button "127.0.0.1:39092" [ref=e82]
            - generic [ref=e87]: Kafka 4 profile
            - link "Open" [ref=e88] [cursor=pointer]:
              - /url: /access
        - main [ref=e90]:
          - generic [ref=e91]:
            - generic [ref=e92]:
              - generic [ref=e93]:
                - navigation "Breadcrumb" [ref=e94]:
                  - list [ref=e95]:
                    - listitem [ref=e96]:
                      - link "Topics" [ref=e97] [cursor=pointer]:
                        - /url: /topics
                - generic [ref=e98]:
                  - generic [ref=e99]: Updated 05:44:30 UTC
                  - combobox "Refresh every" [ref=e100]:
                    - generic: "Refresh: Off"
                  - button "Refresh" [ref=e101]
              - generic [ref=e102]:
                - generic [ref=e103]:
                  - heading "orders" [level=1] [ref=e104]
                  - generic [ref=e105]: delete
                - generic [ref=e107]:
                  - link "Partitions" [ref=e108] [cursor=pointer]:
                    - /url: /topics/orders/partitions
                  - button "Produce" [ref=e109]
                  - button "Delete topic" [ref=e110]
            - generic [ref=e111]:
              - generic [ref=e112]:
                - term [ref=e113]: Partitions
                - definition [ref=e115]: "3"
                - generic "all online" [ref=e116]
              - generic [ref=e117]:
                - term [ref=e118]: Retention
                - definition [ref=e120]: for ever
                - generic "delete" [ref=e121]
              - generic [ref=e122]:
                - term [ref=e123]: Records
                - definition [ref=e125]: "12"
              - generic [ref=e126]:
                - term [ref=e127]: Size on disk
                - definition [ref=e129]: 655 B
              - generic [ref=e130]:
                - term [ref=e131]: Last write
                - definition [ref=e133]: 05:44:11
                - generic "UTC" [ref=e134]
            - generic [ref=e135]: No open transactions.
            - navigation "Topic" [ref=e140]:
              - list [ref=e141]:
                - listitem [ref=e142]:
                  - link "Messages" [ref=e143] [cursor=pointer]:
                    - /url: /topics/orders/messages
                - listitem [ref=e144]:
                  - link "Log" [ref=e145] [cursor=pointer]:
                    - /url: /topics/orders/log
                - listitem [ref=e146]:
                  - link "Config" [ref=e147] [cursor=pointer]:
                    - /url: /topics/orders/config
                - listitem [ref=e148]:
                  - link "Consumers 1" [ref=e149] [cursor=pointer]:
                    - /url: /topics/orders/consumers
                    - text: Consumers
                    - generic [ref=e150]: "1"
                - listitem [ref=e151]:
                  - link "Producers 0" [ref=e152] [cursor=pointer]:
                    - /url: /topics/orders/producers
                    - text: Producers
                    - generic [ref=e153]: "0"
            - generic [ref=e155]:
              - generic [ref=e156]:
                - combobox "Partitions" [ref=e159]:
                  - generic [ref=e160]: All partitions
                - search [ref=e161]:
                  - textbox "Find a key, or go to an offset" [ref=e162]:
                    - /placeholder: Key, or offset
                - button "From time" [ref=e163]
                - generic [ref=e165]:
                  - generic [ref=e166]: Reading as
                  - radiogroup "Reading as" [ref=e167]:
                    - radio "read_uncommitted" [checked] [ref=e168]
                    - radio "read_committed" [ref=e169]
              - generic [ref=e170]:
                - generic [ref=e171]:
                  - generic [ref=e172]: "No filter: the newest records of every partition."
                  - generic [ref=e174]:
                    - radiogroup "Snapshot or Live tail" [ref=e176]:
                      - radio "Snapshot" [checked] [ref=e177]
                      - radio "Live tail" [ref=e178]
                    - combobox "Tail speed" [disabled] [ref=e179]:
                      - generic: Relaxed
                    - generic [ref=e180]: Snapshot
                - generic [ref=e183]:
                  - table "Records, newest first" [ref=e185]:
                    - rowgroup [ref=e194]:
                      - row [ref=e195]:
                        - columnheader "Partition" [ref=e196]:
                          - button "Partition" [ref=e197]
                        - columnheader "Offset" [ref=e200]:
                          - button "Offset" [ref=e201]
                        - columnheader "Timestamp" [ref=e204]:
                          - button "Timestamp" [ref=e205]
                        - columnheader "Key" [ref=e208]:
                          - button "Key" [ref=e209]
                        - columnheader "Value" [ref=e212]:
                          - button "Value" [ref=e213]
                        - columnheader "Value size" [ref=e216]:
                          - button "Value size" [ref=e217]
                        - columnheader "For read_committed" [ref=e220]:
                          - button "For read_committed" [ref=e221]
                    - rowgroup [ref=e224]:
                      - row [ref=e225] [cursor=pointer]:
                        - cell "0" [ref=e226]
                        - cell [ref=e227]:
                          - button "4" [ref=e228]
                        - cell "05:44:11.106" [ref=e229]
                        - cell "orders-9" [ref=e230]
                        - 'cell "{\"i\":9,\"topic\":\"orders\"}" [ref=e231]'
                        - cell "24 B" [ref=e232]
                        - cell "Committed" [ref=e233]
                      - row [ref=e236] [cursor=pointer]:
                        - cell "0" [ref=e237]
                        - cell [ref=e238]:
                          - button "3" [ref=e239]
                        - cell "05:44:11.106" [ref=e240]
                        - cell "orders-5" [ref=e241]
                        - 'cell "{\"i\":5,\"topic\":\"orders\"}" [ref=e242]'
                        - cell "24 B" [ref=e243]
                        - cell "Committed" [ref=e244]
                      - row [ref=e247] [cursor=pointer]:
                        - cell "0" [ref=e248]
                        - cell [ref=e249]:
                          - button "2" [ref=e250]
                        - cell "05:44:11.106" [ref=e251]
                        - cell "orders-2" [ref=e252]
                        - 'cell "{\"i\":2,\"topic\":\"orders\"}" [ref=e253]'
                        - cell "24 B" [ref=e254]
                        - cell "Committed" [ref=e255]
                      - row [ref=e258] [cursor=pointer]:
                        - cell "0" [ref=e259]
                        - cell [ref=e260]:
                          - button "1" [ref=e261]
                        - cell "05:44:11.106" [ref=e262]
                        - cell "orders-1" [ref=e263]
                        - 'cell "{\"i\":1,\"topic\":\"orders\"}" [ref=e264]'
                        - cell "24 B" [ref=e265]
                        - cell "Committed" [ref=e266]
                      - row [ref=e269] [cursor=pointer]:
                        - cell "0" [ref=e270]
                        - cell [ref=e271]:
                          - button "0" [ref=e272]
                        - cell "05:44:11.106" [ref=e273]
                        - cell "orders-0" [ref=e274]
                        - 'cell "{\"i\":0,\"topic\":\"orders\"}" [ref=e275]'
                        - cell "24 B" [ref=e276]
                        - cell "Committed" [ref=e277]
                      - row [ref=e280] [cursor=pointer]:
                        - cell "1" [ref=e281]
                        - cell [ref=e282]:
                          - button "2" [ref=e283]
                        - cell "05:44:11.106" [ref=e284]
                        - cell "orders-7" [ref=e285]
                        - 'cell "{\"i\":7,\"topic\":\"orders\"}" [ref=e286]'
                        - cell "24 B" [ref=e287]
                        - cell "Committed" [ref=e288]
                      - row [ref=e291] [cursor=pointer]:
                        - cell "1" [ref=e292]
                        - cell [ref=e293]:
                          - button "1" [ref=e294]
                        - cell "05:44:11.106" [ref=e295]
                        - cell "orders-6" [ref=e296]
                        - 'cell "{\"i\":6,\"topic\":\"orders\"}" [ref=e297]'
                        - cell "24 B" [ref=e298]
                        - cell "Committed" [ref=e299]
                      - row [ref=e302] [cursor=pointer]:
                        - cell "1" [ref=e303]
                        - cell [ref=e304]:
                          - button "0" [ref=e305]
                        - cell "05:44:11.106" [ref=e306]
                        - cell "orders-4" [ref=e307]
                        - 'cell "{\"i\":4,\"topic\":\"orders\"}" [ref=e308]'
                        - cell "24 B" [ref=e309]
                        - cell "Committed" [ref=e310]
                      - row [ref=e313] [cursor=pointer]:
                        - cell "2" [ref=e314]
                        - cell [ref=e315]:
                          - button "3" [ref=e316]
                        - cell "05:44:11.106" [ref=e317]
                        - cell "orders-11" [ref=e318]
                        - 'cell "{\"i\":11,\"topic\":\"orders\"}" [ref=e319]'
                        - cell "25 B" [ref=e320]
                        - cell "Committed" [ref=e321]
                      - row [ref=e324] [cursor=pointer]:
                        - cell "2" [ref=e325]
                        - cell [ref=e326]:
                          - button "2" [ref=e327]
                        - cell "05:44:11.106" [ref=e328]
                        - cell "orders-10" [ref=e329]
                        - 'cell "{\"i\":10,\"topic\":\"orders\"}" [ref=e330]'
                        - cell "25 B" [ref=e331]
                        - cell "Committed" [ref=e332]
                      - row [ref=e335] [cursor=pointer]:
                        - cell "2" [ref=e336]
                        - cell [ref=e337]:
                          - button "1" [ref=e338]
                        - cell "05:44:11.106" [ref=e339]
                        - cell "orders-8" [ref=e340]
                        - 'cell "{\"i\":8,\"topic\":\"orders\"}" [ref=e341]'
                        - cell "24 B" [ref=e342]
                        - cell "Committed" [ref=e343]
                      - row [ref=e346] [cursor=pointer]:
                        - cell "2" [ref=e347]
                        - cell [ref=e348]:
                          - button "0" [ref=e349]
                        - cell "05:44:11.106" [ref=e350]
                        - cell "orders-3" [ref=e351]
                        - 'cell "{\"i\":3,\"topic\":\"orders\"}" [ref=e352]'
                        - cell "24 B" [ref=e353]
                        - cell "Committed" [ref=e354]
                  - generic [ref=e357]: Newest first · 12 of 12
                - paragraph [ref=e359]:
                  - generic [ref=e360]: 3 of 3 partitions online
                - button "How this read works" [ref=e363]
        - generic [ref=e364]:
          - heading "Jump to a topic or group" [level=2] [ref=e365]
          - paragraph [ref=e366]: Type a name, then Enter
    - region "Notifications alt+T"
  - dialog [ref=e368]:
    - generic [ref=e369]:
      - generic [ref=e370]: From time, UTC
      - textbox "From time, UTC" [ref=e371]:
        - /placeholder: 2026-11-13 14:30
        - text: 2026-11-12 05:44
      - generic [ref=e372]:
        - button "5 min ago" [ref=e373]
        - button "1 hour ago" [ref=e374]
        - button "24 hours ago" [ref=e375]
      - paragraph [ref=e376]: The first record at or after this time, in every partition read. A UTC time like 2026-11-09 22:13, or milliseconds since the epoch.
      - button "Apply" [ref=e378]
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
  57  |   await expect(page).toHaveURL(/partition=1/);
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
> 88  |   expect(await boxes(page, ["[data-testid=toolbar]", "[data-testid=filter-line]"])).toEqual(toolbar);
      |                                                                                     ^ Error: expect(received).toEqual(expected) // deep equality
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
  104 |     expect(await sheet.getByTestId("record-value").textContent()).toContain("\n  ");
  105 |     await sheet.getByTestId("json-view").getByRole("radio", { name: "As sent" }).click();
  106 |     await expect(sheet.getByTestId("json-view").getByRole("radio", { name: "As sent" })).toHaveAttribute("aria-checked", "true");
  107 |     expect(await sheet.getByTestId("record-value").textContent()).not.toContain("\n  ");
  108 |   }
  109 |   await page.keyboard.press("Escape");
  110 |   await expect(sheet).toBeHidden();
  111 |   await expect(page).not.toHaveURL(/record=/);
  112 |   await expect(opener).toBeFocused();
  113 |   // Back closes it too.
  114 |   await opener.click();
  115 |   await expect(sheet).toBeVisible();
  116 |   const url = page.url();
  117 |   await page.goBack();
  118 |   await expect(sheet).toBeHidden();
  119 |   await expect(page).not.toHaveURL(/record=/);
  120 |   // A deep link opens the page with the record over it; closing stays on the page.
  121 |   await page.goto(url);
  122 |   await expect(page.getByTestId("record-sheet")).toBeVisible();
  123 |   await expect(page.locator("h1")).toHaveText("orders");
  124 |   await page.getByTestId("record-sheet").getByRole("button", { name: "Close" }).click();
  125 |   await expect(page.getByTestId("record-sheet")).toBeHidden();
  126 |   await expect(page).toHaveURL(/\/topics\/orders\/messages$/);
  127 |   // A record that is not there says so in the Sheet.
  128 |   await page.goto("/topics/orders/messages?record=0:999999999");
  129 |   await expect(page.getByTestId("record-missing")).toContainText("There is no record at offset 999,999,999");
  130 | });
  131 | 
  132 | test("Older asks for more records, in the address", async ({ page }) => {
  133 |   await page.goto("/topics/orders/messages?limit=5");
  134 |   await expect(rows(page)).toHaveCount(5);
  135 |   await page.getByTestId("older").click();
  136 |   await expect(page).toHaveURL(/limit=10/);
  137 |   await expect(rows(page)).toHaveCount(10);
  138 | });
  139 | 
  140 | test("the Live tail: new rows at the top, no row rewritten, the header stable, held behind a pill while hovered", async ({ page }) => {
  141 |   test.setTimeout(120_000);
  142 |   await page.goto("/topics/orders/messages");
  143 |   await expect(rows(page).first()).toBeVisible();
  144 |   await page.getByTestId("mode").getByRole("radio", { name: "Live tail" }).click();
  145 |   await expect(page).toHaveURL(/mode=tail/);
  146 |   await choose(page, page.getByTestId("speed"), "Realtime");
  147 |   await expect(page.getByTestId("conn")).toHaveAttribute("data-state", "live", { timeout: 15_000 });
  148 |   await page.mouse.move(5, 5);
  149 |   const head = await page.getByRole("table", { name: "Records, newest first" }).locator("thead").elementHandle();
  150 |   const top = rows(page).first();
  151 |   const topId = await top.getAttribute("data-row");
  152 |   const topEl = await top.elementHandle();
  153 |   const topText = await top.innerText();
  154 |   const box = await boxes(page, ["[data-testid=toolbar]", "[data-testid=filter-line]", "thead"]);
  155 |   await produce("orders", 3, "tail-a");
  156 |   await expect(rows(page).first()).not.toHaveAttribute("data-row", topId!, { timeout: 45_000 });
  157 |   expect(await head!.evaluate((el) => el.isConnected)).toBe(true);
  158 |   expect(await topEl!.evaluate((el) => el.isConnected)).toBe(true);
  159 |   expect(await topEl!.evaluate((el) => (el as HTMLElement).innerText)).toBe(topText);
  160 |   expect(await page.locator(`tr[data-row="${topId}"]`).elementHandle().then((h) => h!.evaluate((el, old) => el === old, topEl))).toBe(true);
  161 |   expect(await boxes(page, ["[data-testid=toolbar]", "[data-testid=filter-line]", "thead"])).toEqual(box);
  162 |   // Hovering the table holds what comes next behind the pill; the rows on screen stay where they are.
  163 |   await page.getByTestId("records").locator("tbody tr").nth(2).hover();
  164 |   const heldTop = await rows(page).first().getAttribute("data-row");
  165 |   await produce("orders", 2, "tail-b");
  166 |   await expect(page.getByTestId("held-pill")).toBeVisible({ timeout: 45_000 });
  167 |   await expect(page.getByTestId("held-pill")).toContainText(/\d+ new · Show/);
  168 |   expect(await rows(page).first().getAttribute("data-row")).toBe(heldTop);
  169 |   await page.getByTestId("held-pill").click();
  170 |   await expect(page.getByTestId("held-pill")).toHaveCount(0);
  171 |   await expect(rows(page).first()).not.toHaveAttribute("data-row", heldTop!);
  172 |   // Back to Snapshot: the stream closes.
  173 |   await page.getByTestId("mode").getByRole("radio", { name: "Snapshot" }).click();
  174 |   await expect(page).not.toHaveURL(/mode=tail/);
  175 |   await expect(page.getByTestId("conn")).toHaveAttribute("data-state", "snapshot");
  176 | });
  177 | 
  178 | test("the Messages page does not scroll sideways at 1024, 1280, 1440 and 1920", async ({ page }) => {
  179 |   await page.goto("/topics/orders/messages");
  180 |   await expect(rows(page).first()).toBeVisible();
  181 |   for (const width of [1024, 1280, 1440, 1920]) {
  182 |     await page.setViewportSize({ width, height: 900 });
  183 |     expect(await overflowX(page), `${width}`).toBeLessThanOrEqual(0);
  184 |   }
  185 | });
  186 | 
```