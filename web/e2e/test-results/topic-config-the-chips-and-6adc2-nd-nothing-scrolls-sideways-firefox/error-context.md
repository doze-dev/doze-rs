# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: topic-config.spec.ts >> the chips and the filter are in the address, Back restores them, and nothing scrolls sideways
- Location: tests/topic-config.spec.ts:9:1

# Error details

```
Error: expect(page).toHaveURL(expected) failed

Expected pattern: /show=changed/
Received string:  "http://127.0.0.1:39094/topics/r1b-cfg-firefox-mwe86376-1/config?show=segments"
Timeout: 8000ms

Call log:
  - Expect "toHaveURL" with timeout 8000ms
    18 × locator resolved to <html lang="en">…</html>
       - unexpected value "http://127.0.0.1:39094/topics/r1b-cfg-firefox-mwe86376-1/config?show=segments"

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
    - text: "4"
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
    - heading "r1b-cfg-firefox-mwe86376-1" [level=1]
    - button "Add partitions"
    - button "Delete records"
    - button "Produce"
    - button "Delete topic"
    - navigation "Topic":
      - link "Messages":
        - /url: /topics/r1b-cfg-firefox-mwe86376-1
      - link "Log":
        - /url: /topics/r1b-cfg-firefox-mwe86376-1/log
      - link "Config":
        - /url: /topics/r1b-cfg-firefox-mwe86376-1/config
      - link "Consumers":
        - /url: /topics/r1b-cfg-firefox-mwe86376-1/consumers
      - link "Producers":
        - /url: /topics/r1b-cfg-firefox-mwe86376-1/producers
    - paragraph: 33 configs. 2 are set on this topic, 1 comes from a broker flag, 30 are Kafka's defaults. The cleaner reads them at every look, so a change counts from the next one, within 15 s.
    - search:
      - textbox "Filter configs":
        - /placeholder: Filter by name or by what it does
      - radiogroup "Show":
        - radio "All 33"
        - radio "Changed 3"
        - radio "Compaction"
        - radio "Segments" [checked]
    - paragraph: "Where a value comes from, first match wins: set on this topic (Dynamic topic), then set for this broker or all brokers (Dynamic broker), then a start flag that differs from Kafka's default (Static flag), then Kafka's default. Open Where from on any changed row to see every place."
    - region "Changed from Kafka's default 1":
      - heading "Changed from Kafka's default 1" [level=2]
      - paragraph: Set here or by a flag. These are the ones worth reading first.
      - list:
        - listitem:
          - text: segment.bytes
          - paragraph: This configuration controls the segment file size for the log.
          - paragraph: A change counts from the next append, by the open partition's writer; existing segments keep their size.
          - button "Where from"
          - button "Kafka's text"
          - text: 134217728 128 MiB Static flag
          - button "Edit segment.bytes": Edit
    - region "Segments and index 6":
      - heading "Segments and index 6" [level=2]
      - list:
        - listitem:
          - text: segment.jitter.ms
          - paragraph: The maximum random jitter subtracted from the scheduled segment roll time to avoid thundering herds of segment rolling
          - paragraph: This broker keeps the value and shows it; it does nothing with it.
          - text: 0 Default
          - button "Edit segment.jitter.ms": Edit
        - listitem:
          - text: file.delete.delay.ms
          - paragraph: The time to wait before deleting a file from the filesystem
          - paragraph: This broker keeps the value and shows it; it does nothing with it.
          - text: 60000 1 min Default
          - button "Edit file.delete.delay.ms": Edit
        - listitem:
          - text: preallocate
          - paragraph: True if we should preallocate the file on disk when creating a new log segment.
          - paragraph: This broker keeps the value and shows it; it does nothing with it.
          - text: false Default
          - button "Edit preallocate": Edit
        - listitem:
          - text: index.interval.bytes
          - paragraph: This setting controls how frequently Kafka adds entries to its offset index and, conditionally, to its time index.
          - paragraph: A change counts from the next append, by the open partition's writer; existing segments keep their size.
          - button "Kafka's text"
          - text: 4096 4 KiB Default
          - button "Edit index.interval.bytes": Edit
        - listitem:
          - text: segment.ms
          - paragraph: This configuration controls the period of time after which Kafka will force the log to roll even if the segment file isn't full to ensure that retention can delete or compact old data.
          - paragraph: A change counts from the next append, by the open partition's writer; existing segments keep their size.
          - text: 604800000 7 d Default
          - button "Edit segment.ms": Edit
        - listitem:
          - text: segment.index.bytes
          - paragraph: This configuration controls the size of the index that maps offsets to file positions.
          - paragraph: This broker keeps the value and shows it; it does nothing with it.
          - button "Kafka's text"
          - text: 10485760 10 MiB Default
          - button "Edit segment.index.bytes": Edit
  - heading "Jump to a topic or group" [level=2]
  - paragraph: Type a name, then Enter
- region "Notifications alt+T"
```

