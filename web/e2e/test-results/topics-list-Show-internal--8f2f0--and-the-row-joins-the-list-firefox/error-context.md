# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: topics-list.spec.ts >> Show internal: the switch works with the mouse and the keyboard, is in the address, and the row joins the list
- Location: tests/topics-list.spec.ts:20:1

# Error details

```
Error: click: Error: strict mode violation: getByText('Show internal', { exact: true }) resolved to 2 elements:
    1) <label data-slot="label" for="show-internal" class="flex items-center gap-2 select-none group-data-[disabled=true]:pointer-events-none group-data-[disabled=true]:opacity-50 peer-disabled:cursor-not-allowed peer-disabled:opacity-50 cursor-pointer text-[13.5px] font-normal">Show internal</label> aka locator('label')
    2) <b class="font-semibold text-foreground">Show internal</b> aka getByTestId('internal-note').getByText('Show internal')

Call log:
  - waiting for getByText('Show internal', { exact: true })

```

# Page snapshot

```yaml
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
            - navigation "Breadcrumb"
            - generic [ref=e93]:
              - generic [ref=e94]: Updated 05:25:58 UTC
              - combobox "Refresh every" [ref=e95]:
                - generic: "Refresh: Off"
              - button "Refresh" [ref=e96]
          - generic [ref=e97]:
            - generic [ref=e98]:
              - heading "Topics" [level=1] [ref=e99]
              - paragraph [ref=e100]: "Where records live. Every topic is a log: open one to read it, produce to it, or see what the broker did to it."
            - button "Create topic" [ref=e102]
        - generic [ref=e103]:
          - generic [ref=e104]:
            - generic [ref=e105]:
              - term [ref=e106]: Topics
              - definition [ref=e108]: "3"
            - generic [ref=e109]:
              - term [ref=e110]: Partitions
              - definition [ref=e112]: "6"
            - generic [ref=e113]:
              - term [ref=e114]: Newest write
              - definition [ref=e116]: 05:25:34
              - generic "UTC" [ref=e117]
            - generic [ref=e118]:
              - term [ref=e119]: Written to
              - definition [ref=e121]: orders
          - search [ref=e122]:
            - textbox "Filter topics by name" [ref=e124]
            - generic [ref=e125]:
              - switch "Show internal" [checked] [active] [ref=e126]
              - generic [ref=e127] [cursor=pointer]: Show internal
          - table "Topics" [ref=e129]:
            - rowgroup [ref=e136]:
              - row [ref=e137]:
                - columnheader "Name" [ref=e138]:
                  - button "Name" [ref=e139]
                - columnheader "Partitions" [ref=e142]:
                  - button "Partitions" [ref=e143]
                - columnheader "Cleanup policy" [ref=e146]:
                  - button "Cleanup policy" [ref=e147]
                - columnheader "Size on disk" [ref=e150]:
                  - button "Size on disk" [ref=e151]
                - columnheader "Last write" [ref=e154]:
                  - button "Last write" [ref=e155]
            - rowgroup [ref=e158]:
              - row [ref=e159]:
                - cell [ref=e160]:
                  - link "inventory.levels" [ref=e163] [cursor=pointer]:
                    - /url: /topics/inventory.levels/messages
                - cell "1" [ref=e164]
                - cell "compact" [ref=e165]
                - cell "773 B" [ref=e167]
                - cell "05:25:05" [ref=e168]
              - row [ref=e169]:
                - cell "orders read by fulfilment" [ref=e170]:
                  - generic [ref=e171]:
                    - link "orders" [ref=e173] [cursor=pointer]:
                      - /url: /topics/orders/messages
                    - generic "read by fulfilment" [ref=e174]
                - cell "3" [ref=e175]
                - cell "delete" [ref=e176]
                - cell "1.4 KiB" [ref=e178]
                - cell "05:25:34" [ref=e179]
              - row [ref=e180]:
                - cell [ref=e181]:
                  - link "payments.settled" [ref=e184] [cursor=pointer]:
                    - /url: /topics/payments.settled/messages
                - cell "2" [ref=e185]
                - cell "delete" [ref=e186]
                - cell "834 B" [ref=e188]
                - cell "05:25:05" [ref=e189]
              - row [ref=e190]:
                - cell "__consumer_offsets internal read only · the group coordinator's name for where commits live" [ref=e191]:
                  - generic [ref=e192]:
                    - generic [ref=e193]:
                      - generic [ref=e194]: __consumer_offsets
                      - generic [ref=e195]: internal
                    - generic "read only · the group coordinator's name for where commits live" [ref=e196]
                - cell "50" [ref=e197]
                - cell "compact" [ref=e198]
                - cell "not a log" [ref=e200]
                - cell "none" [ref=e201]
          - paragraph [ref=e202]: 3 of 3 topics
          - generic [ref=e208]:
            - paragraph [ref=e209]:
              - text: The internal topic is shown. Turn on Show internal and
              - code [ref=e210]: __consumer_offsets
              - text: joins the list.
            - paragraph [ref=e211]:
              - text: Why it is read only.
              - code [ref=e212]: __consumer_offsets
              - text: "is Kafka's own name for the place the group coordinator keeps each group's committed offsets, spread over 50 partitions. This broker keeps commits in its own store and answers Kafka's offset calls from it, so there is no log here to read, produce to, delete from, or change. The name is shown because Kafka clients and tools know it; the 50 partitions decide which coordinator slot a group lands in. Look at a group's commits on its Lag tab instead:"
              - link "open Groups" [ref=e213] [cursor=pointer]:
                - /url: /groups
              - text: .
      - generic [ref=e214]:
        - heading "Jump to a topic or group" [level=2] [ref=e215]
        - paragraph [ref=e216]: Type a name, then Enter
  - region "Notifications alt+T"
```

