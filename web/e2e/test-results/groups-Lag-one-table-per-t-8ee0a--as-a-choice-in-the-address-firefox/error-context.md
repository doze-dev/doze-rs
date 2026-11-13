# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: groups.spec.ts >> Lag: one table per topic with the lag as a bar, and when the commits expire as a choice in the address
- Location: tests/groups.spec.ts:149:1

# Error details

```
Error: expect(page).not.toHaveURL(expected) failed

Expected pattern: not /expiring=1/
Received string: "http://127.0.0.1:39094/groups/r2-idle-firefox/lag?expiring=1"
Timeout: 8000ms

Call log:
  - Expect "not toHaveURL" with timeout 8000ms
    19 × locator resolved to <html lang="en">…</html>
       - unexpected value "http://127.0.0.1:39094/groups/r2-idle-firefox/lag?expiring=1"

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
    - text: "3"
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
          - link "Groups":
            - /url: /groups
    - text: Updated 00:21:35 UTC
    - combobox "Refresh every": "Refresh: Off"
    - button "Refresh"
    - heading "r2-idle-firefox" [level=1]
    - paragraph: Empty classic no members
    - button "Reset offsets"
    - button "Delete offsets"
    - button "Delete group"
    - term: State
    - definition: Empty
    - term: Members
    - definition: "0"
    - term: Generation
    - definition: "4"
    - term: Lag
    - definition: "0"
    - text: records behind
    - term: Committed
    - definition: "3"
    - text: partitions
    - term: Last commit
    - definition: 00:20:10
    - text: UTC Switch group
    - link "fulfilment":
      - /url: /groups/fulfilment/lag
    - link "r2-busy-firefox":
      - /url: /groups/r2-busy-firefox/lag
    - text: r2-idle-firefox
    - navigation "Group":
      - link "Members 0":
        - /url: /groups/r2-idle-firefox
      - link "Lag 0":
        - /url: /groups/r2-idle-firefox/lag
      - link "Story 4":
        - /url: /groups/r2-idle-firefox/story
      - link "Wire":
        - /url: /groups/r2-idle-firefox/wire
    - radiogroup "Show":
      - radio "Lag now"
      - radio "When do these expire?" [checked]
    - text: "Lag is the log end minus the committed offset: records the app has not finished with, not records it has not fetched."
    - heading "Retention window" [level=2]
    - paragraph:
      - text: 7 d (
      - code: "--offsets-retention 168h"
      - text: ), counted from when the group lost its last member, Nov 13 00:21.
    - text: Nov 13 00:21 Nov 20 00:21 28 s gone 6 d 23 h left
    - link "r2-orders":
      - /url: /topics/r2-orders
    - text: 3 partitions, no member holds any
    - table "Lag of r2-orders":
      - rowgroup:
        - row "Partition Log end Committed Forgotten at Last commit":
          - columnheader "Partition":
            - button "Partition"
          - columnheader "Log end":
            - button "Log end"
          - columnheader "Committed":
            - button "Committed"
          - columnheader "Forgotten at":
            - button "Forgotten at"
          - columnheader "Last commit":
            - button "Last commit"
      - rowgroup:
        - row "r2-orders-0 8 8 Nov 20 00:21 Nov 13 00:20":
          - cell "r2-orders-0"
          - cell "8"
          - cell "8"
          - cell "Nov 20 00:21"
          - cell "Nov 13 00:20"
        - row "r2-orders-1 12 12 Nov 20 00:21 Nov 13 00:20":
          - cell "r2-orders-1"
          - cell "12"
          - cell "12"
          - cell "Nov 20 00:21"
          - cell "Nov 13 00:20"
        - row "r2-orders-2 10 10 Nov 20 00:21 Nov 13 00:20":
          - cell "r2-orders-2"
          - cell "10"
          - cell "10"
          - cell "Nov 20 00:21"
          - cell "Nov 13 00:20"
    - text: "What happens then. At the first sweep after that time the offsets go. A group with no members and no offsets is forgotten, and the id is free: a consumer that joins as r2-idle-firefox afterwards is a new group and starts at its"
    - code: auto.offset.reset
    - text: ". To keep them, start a consumer in r2-idle-firefox before then: a group with members keeps what it subscribes to."
  - heading "Jump to a topic or group" [level=2]
  - paragraph: Type a name, then Enter
- region "Notifications alt+T"
```

# Test source