# Test source

```ts
  1   | // Slice R1b: a topic's Config tab: the filter and the chips in the address, the panels that open in place, and the
  2   | // dialogs that change a config (Check, Save, a list, the two lags, a refused value) and reset it.
  3   | import { expect, overflowX, test } from "./fixtures";
  4   | import { call, fresh, get, topic } from "./r1b-helpers";
  5   | 
  6   | const configUrl = (t: string, q = "") => `/topics/${encodeURIComponent(t)}/config${q}`;
  7   | const row = (page: import("@playwright/test").Page, key: string) => page.locator(`[data-testid=config-row][data-key="${key}"]`).first();
  8   | 
  9   | test("the chips and the filter are in the address, Back restores them, and nothing scrolls sideways", async ({ page, request, baseURL }, info) => {
  10  |   const name = fresh(info, "cfg");
  11  |   await topic(request, baseURL!, name, 1, "short");
  12  |   await page.goto(configUrl(name));
  13  |   await expect(page.getByTestId("config-sentence")).toContainText("configs.");
  14  |   await expect(page.getByTestId("section-changed")).toContainText("retention.ms");
  15  |   const chips = page.getByTestId("config-chips");
  16  |   await chips.getByRole("radio", { name: /Changed/ }).click();
  17  |   await expect(page).toHaveURL(/show=changed/);
  18  |   await expect(page.getByTestId("section-segments")).toHaveCount(0);
  19  |   await chips.getByRole("radio", { name: "Segments" }).click();
  20  |   await expect(page).toHaveURL(/show=segments/);
  21  |   await expect(row(page, "segment.bytes")).toBeVisible();
  22  |   await expect(page.getByTestId("section-cleanup")).toHaveCount(0);
  23  |   await page.goBack();
> 24  |   await expect(page).toHaveURL(/show=changed/);
      |                      ^ Error: expect(page).toHaveURL(expected) failed
  25  |   await expect(chips.getByRole("radio", { name: /Changed/ })).toHaveAttribute("aria-checked", "true");
  26  |   await chips.getByRole("radio", { name: /All/ }).click();
  27  |   await expect(page).not.toHaveURL(/show=/);
  28  |   await page.getByTestId("config-filter").fill("dirty");
  29  |   await expect(page).toHaveURL(/q=dirty/);
  30  |   await expect(page.getByTestId("config-row")).toHaveCount(1);
  31  |   await expect(row(page, "min.cleanable.dirty.ratio")).toBeVisible();
  32  |   await page.getByRole("button", { name: "Clear the filter" }).click();
  33  |   await expect(page).not.toHaveURL(/q=/);
  34  |   for (const w of [1024, 1280, 1440, 1920]) {
  35  |     await page.setViewportSize({ width: w, height: 900 });
  36  |     expect(await overflowX(page), `overflow at ${w}`).toBe(0);
  37  |   }
  38  | });
  39  | 
  40  | test("Where from and Kafka's text open in place: the address and the scroll stay", async ({ page, request, baseURL }, info) => {
  41  |   const name = fresh(info, "cfgp");
  42  |   await topic(request, baseURL!, name, 1, "short");
  43  |   await page.goto(configUrl(name));
  44  |   const r = row(page, "segment.ms");
  45  |   await r.scrollIntoViewIfNeeded();
  46  |   const url = page.url();
  47  |   const y = await page.evaluate(() => window.scrollY);
  48  |   await r.getByRole("button", { name: "Kafka's text" }).click();
  49  |   await expect(r.getByTestId("kafka-text")).toBeVisible();
  50  |   await expect(r.getByTestId("kafka-text")).toContainText("Type");
  51  |   expect(page.url()).toBe(url);
  52  |   expect(Math.abs((await page.evaluate(() => window.scrollY)) - y)).toBeLessThan(2);
  53  |   await r.getByRole("button", { name: "Kafka's text" }).click();
  54  |   await expect(r.getByTestId("kafka-text")).toBeHidden();
  55  |   const ret = row(page, "retention.ms");
  56  |   await ret.getByRole("button", { name: "Where from" }).click();
  57  |   await expect(ret.getByTestId("where-from")).toContainText("Dynamic topic");
  58  |   await expect(ret.getByTestId("where-from")).toContainText("takes over");
  59  |   expect(page.url()).toBe(url);
  60  | });
  61  | 
  62  | test("edit a value: Check asks Kafka and changes nothing, Save changes it, and the tab shows it without a reload", async ({ page, request, baseURL }, info) => {
  63  |   const name = fresh(info, "cfge");
  64  |   await topic(request, baseURL!, name, 1, "short");
  65  |   await page.goto(configUrl(name));
  66  |   const r = row(page, "retention.ms");
  67  |   await r.getByTestId("row-menu").click();
  68  |   await page.getByRole("menuitem", { name: "Edit" }).click();
  69  |   await expect(page).toHaveURL(/edit=retention\.ms/);
  70  |   const d = page.getByTestId("dialog-config-edit");
  71  |   await expect(d.getByTestId("config-value")).toHaveValue("3600000");
  72  |   await d.getByTestId("config-value").fill("7200000");
  73  |   await expect(d.getByTestId("the-call")).toContainText("7200000");
  74  |   await d.getByTestId("config-check").click();
  75  |   await expect(d.getByTestId("config-checked")).toContainText("Kafka would take this");
  76  |   const still = await get(request, `/api/v1/topics/${name}/config`);
  77  |   expect(still.body.rows.find((x: { name: string }) => x.name === "retention.ms").value).toBe("3600000");
  78  |   await d.getByTestId("config-save").click();
  79  |   await expect(d.getByTestId("config-saved")).toBeVisible();
  80  |   await expect(d.getByTestId("saved-now")).toContainText("7200000");
  81  |   await d.getByTestId("saved-done").click();
  82  |   await expect(d).toBeHidden();
  83  |   await expect(page).not.toHaveURL(/edit=/);
  84  |   await expect(r.getByTestId("config-value")).toHaveText("7200000");
  85  | });
  86  | 
  87  | test("a list: append a policy", async ({ page, request, baseURL }, info) => {
  88  |   const name = fresh(info, "cfgl");
  89  |   await topic(request, baseURL!, name, 1, "event");
  90  |   await page.goto(configUrl(name, "?edit=cleanup.policy"));
  91  |   const d = page.getByTestId("dialog-config-edit");
  92  |   await expect(d.getByTestId("config-op").getByRole("radio", { name: /Append/ })).toHaveAttribute("aria-checked", "true");
  93  |   await d.getByTestId("config-value").fill("compact");
  94  |   await expect(d.getByTestId("the-call")).toContainText("APPEND");
  95  |   await d.getByTestId("config-save").click();
  96  |   await expect(d.getByTestId("saved-now")).toContainText("delete,compact");
  97  |   await page.keyboard.press("Escape");
  98  |   await expect(row(page, "cleanup.policy").getByTestId("config-value")).toHaveText("delete,compact");
  99  | });
  100 | 
  101 | test("the two lags together: min above max is refused in Kafka's words, Edit again, then saved", async ({ page, request, baseURL }, info) => {
  102 |   const name = fresh(info, "cfgg");
  103 |   await topic(request, baseURL!, name, 1, "compact");
  104 |   await page.goto(configUrl(name, "?edit=min.compaction.lag.ms"));
  105 |   const d = page.getByTestId("dialog-config-edit");
  106 |   await expect(d.getByRole("heading", { name: "Edit compaction lag" })).toBeVisible();
  107 |   await d.getByTestId("config-value").fill("50000");
  108 |   await d.getByTestId("config-other").fill("10000");
  109 |   await d.getByTestId("config-save").click();
  110 |   const refused = d.getByTestId("config-refused");
  111 |   await expect(refused.getByTestId("refusal-code")).toContainText("INVALID_CONFIG");
  112 |   const api = await call(request, baseURL!, `/api/v1/topics/${name}/config/edit`, { key: "min.compaction.lag.ms", op: "set", value: "50000", other: "10000" });
  113 |   await expect(refused.getByTestId("refusal-message")).toHaveText(api.body.message);
  114 |   await expect(refused.getByTestId("would-pass")).toBeVisible();
  115 |   await refused.getByTestId("edit-again").click();
  116 |   await d.getByTestId("config-value").fill("1000");
  117 |   await d.getByTestId("config-other").fill("60000");
  118 |   await d.getByTestId("config-save").click();
  119 |   await expect(d.getByTestId("saved-change")).toHaveCount(2);
  120 | });
  121 | 
  122 | test("a value Kafka will not take: refused in its own words, and nothing changed", async ({ page, request, baseURL }, info) => {
  123 |   const name = fresh(info, "cfgv");
  124 |   await topic(request, baseURL!, name, 1, "event");
```