# Test source

```ts
  1  | // The Topics list (slice R1a): the filter and the Show internal switch are in the address and work with the mouse and the
  2  | // keyboard; the empty filter result says so; a name opens the topic; nothing scrolls sideways.
  3  | import { expect, overflowX, test } from "./fixtures";
  4  | 
  5  | test("the list: one h1, the figures as stat tiles, a row per topic, and a name opens the topic", async ({ page }) => {
  6  |   await page.goto("/topics");
  7  |   await expect(page.locator("h1")).toHaveText("Topics");
  8  |   const strip = page.locator("[data-slot=stat-strip]");
  9  |   for (const label of ["Topics", "Partitions", "Newest write", "Written to"]) await expect(strip).toContainText(label);
  10 |   const table = page.getByRole("table", { name: "Topics" });
  11 |   await expect(table.locator("#r-orders")).toBeVisible();
  12 |   await expect(table.locator("#r-__consumer_offsets")).toHaveCount(0);
  13 |   await table.locator("#r-orders").getByTestId("topic-link").click();
  14 |   await expect(page).toHaveURL(/\/topics\/orders\/messages$/);
  15 |   await expect(page.locator("h1")).toHaveText("orders");
  16 |   await page.goBack();
  17 |   await expect(page.locator("h1")).toHaveText("Topics");
  18 | });
  19 | 
  20 | test("Show internal: the switch works with the mouse and the keyboard, is in the address, and the row joins the list", async ({ page }) => {
  21 |   await page.goto("/topics");
  22 |   const sw = page.getByTestId("show-internal");
  23 |   await expect(sw).toHaveAttribute("aria-checked", "false");
  24 |   await sw.click();
  25 |   await expect(sw).toHaveAttribute("aria-checked", "true");
  26 |   await expect(page).toHaveURL(/internal=true/);
  27 |   const table = page.getByRole("table", { name: "Topics" });
  28 |   await expect(table.locator("#r-__consumer_offsets")).toBeVisible();
  29 |   await expect(table.locator("#r-__consumer_offsets")).toContainText("internal");
  30 |   await expect(page.getByTestId("internal-note")).toContainText("The internal topic is shown.");
  31 |   // The label toggles it too.
> 32 |   await page.getByText("Show internal", { exact: true }).click();
     |                                                          ^ Error: click: Error: strict mode violation: getByText('Show internal', { exact: true }) resolved to 2 elements:
  33 |   await expect(sw).toHaveAttribute("aria-checked", "false");
  34 |   await expect(table.locator("#r-__consumer_offsets")).toHaveCount(0);
  35 |   // The keyboard.
  36 |   await sw.focus();
  37 |   await page.keyboard.press("Space");
  38 |   await expect(sw).toHaveAttribute("aria-checked", "true");
  39 |   await expect(page).toHaveURL(/internal=true/);
  40 |   // A reload keeps it.
  41 |   await page.reload();
  42 |   await expect(page.getByTestId("show-internal")).toHaveAttribute("aria-checked", "true");
  43 |   await expect(page.getByRole("table", { name: "Topics" }).locator("#r-__consumer_offsets")).toBeVisible();
  44 | });
  45 | 
  46 | test("the filter narrows the list as you type, is in the address, says when nothing matches, and Clear brings all back", async ({ page }) => {
  47 |   await page.goto("/topics");
  48 |   const table = page.getByRole("table", { name: "Topics" });
  49 |   await expect(table.locator("#r-orders")).toBeVisible();
  50 |   const filter = page.getByTestId("topics-filter");
  51 |   await filter.click();
  52 |   await page.keyboard.type("pay");
  53 |   await expect(page).toHaveURL(/q=pay/);
  54 |   await expect(table.locator("#r-orders")).toHaveCount(0);
  55 |   await expect(table.locator("#r-payments\\.settled")).toBeVisible();
  56 |   await expect(page.getByTestId("topics-count")).toHaveText(/^\d+ of \d+ topics$/);
  57 |   await filter.fill("zzz-nothing-is-called-this");
  58 |   await expect(page.getByTestId("topics-page")).toHaveAttribute("data-state", "filtered");
  59 |   await expect(page.getByText("No topic name contains zzz-nothing-is-called-this")).toBeVisible();
  60 |   await page.getByTestId("clear-filter").click();
  61 |   await expect(page).not.toHaveURL(/q=/);
  62 |   await expect(page.getByRole("table", { name: "Topics" }).locator("#r-orders")).toBeVisible();
  63 |   await expect(filter).toHaveValue("");
  64 |   // Esc in the field clears it.
  65 |   await filter.fill("ord");
  66 |   await expect(page).toHaveURL(/q=ord/);
  67 |   await filter.press("Escape");
  68 |   await expect(filter).toHaveValue("");
  69 |   await expect(page).not.toHaveURL(/q=/);
  70 | });
  71 | 
  72 | test("the Topics list does not scroll sideways at 1024, 1280, 1440 and 1920, and a refresh moves nothing", async ({ page }) => {
  73 |   await page.goto("/topics");
  74 |   await expect(page.getByRole("table", { name: "Topics" })).toBeVisible();
  75 |   for (const width of [1024, 1280, 1440, 1920]) {
  76 |     await page.setViewportSize({ width, height: 900 });
  77 |     expect(await overflowX(page), `${width}`).toBeLessThanOrEqual(0);
  78 |   }
  79 |   await page.setViewportSize({ width: 1440, height: 900 });
  80 |   const before = await page.getByRole("table", { name: "Topics" }).boundingBox();
  81 |   const row = await page.locator("#r-orders").elementHandle();
  82 |   await page.getByTestId("refresh-now").click();
  83 |   await expect(page.getByTestId("refresh-now").locator("svg.animate-spin")).toHaveCount(0);
  84 |   const after = await page.getByRole("table", { name: "Topics" }).boundingBox();
  85 |   expect(Math.round(after!.y)).toBe(Math.round(before!.y));
  86 |   expect(await row!.evaluate((el) => el.isConnected)).toBe(true);
  87 | });
  88 | 
```