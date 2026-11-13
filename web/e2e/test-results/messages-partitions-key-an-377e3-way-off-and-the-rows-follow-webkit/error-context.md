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
- generic [ref=e1]:
  - generic [ref=e2]:
    - generic [ref=e3]:
      - generic [ref=e6]:
        - link "doze-kafka, Home" [ref=e8]:
          - /url: /
          - generic [ref=e12]: doze-kafka
        - list [ref=e16]:
          - listitem [ref=e17]:
            - link "Home" [ref=e18]:
              - /url: /
          - listitem [ref=e23]:
            - link "Live" [ref=e24]:
              - /url: /live
          - listitem [ref=e28]:
            - link "Topics" [ref=e29]:
              - /url: /topics
            - generic: "3"
          - listitem [ref=e33]:
            - link "Groups" [ref=e34]:
              - /url: /groups
            - generic: "1"
          - listitem [ref=e41]:
            - link "Access" [ref=e42]:
              - /url: /access
          - listitem [ref=e46]:
            - link "Lab" [ref=e47]:
              - /url: /lab
          - listitem [ref=e51]:
            - link "Settings" [ref=e52]:
              - /url: /settings
        - list [ref=e58]:
          - listitem [ref=e59]:
            - link "Classic console" [ref=e60]:
              - /url: /classic/
        - button "Toggle Sidebar" [ref=e66]
      - main [ref=e67]:
        - generic [ref=e68]:
          - button "Toggle Sidebar" [ref=e69]
          - button "Jump to a topic or group ⌘K" [ref=e71]:
            - generic [ref=e75]: Jump to a topic or group
            - generic [ref=e76]: ⌘K
          - generic [ref=e77]:
            - button "127.0.0.1:39092" [ref=e78]
            - generic [ref=e83]: Kafka 4 profile
            - link "Open" [ref=e84]:
              - /url: /access
        - main [active] [ref=e86]:
          - generic [ref=e87]:
            - generic [ref=e88]:
              - generic [ref=e89]:
                - navigation "Breadcrumb" [ref=e90]:
                  - list [ref=e91]:
                    - listitem [ref=e92]:
                      - link "Topics" [ref=e93]:
                        - /url: /topics
                - generic [ref=e94]:
                  - generic [ref=e95]: Updated 05:45:35 UTC
                  - combobox "Refresh every" [ref=e96]:
                    - generic: "Refresh: Off"
                  - button "Refresh" [ref=e97]
              - generic [ref=e98]:
                - generic [ref=e99]:
                  - heading "orders" [level=1] [ref=e100]
                  - generic [ref=e101]: delete
                - generic [ref=e103]:
                  - link "Partitions" [ref=e104]:
                    - /url: /topics/orders/partitions
                  - button "Produce" [ref=e105]
                  - button "Delete topic" [ref=e106]
            - generic [ref=e107]:
              - generic [ref=e108]:
                - term [ref=e109]: Partitions
                - definition [ref=e111]: "3"
                - generic "all online" [ref=e112]
              - generic [ref=e113]:
                - term [ref=e114]: Retention
                - definition [ref=e116]: for ever
                - generic "delete" [ref=e117]
              - generic [ref=e118]:
                - term [ref=e119]: Records
                - definition [ref=e121]: "17"
              - generic [ref=e122]:
                - term [ref=e123]: Size on disk
                - definition [ref=e125]: 1.3 KiB
              - generic [ref=e126]:
                - term [ref=e127]: Last write
                - definition [ref=e129]: 05:44:36
                - generic "UTC" [ref=e130]
            - generic [ref=e131]: No open transactions.
            - navigation "Topic" [ref=e136]:
              - list [ref=e137]:
                - listitem [ref=e138]:
                  - link "Messages" [ref=e139]:
                    - /url: /topics/orders/messages
                - listitem [ref=e140]:
                  - link "Log" [ref=e141]:
                    - /url: /topics/orders/log
                - listitem [ref=e142]:
                  - link "Config" [ref=e143]:
                    - /url: /topics/orders/config
                - listitem [ref=e144]:
                  - link "Consumers 1" [ref=e145]:
                    - /url: /topics/orders/consumers
                    - text: Consumers
                    - generic [ref=e146]: "1"
                - listitem [ref=e147]:
                  - link "Producers 0" [ref=e148]:
                    - /url: /topics/orders/producers
                    - text: Producers
                    - generic [ref=e149]: "0"
            - generic [ref=e151]:
              - generic [ref=e152]:
                - combobox "Partitions" [ref=e155]:
                  - generic [ref=e156]: All partitions
                - search [ref=e157]:
                  - textbox "Find a key, or go to an offset" [ref=e158]:
                    - /placeholder: Key, or offset
                - button "From time" [ref=e159]
                - generic [ref=e161]:
                  - generic [ref=e162]: Reading as
                  - radiogroup "Reading as" [ref=e163]:
                    - radio "read_uncommitted" [checked] [ref=e164]
                    - radio "read_committed" [ref=e165]
              - generic [ref=e166]:
                - generic [ref=e167]:
                  - generic [ref=e168]: "No filter: the newest records of every partition."
                  - generic [ref=e170]:
                    - radiogroup "Snapshot or Live tail" [ref=e172]:
                      - radio "Snapshot" [checked] [ref=e173]
                      - radio "Live tail" [ref=e174]
                    - combobox "Tail speed" [disabled] [ref=e175]:
                      - generic: Relaxed
                    - generic [ref=e176]: Snapshot
                - generic [ref=e179]:
                  - table "Records, newest first" [ref=e181]:
                    - rowgroup [ref=e182]:
                      - row [ref=e183]:
                        - columnheader "Partition" [ref=e184]:
                          - button "Partition" [ref=e185]
                        - columnheader "Offset" [ref=e188]:
                          - button "Offset" [ref=e189]
                        - columnheader "Timestamp" [ref=e192]:
                          - button "Timestamp" [ref=e193]
                        - columnheader "Key" [ref=e196]:
                          - button "Key" [ref=e197]
                        - columnheader "Value" [ref=e200]:
                          - button "Value" [ref=e201]
                        - columnheader "Value size" [ref=e204]:
                          - button "Value size" [ref=e205]
                        - columnheader "For read_committed" [ref=e208]:
                          - button "For read_committed" [ref=e209]
                    - rowgroup [ref=e212]:
                      - row [ref=e213] [cursor=pointer]:
                        - cell "1" [ref=e214]
                        - cell [ref=e215]:
                          - button "4" [ref=e216]
                        - cell "05:44:36.691" [ref=e217]
                        - cell "tail-b-1794548676691-0" [ref=e218]
                        - 'cell "{\"tag\":\"tail-b\",\"i\":0,\"at\":1794548676691}" [ref=e219]'
                        - cell "41 B" [ref=e220]
                        - cell "Committed" [ref=e221]
                      - row [ref=e224] [cursor=pointer]:
                        - cell "2" [ref=e225]
                        - cell [ref=e226]:
                          - button "6" [ref=e227]
                        - cell "05:44:36.691" [ref=e228]
                        - cell "tail-b-1794548676691-1" [ref=e229]
                        - 'cell "{\"tag\":\"tail-b\",\"i\":1,\"at\":1794548676691}" [ref=e230]'
                        - cell "41 B" [ref=e231]
                        - cell "Committed" [ref=e232]
                      - row [ref=e235] [cursor=pointer]:
                        - cell "2" [ref=e236]
                        - cell [ref=e237]:
                          - button "5" [ref=e238]
                        - cell "05:44:35.831" [ref=e239]
                        - cell "tail-a-1794548675821-1" [ref=e240]
                        - 'cell "{\"tag\":\"tail-a\",\"i\":1,\"at\":1794548675821}" [ref=e241]'
                        - cell "41 B" [ref=e242]
                        - cell "Committed" [ref=e243]
                      - row [ref=e246] [cursor=pointer]:
                        - cell "2" [ref=e247]
                        - cell [ref=e248]:
                          - button "4" [ref=e249]
                        - cell "05:44:35.831" [ref=e250]
                        - cell "tail-a-1794548675821-0" [ref=e251]
                        - 'cell "{\"tag\":\"tail-a\",\"i\":0,\"at\":1794548675821}" [ref=e252]'
                        - cell "41 B" [ref=e253]
                        - cell "Committed" [ref=e254]
                      - row [ref=e257] [cursor=pointer]:
                        - cell "1" [ref=e258]
                        - cell [ref=e259]:
                          - button "3" [ref=e260]
                        - cell "05:44:35.830" [ref=e261]
                        - cell "tail-a-1794548675821-2" [ref=e262]
                        - 'cell "{\"tag\":\"tail-a\",\"i\":2,\"at\":1794548675821}" [ref=e263]'
                        - cell "41 B" [ref=e264]
                        - cell "Committed" [ref=e265]
                      - row [ref=e268] [cursor=pointer]:
                        - cell "0" [ref=e269]
                        - cell [ref=e270]:
                          - button "4" [ref=e271]
                        - cell "05:44:11.106" [ref=e272]
                        - cell "orders-9" [ref=e273]
                        - 'cell "{\"i\":9,\"topic\":\"orders\"}" [ref=e274]'
                        - cell "24 B" [ref=e275]
                        - cell "Committed" [ref=e276]
                      - row [ref=e279] [cursor=pointer]:
                        - cell "0" [ref=e280]
                        - cell [ref=e281]:
                          - button "3" [ref=e282]
                        - cell "05:44:11.106" [ref=e283]
                        - cell "orders-5" [ref=e284]
                        - 'cell "{\"i\":5,\"topic\":\"orders\"}" [ref=e285]'
                        - cell "24 B" [ref=e286]
                        - cell "Committed" [ref=e287]
                      - row [ref=e290] [cursor=pointer]:
                        - cell "0" [ref=e291]
                        - cell [ref=e292]:
                          - button "2" [ref=e293]
                        - cell "05:44:11.106" [ref=e294]
                        - cell "orders-2" [ref=e295]
                        - 'cell "{\"i\":2,\"topic\":\"orders\"}" [ref=e296]'
                        - cell "24 B" [ref=e297]
                        - cell "Committed" [ref=e298]
                      - row [ref=e301] [cursor=pointer]:
                        - cell "0" [ref=e302]
                        - cell [ref=e303]:
                          - button "1" [ref=e304]
                        - cell "05:44:11.106" [ref=e305]
                        - cell "orders-1" [ref=e306]
                        - 'cell "{\"i\":1,\"topic\":\"orders\"}" [ref=e307]'
                        - cell "24 B" [ref=e308]
                        - cell "Committed" [ref=e309]
                      - row [ref=e312] [cursor=pointer]:
                        - cell "0" [ref=e313]
                        - cell [ref=e314]:
                          - button "0" [ref=e315]
                        - cell "05:44:11.106" [ref=e316]
                        - cell "orders-0" [ref=e317]
                        - 'cell "{\"i\":0,\"topic\":\"orders\"}" [ref=e318]'
                        - cell "24 B" [ref=e319]
                        - cell "Committed" [ref=e320]
                      - row [ref=e323] [cursor=pointer]:
                        - cell "1" [ref=e324]
                        - cell [ref=e325]:
                          - button "2" [ref=e326]
                        - cell "05:44:11.106" [ref=e327]
                        - cell "orders-7" [ref=e328]
                        - 'cell "{\"i\":7,\"topic\":\"orders\"}" [ref=e329]'
                        - cell "24 B" [ref=e330]
                        - cell "Committed" [ref=e331]
                      - row [ref=e334] [cursor=pointer]:
                        - cell "1" [ref=e335]
                        - cell [ref=e336]:
                          - button "1" [ref=e337]
                        - cell "05:44:11.106" [ref=e338]
                        - cell "orders-6" [ref=e339]
                        - 'cell "{\"i\":6,\"topic\":\"orders\"}" [ref=e340]'
                        - cell "24 B" [ref=e341]
                        - cell "Committed" [ref=e342]
                      - row [ref=e345] [cursor=pointer]:
                        - cell "1" [ref=e346]
                        - cell [ref=e347]:
                          - button "0" [ref=e348]
                        - cell "05:44:11.106" [ref=e349]
                        - cell "orders-4" [ref=e350]
                        - 'cell "{\"i\":4,\"topic\":\"orders\"}" [ref=e351]'
                        - cell "24 B" [ref=e352]
                        - cell "Committed" [ref=e353]
                      - row [ref=e356] [cursor=pointer]:
                        - cell "2" [ref=e357]
                        - cell [ref=e358]:
                          - button "3" [ref=e359]
                        - cell "05:44:11.106" [ref=e360]
                        - cell "orders-11" [ref=e361]
                        - 'cell "{\"i\":11,\"topic\":\"orders\"}" [ref=e362]'
                        - cell "25 B" [ref=e363]
                        - cell "Committed" [ref=e364]
                      - row [ref=e367] [cursor=pointer]:
                        - cell "2" [ref=e368]
                        - cell [ref=e369]:
                          - button "2" [ref=e370]
                        - cell "05:44:11.106" [ref=e371]
                        - cell "orders-10" [ref=e372]
                        - 'cell "{\"i\":10,\"topic\":\"orders\"}" [ref=e373]'
                        - cell "25 B" [ref=e374]
                        - cell "Committed" [ref=e375]
                      - row [ref=e378] [cursor=pointer]:
                        - cell "2" [ref=e379]
                        - cell [ref=e380]:
                          - button "1" [ref=e381]
                        - cell "05:44:11.106" [ref=e382]
                        - cell "orders-8" [ref=e383]
                        - 'cell "{\"i\":8,\"topic\":\"orders\"}" [ref=e384]'
                        - cell "24 B" [ref=e385]
                        - cell "Committed" [ref=e386]
                      - row [ref=e389] [cursor=pointer]:
                        - cell "2" [ref=e390]
                        - cell [ref=e391]:
                          - button "0" [ref=e392]
                        - cell "05:44:11.106" [ref=e393]
                        - cell "orders-3" [ref=e394]
                        - 'cell "{\"i\":3,\"topic\":\"orders\"}" [ref=e395]'
                        - cell "24 B" [ref=e396]
                        - cell "Committed" [ref=e397]
                  - generic [ref=e400]: Newest first · 17 of 17
                - paragraph [ref=e402]:
                  - generic [ref=e403]: 3 of 3 partitions online
                - button "How this read works" [ref=e406]
        - generic [ref=e407]:
          - heading "Jump to a topic or group" [level=2] [ref=e408]
          - paragraph [ref=e409]: Type a name, then Enter
    - region "Notifications alt+T"
  - dialog [ref=e411]:
    - generic [ref=e412]:
      - generic [ref=e413]: From time, UTC
      - textbox "From time, UTC" [ref=e414]:
        - /placeholder: 2026-11-13 14:30
        - text: 2026-11-12 05:45
      - generic [ref=e415]:
        - button "5 min ago" [ref=e416]
        - button "1 hour ago" [ref=e417]
        - button "24 hours ago" [ref=e418]
      - paragraph [ref=e419]: The first record at or after this time, in every partition read. A UTC time like 2026-11-09 22:13, or milliseconds since the epoch.
      - button "Apply" [ref=e421]
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