```ts
  62  |   await page.getByTestId("groups-filter").fill("r2-");
  63  |   await expect(page).toHaveURL(/q=r2-/);
  64  |   const links = page.getByTestId("group-link");
  65  |   await expect(links.filter({ hasText: idle })).toBeVisible();
  66  |   for (const t of await links.allInnerTexts()) expect(t).toContain("r2-");
  67  |   // The states: a MultiSelect with how many groups each holds; Empty keeps only the empty ones.
  68  |   await page.getByRole("combobox", { name: "States" }).click();
  69  |   const empty = page.getByRole("option", { name: /Empty/ });
  70  |   await expect(empty).toContainText(/\d/);
  71  |   await empty.click();
  72  |   await page.keyboard.press("Escape");
  73  |   await expect(page).toHaveURL(/state=/);
  74  |   await expect(page.getByTestId("multi-summary")).toHaveText("Empty");
  75  |   await expect(links.filter({ hasText: idle })).toBeVisible();
  76  |   await expect(links.filter({ hasText: member })).toHaveCount(0);
  77  |   for (const row of await page.locator("tbody tr").all()) await expect(row.getByTestId("group-phase")).toHaveText("Empty");
  78  |   // Back undoes the filter (replace keeps history short: Back leaves the page's first state).
  79  |   await page.reload();
  80  |   await expect(page.getByTestId("multi-summary")).toHaveText("Empty");
  81  |   await expect(page.getByTestId("groups-filter")).toHaveValue("r2-");
  82  |   await page.getByRole("button", { name: "Clear states" }).click();
  83  |   await expect(page).not.toHaveURL(/state=/);
  84  |   await expect(links.filter({ hasText: member })).toBeVisible();
  85  |   await links.filter({ hasText: idle }).click();
  86  |   await expect(page).toHaveURL(new RegExp(`/groups/${idle}$`));
  87  |   await expect(page.locator("h1")).toHaveText(idle);
  88  | });
  89  | 
  90  | test("A group: the state, the figures as tiles, the tabs are routes, and Refresh moves nothing", async ({ page }) => {
  91  |   await page.goto(`/groups/${member}`);
  92  |   await expect(page.locator("h1")).toHaveText(member);
  93  |   await expect(page.locator("h1")).toHaveCount(1);
  94  |   await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toHaveText("Groups");
  95  |   await expect(page.getByTestId("group-pills")).toContainText("Stable");
  96  |   const strip = page.locator("[data-slot=stat-strip]").first();
  97  |   for (const label of ["State", "Members", "Generation", "Lag", "Committed", "Last commit"]) await expect(strip).toContainText(label);
  98  |   await expect(page.getByTestId("member-link")).toHaveCount(1);
  99  |   await expect(page.getByTestId("wire-dock")).toBeVisible();
  100 |   const tabs = page.getByTestId("group-tabs");
  101 |   for (const [name, path] of [["Lag", "/lag"], ["Story", "/story"], ["Wire", "/wire"], ["Members", ""]] as const) {
  102 |     await tabs.getByRole("link", { name: new RegExp(`^${name}`) }).click();
  103 |     await expect(page).toHaveURL(new RegExp(`/groups/${member}${path}$`));
  104 |     await expect(tabs.getByRole("link", { name: new RegExp(`^${name}`) })).toHaveAttribute("aria-current", "page");
  105 |   }
  106 |   await page.goBack();
  107 |   await expect(page).toHaveURL(new RegExp(`/groups/${member}/wire$`));
  108 |   await page.goBack();
  109 |   await expect(page).toHaveURL(new RegExp(`/groups/${member}/story$`));
  110 |   await expect(page.getByTestId("generation").first()).toBeVisible();
  111 |   // Refresh: the data is fetched again and nothing on the page moves.
  112 |   const selectors = ["h1", "[data-slot=stat-strip]", "[data-testid=group-tabs]", "[data-testid=story-timeline]"];
  113 |   const before = await boxes(page, selectors);
  114 |   const stamp = page.getByTestId("refresh-stamp");
  115 |   await page.waitForTimeout(1_100);
  116 |   const first = await stamp.textContent();
  117 |   await page.getByTestId("refresh-now").click();
  118 |   await expect(stamp).not.toHaveText(first ?? "");
  119 |   expect(await boxes(page, selectors)).toEqual(before);
  120 | });
  121 | 
  122 | test("A member opens as a Sheet with its own address: Esc closes it, the focus returns, Back and a deep link work", async ({ page }) => {
  123 |   await page.goto(`/groups/${member}`);
  124 |   const link = page.getByTestId("member-link").first();
  125 |   await link.click();
  126 |   const sheet = page.getByTestId("member-sheet");
  127 |   await expect(sheet).toBeVisible();
  128 |   await expect(page).toHaveURL(/member=/);
  129 |   await expect(sheet).toContainText(`${member}-app`);
  130 |   await expect(sheet.getByTestId("member-holds")).toContainText(TOPIC);
  131 |   await page.keyboard.press("Escape");
  132 |   await expect(sheet).toBeHidden();
  133 |   await expect(page).not.toHaveURL(/member=/);
  134 |   await expect(link).toBeFocused();
  135 |   // Opened again, Back closes it.
  136 |   await link.click();
  137 |   await expect(sheet).toBeVisible();
  138 |   const url = page.url();
  139 |   await page.goBack();
  140 |   await expect(sheet).toBeHidden();
  141 |   // A deep link opens the page with the Sheet on it; closing it stays on the page.
  142 |   await page.goto(url);
  143 |   await expect(sheet).toBeVisible();
  144 |   await sheet.getByRole("button", { name: "Close" }).first().click();
  145 |   await expect(sheet).toBeHidden();
  146 |   await expect(page).toHaveURL(new RegExp(`/groups/${member}$`));
  147 | });
  148 | 
  149 | test("Lag: one table per topic with the lag as a bar, and when the commits expire as a choice in the address", async ({ page }) => {
  150 |   await page.goto(`/groups/${idle}/lag`);
  151 |   const block = page.getByTestId("lag-topic").filter({ has: page.getByRole("link", { name: TOPIC }) });
  152 |   await expect(block).toBeVisible();
  153 |   await expect(block.locator("tbody tr")).toHaveCount(3);
  154 |   await expect(block).toContainText("0 behind");
  155 |   await expect(page.getByTestId("lag-explain")).toBeVisible();
  156 |   await page.getByTestId("lag-view").getByRole("radio", { name: "When do these expire?" }).click();
  157 |   await expect(page).toHaveURL(/expiring=1/);
  158 |   await expect(page.getByText("Retention window")).toBeVisible();
  159 |   await expect(block.getByRole("columnheader", { name: "Forgotten at" })).toBeVisible();
  160 |   await expect(page.getByTestId("lag-explain")).toHaveAttribute("data-state", "expiring");
  161 |   await page.goBack();
> 162 |   await expect(page).not.toHaveURL(/expiring=1/);
      |                          ^ Error: expect(page).not.toHaveURL(expected) failed
  163 |   await expect(block.getByRole("columnheader", { name: "Lag" })).toBeVisible();
  164 | });
  165 | 
  166 | test("Story: the generations newest first; a rebalance opens as a Sheet, the flapping rule as another", async ({ page }) => {
  167 |   await page.goto(`/groups/${member}/story`);
  168 |   const gens = page.getByTestId("generation");
  169 |   await expect(gens.first()).toContainText("Member joined");
  170 |   await expect(page.locator("[data-slot=stat-strip]").nth(1)).toContainText("Rebalances kept");
  171 |   const open = page.getByTestId("open-rebalance").first();
  172 |   await open.click();
  173 |   const sheet = page.getByTestId("rebalance-sheet");
  174 |   await expect(sheet).toBeVisible();
  175 |   await expect(page).toHaveURL(/generation=\d+/);
  176 |   await expect(sheet).toContainText("Began");
  177 |   await expect(sheet).toContainText("Members");
  178 |   await page.keyboard.press("Escape");
  179 |   await expect(sheet).toBeHidden();
  180 |   await expect(page).not.toHaveURL(/generation=/);
  181 |   await expect(open).toBeFocused();
  182 |   // A quiet group's page says so; one with more than one rebalance has the flapping card.
  183 |   const rule = page.getByTestId("check-rule");
  184 |   if (await rule.count()) {
  185 |     await rule.click();
  186 |     await expect(page.getByTestId("rule-sheet")).toBeVisible();
  187 |     await expect(page).toHaveURL(/rule=1/);
  188 |     await page.goBack();
  189 |     await expect(page.getByTestId("rule-sheet")).toBeHidden();
  190 |   } else await expect(page.getByTestId("story-quiet")).toContainText("A quiet story is a good one");
  191 | });
  192 | 
  193 | test("Wire: the requests that touched the group, filtered by kind and by text in the address", async ({ page }) => {
  194 |   await page.goto(`/groups/${member}/wire`);
  195 |   const table = page.getByRole("table", { name: "Requests that touched this group" });
  196 |   await expect(table.locator("tbody tr").first()).toBeVisible();
  197 |   const chips = page.getByTestId("wire-chips");
  198 |   await chips.getByRole("radio", { name: /^Commits/ }).click();
  199 |   await expect(page).toHaveURL(/kind=commits/);
  200 |   await expect(chips.getByRole("radio", { name: /^Commits/ })).toHaveAttribute("aria-checked", "true");
  201 |   for (const t of await table.locator("tbody tr td:nth-child(3)").allInnerTexts()) expect(t).toMatch(/Offset/);
  202 |   await chips.getByRole("radio", { name: /^Join and sync/ }).click();
  203 |   await expect(page).toHaveURL(/kind=join/);
  204 |   await page.getByTestId("wire-filter").fill(`${member}-app`);
  205 |   await expect(page).toHaveURL(/q=/);
  206 |   await chips.getByRole("radio", { name: /^All/ }).click();
  207 |   await expect(page).not.toHaveURL(/kind=/);
  208 |   for (const t of await table.locator("tbody tr td:nth-child(2)").allInnerTexts()) expect(t).toContain(`${member}-app`);
  209 | });
  210 | 
  211 | test("Reset offsets: a GET preview, then the reset; the Lag tab shows where the group now reads", async ({ page }) => {
  212 |   await page.goto(`/groups/${idle}/lag`);
  213 |   await page.getByTestId("open-reset").click();
  214 |   const dialog = page.getByTestId("dialog-reset");
  215 |   await expect(dialog).toBeVisible();
  216 |   await expect(page).toHaveURL(/dialog=reset/);
  217 |   await expect(dialog).toContainText("has no members");
  218 |   // A value that is not a number: the preview says so in the console's words, and Reset is off.
  219 |   await dialog.getByRole("radio", { name: /A specific offset/ }).click();
  220 |   await dialog.getByTestId("reset-value").fill("abc");
  221 |   await dialog.getByTestId("reset-preview-button").click();
  222 |   await expect(dialog.getByTestId("reset-error")).toHaveText("The offset must be a whole number.");
  223 |   await expect(dialog.getByTestId("reset-submit")).toBeDisabled();
  224 |   // A time, chosen with the picker (never the browser's own widget): a day of the grid and a time typed.
  225 |   await dialog.getByRole("radio", { name: /A time/ }).click();
  226 |   await dialog.getByTestId("datetime-trigger").click();
  227 |   const picker = page.getByTestId("datetime-popover");
  228 |   await picker.getByRole("button", { name: /^1 / }).click();
  229 |   await picker.getByTestId("datetime-clock").fill("00:00:00");
  230 |   await picker.getByTestId("datetime-done").click();
  231 |   await expect(dialog.getByTestId("datetime-trigger")).toContainText(/\d{4}-\d{2}-01 00:00:00 UTC/);
  232 |   await dialog.getByTestId("reset-preview-button").click();
  233 |   await expect(dialog.getByTestId("reset-error")).toHaveCount(0);
  234 |   // Earliest: every partition goes to its log start.
  235 |   await dialog.getByRole("radio", { name: /Earliest/ }).click();
  236 |   await expect(dialog.getByTestId("reset-submit")).toBeDisabled();
  237 |   await dialog.getByTestId("reset-preview-button").click();
  238 |   await expect(dialog.getByTestId("reset-after")).toHaveText(["0", "0", "0"]);
  239 |   await expect(dialog.getByTestId("reset-submit")).toHaveText(/Reset 3 offsets/);
  240 |   await dialog.getByTestId("reset-submit").click();
  241 |   await expect(page.locator("[data-sonner-toast]").filter({ hasText: "Reset 3 offsets" })).toBeVisible();
  242 |   await expect(dialog).toBeHidden();
  243 |   await expect(page).not.toHaveURL(/dialog=/);
  244 |   const block = page.getByTestId("lag-topic").filter({ has: page.getByRole("link", { name: TOPIC }) });
  245 |   await expect(block.locator("tbody tr td:nth-child(3)")).toHaveText(["0", "0", "0"]);
  246 |   await expect(block).not.toContainText("0 behind");
  247 |   // Back to the end, with the keyboard: Esc cancels, Enter in the dialog previews.
  248 |   await page.getByTestId("open-reset").click();
  249 |   await expect(dialog).toBeVisible();
  250 |   await page.keyboard.press("Escape");
  251 |   await expect(dialog).toBeHidden();
  252 |   await page.getByTestId("open-reset").click();
  253 |   await dialog.getByRole("radio", { name: /Latest/ }).click();
  254 |   await dialog.getByTestId("reset-preview-button").click();
  255 |   await dialog.getByTestId("reset-submit").click();
  256 |   await expect(block).toContainText("0 behind");
  257 | });
  258 | 
  259 | test("Reset offsets of a group with a member is refused, in Kafka's name and words, and nothing changes", async ({ page }) => {
  260 |   await page.goto(`/groups/${member}?dialog=reset`);
  261 |   const dialog = page.getByTestId("dialog-reset");
  262 |   await expect(dialog.getByTestId("reset-not-empty")).toContainText("This group is not empty.");
```